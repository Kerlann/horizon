//! Parts (arismoko/horizon#41): code a mod attaches to one block, block entity, item or mod
//! menu type at startup. The scoped hooks ask the parts attached to the thing they are about,
//! after every mod's `Hooks` method. Parts attached by tag are resolved by each game against its
//! own tags, at world open and on `/reload`, into a table kept by that game's thread.
use super::{BlockEntityPart, BlockPart, Hooks, ItemPart, MenuPart};
use std::{
    any::{TypeId, type_name},
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    rc::Rc,
    sync::{
        Arc, LazyLock, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};

/// A part, with the mod that attached it.
pub struct Attached<P: ?Sized> {
    /// The mod's place in dispatch order.
    pub(super) mod_index: usize,
    /// Its place in its mod's registrations: a mod's parts on one thing keep this order, key or
    /// tag (#41 §3).
    order: usize,
    /// The part's kind: one per thing.
    kind: TypeId,
    pub(super) type_name: &'static str,
    pub(super) part: Arc<P>,
}

impl<P: ?Sized> Clone for Attached<P> {
    fn clone(&self) -> Self {
        Self {
            mod_index: self.mod_index,
            order: self.order,
            kind: self.kind,
            type_name: self.type_name,
            part: self.part.clone(),
        }
    }
}

/// Every mod's parts, by the id of the thing they are attached to, in dispatch order.
#[derive(Clone, Default)]
pub struct Parts {
    /// Block parts, and block entity parts as the block parts they also are.
    blocks: HashMap<Box<str>, Vec<Attached<dyn BlockPart>>>,
    /// Block entity parts by block, for the hooks asked by block (`block_entity`, `screen`).
    block_entities: HashMap<Box<str>, Vec<Attached<dyn BlockEntityPart>>>,
    /// Block entity parts by block entity type, for the hooks asked by the entity
    /// (`block_entity_tick`, `menu_button`): its type id need not be its block's id.
    block_entity_kinds: HashMap<Box<str>, Vec<Attached<dyn BlockEntityPart>>>,
    items: HashMap<Box<str>, Vec<Attached<dyn ItemPart>>>,
    menus: HashMap<Box<str>, Vec<Attached<dyn MenuPart>>>,
}

/// The parts attached by tag, by tag name, in registration order.
#[derive(Default)]
struct TagParts {
    blocks: Vec<(Box<str>, Attached<dyn BlockPart>)>,
    items: Vec<(Box<str>, Attached<dyn ItemPart>)>,
}

/// What every mod's [`Hooks::register`] attached.
#[derive(Default)]
struct Installed {
    keys: Parts,
    tags: TagParts,
}

static INSTALLED: OnceLock<Installed> = OnceLock::new();
static NO_PARTS: LazyLock<Parts> = LazyLock::new(Parts::default);
static OFF_THREAD_WARNED: AtomicBool = AtomicBool::new(false);

thread_local! {
    /// This thread's game's parts, its tags resolved; none before its world opens.
    static GAME_PARTS: RefCell<Option<Rc<Parts>>> = const { RefCell::new(None) };
    /// Whether a `Game` runs on this thread.
    static GAME_THREAD: Cell<bool> = const { Cell::new(false) };
}

/// The parts table a dispatch reads: the process-wide one, or this game thread's.
pub(crate) enum PartsRef {
    Shared(&'static Parts),
    Game(Rc<Parts>),
}

impl std::ops::Deref for PartsRef {
    type Target = Parts;
    fn deref(&self) -> &Parts {
        match self {
            Self::Shared(parts) => parts,
            Self::Game(parts) => parts,
        }
    }
}

/// The parts hook `hook` asks: with no tag parts, the key parts every game shares; otherwise this
/// thread's game's, or the key parts until its world opens.
pub(crate) fn parts(hook: &'static str) -> PartsRef {
    let Some(installed) = INSTALLED.get() else {
        return PartsRef::Shared(&NO_PARTS);
    };
    if installed.tags.blocks.is_empty() && installed.tags.items.is_empty() {
        return PartsRef::Shared(&installed.keys);
    }
    if let Some(parts) = GAME_PARTS.with_borrow(Option::clone) {
        return PartsRef::Game(parts);
    }
    if !GAME_THREAD.get() {
        let thread = std::thread::current();
        let thread = thread.name().unwrap_or("unnamed");
        debug_assert!(false, "hook {hook} dispatched off a game thread ({thread})");
        if !OFF_THREAD_WARNED.swap(true, Ordering::Relaxed) {
            log::warn!(
                "Native hook {hook} ran off a game thread ({thread}): parts attached by tag are not asked"
            );
        }
    }
    PartsRef::Shared(&installed.keys)
}

/// Mark this thread as running a `Game`, whose dispatches read its own resolved parts.
pub(crate) fn mark_game_thread() {
    GAME_THREAD.set(true);
}

/// The ids a game knows, which a part's key must name.
pub(crate) struct KnownIds<'a> {
    pub block: &'a dyn Fn(&str) -> bool,
    pub item: &'a dyn Fn(&str) -> bool,
}

/// Resolve this thread's game's parts against its tags (`block_tags`, `item_tags`: flattened,
/// tag name without `#` to members). Each run starts again from the key parts, so `/reload` drops
/// what left a tag. A key naming a block or item the game doesn't know is an error, naming the
/// mod; a missing tag or a part a tag gives twice is a warning, and the earlier mod's part stays.
pub(crate) fn resolve_parts(
    block_tags: &HashMap<String, HashSet<String>>,
    item_tags: &HashMap<String, HashSet<String>>,
    known: KnownIds<'_>,
) -> Result<(), Vec<String>> {
    let Some(installed) = INSTALLED.get() else {
        return Ok(());
    };
    let mut unknown: Vec<String> = unknown_keys(&installed.keys.blocks, known.block, "block")
        .chain(unknown_keys(&installed.keys.items, known.item, "item"))
        .collect();
    if !unknown.is_empty() {
        unknown.sort();
        return Err(unknown);
    }
    if installed.tags.blocks.is_empty() && installed.tags.items.is_empty() {
        return Ok(());
    }
    let mut parts = installed.keys.clone();
    add_tagged(
        &mut parts.blocks,
        &installed.tags.blocks,
        block_tags,
        "block",
    );
    add_tagged(&mut parts.items, &installed.tags.items, item_tags, "item");
    GAME_PARTS.set(Some(Rc::new(parts)));
    Ok(())
}

/// A line for each part attached to an id `known` refuses.
fn unknown_keys<'a, P: ?Sized>(
    table: &'a HashMap<Box<str>, Vec<Attached<P>>>,
    known: &'a dyn Fn(&str) -> bool,
    scope: &'a str,
) -> impl Iterator<Item = String> + 'a {
    table
        .iter()
        .filter(|(id, _)| !known(id))
        .flat_map(move |(id, list)| {
            list.iter().map(move |part| {
                format!(
                    "{} attached {} to {scope} {id}, which this game does not have",
                    super::mod_name(part.mod_index),
                    part.type_name
                )
            })
        })
}

/// Attach each tag part to its tag's members, in dispatch order: by mod, then by registration
/// within a mod.
fn add_tagged<P: ?Sized>(
    table: &mut HashMap<Box<str>, Vec<Attached<P>>>,
    tagged: &[(Box<str>, Attached<P>)],
    tags: &HashMap<String, HashSet<String>>,
    scope: &str,
) {
    for (tag, part) in tagged {
        let Some(members) = tags.get(&**tag) else {
            log::warn!(
                "{} attached {} to {scope} tag #{tag}, which does not exist",
                super::mod_name(part.mod_index),
                part.type_name
            );
            continue;
        };
        for member in members {
            let list = table.entry(member.as_str().into()).or_default();
            match list.iter_mut().find(|a| a.kind == part.kind) {
                Some(other) => {
                    log::warn!(
                        "{} is attached to {member} twice, by {} and {} (#{tag}); {}'s is kept",
                        part.type_name,
                        super::mod_name(other.mod_index),
                        super::mod_name(part.mod_index),
                        super::mod_name(other.mod_index.min(part.mod_index)),
                    );
                    if part.mod_index < other.mod_index {
                        *other = part.clone();
                    }
                }
                None => list.push(part.clone()),
            }
            list.sort_by_key(|a| (a.mod_index, a.order));
        }
    }
}

impl Parts {
    // Each takes the thing's id as a closure: with nothing attached in that scope the id is never
    // worked out, so a game with no parts pays no registry read on the hot hooks.
    pub(crate) fn block<'k>(
        &self,
        id: impl FnOnce() -> Option<&'k str>,
    ) -> &[Attached<dyn BlockPart>] {
        attached(&self.blocks, id)
    }
    pub(crate) fn block_entity<'k>(
        &self,
        id: impl FnOnce() -> Option<&'k str>,
    ) -> &[Attached<dyn BlockEntityPart>] {
        attached(&self.block_entities, id)
    }
    pub(crate) fn block_entity_kind<'k>(
        &self,
        id: impl FnOnce() -> Option<&'k str>,
    ) -> &[Attached<dyn BlockEntityPart>] {
        attached(&self.block_entity_kinds, id)
    }
    pub(crate) fn item<'k>(
        &self,
        id: impl FnOnce() -> Option<&'k str>,
    ) -> &[Attached<dyn ItemPart>] {
        attached(&self.items, id)
    }
    pub(crate) fn menu<'k>(
        &self,
        id: impl FnOnce() -> Option<&'k str>,
    ) -> &[Attached<dyn MenuPart>] {
        attached(&self.menus, id)
    }
}

fn attached<'a, 'k, P: ?Sized>(
    table: &'a HashMap<Box<str>, Vec<Attached<P>>>,
    id: impl FnOnce() -> Option<&'k str>,
) -> &'a [Attached<P>] {
    if table.is_empty() {
        return &[];
    }
    id().and_then(|id| table.get(id)).map_or(&[], Vec::as_slice)
}

/// What a mod's [`Hooks::register`] attaches its parts with. A thing takes one part of each
/// kind: the same kind twice on one thing refuses the mods at startup, naming both.
#[derive(Default)]
pub struct Registrar {
    mod_index: usize,
    /// Parts registered so far, by every mod: the next part's `order`.
    registered: usize,
    parts: Parts,
    tags: TagParts,
    refused: Vec<String>,
}

impl Registrar {
    /// Attach `part` to block `block` (`mymod:shaft`, `minecraft:oak_log`).
    pub fn block_part<P: BlockPart>(&mut self, block: &str, part: P) {
        self.attach::<P, dyn BlockPart>(|p| &mut p.blocks, block, Arc::new(part));
    }
    /// Attach `part` to the mod block entity type `kind` (the id a `block_entity` hook gives the
    /// entity, `mymod:press`) and to `blocks`, the blocks that hold one. It answers the hooks
    /// asked by the entity (`block_entity_tick`, `menu_button`), those asked by block
    /// (`block_entity`, `screen`) and every block hook of those blocks.
    pub fn block_entity_part<P: BlockEntityPart>(&mut self, kind: &str, blocks: &[&str], part: P) {
        let part = Arc::new(part);
        if !self.attach::<P, dyn BlockEntityPart>(|p| &mut p.block_entity_kinds, kind, part.clone())
        {
            return;
        }
        for block in blocks {
            // Every block entity part is a block part too, so a second one is found there.
            if self.attach::<P, dyn BlockPart>(|p| &mut p.blocks, block, part.clone()) {
                self.attach::<P, dyn BlockEntityPart>(
                    |p| &mut p.block_entities,
                    block,
                    part.clone(),
                );
            }
        }
    }
    /// Attach `part` to every block in tag `tag` (`minecraft:logs`, without `#`), as each game's
    /// tags have it when its world opens and after `/reload`.
    pub fn block_part_tag<P: BlockPart>(&mut self, tag: &str, part: P) {
        let part = self.tagged::<P, dyn BlockPart>(tag, Arc::new(part));
        self.tags.blocks.extend(part);
    }
    /// Attach `part` to every item in tag `tag` (`minecraft:axes`, without `#`).
    pub fn item_part_tag<P: ItemPart>(&mut self, tag: &str, part: P) {
        let part = self.tagged::<P, dyn ItemPart>(tag, Arc::new(part));
        self.tags.items.extend(part);
    }
    /// Attach `part` to item `item` (`mymod:wrench`).
    pub fn item_part<P: ItemPart>(&mut self, item: &str, part: P) {
        self.attach::<P, dyn ItemPart>(|p| &mut p.items, item, Arc::new(part));
    }
    /// Attach `part` to the mod menu type `menu`.
    pub fn menu_part<P: MenuPart>(&mut self, menu: &str, part: P) {
        self.attach::<P, dyn MenuPart>(|p| &mut p.menus, menu, Arc::new(part));
    }

    /// `part`, of kind `K`, for tag `tag`; none if it is refused.
    fn tagged<K: 'static, P: ?Sized>(
        &mut self,
        tag: &str,
        part: Arc<P>,
    ) -> Option<(Box<str>, Attached<P>)> {
        let name = type_name::<K>();
        if tag.starts_with('#') || !namespaced(tag) {
            self.refused.push(format!(
                "{} attached {name} to tag {tag:?}, which is not a namespaced tag name (write it without #)",
                super::mod_name(self.mod_index)
            ));
            return None;
        }
        self.registered += 1;
        Some((
            tag.into(),
            Attached {
                mod_index: self.mod_index,
                order: self.registered,
                kind: TypeId::of::<K>(),
                type_name: name,
                part,
            },
        ))
    }

    /// Attach `part`, of kind `K`, to `id` in `table`; false if it is refused.
    fn attach<K: 'static, P: ?Sized>(
        &mut self,
        table: fn(&mut Parts) -> &mut HashMap<Box<str>, Vec<Attached<P>>>,
        id: &str,
        part: Arc<P>,
    ) -> bool {
        let name = type_name::<K>();
        if !namespaced(id) {
            self.refused.push(format!(
                "{} attached {name} to {id:?}, which is not a namespaced id",
                super::mod_name(self.mod_index)
            ));
            return false;
        }
        let list = table(&mut self.parts).entry(id.into()).or_default();
        if let Some(other) = list.iter().find(|a| a.kind == TypeId::of::<K>()) {
            self.refused.push(format!(
                "{name} is attached to {id} twice, by {} and {}",
                super::mod_name(other.mod_index),
                super::mod_name(self.mod_index)
            ));
            return false;
        }
        self.registered += 1;
        list.push(Attached {
            mod_index: self.mod_index,
            order: self.registered,
            kind: TypeId::of::<K>(),
            type_name: name,
            part,
        });
        true
    }
}

fn namespaced(id: &str) -> bool {
    id.split_once(':')
        .is_some_and(|(namespace, path)| !namespace.is_empty() && !path.is_empty())
}

/// Builds the parts from every mod's [`Hooks::register`], in dispatch order. Once, at startup;
/// the error lists every part refused, naming its mods.
pub(crate) fn install_parts(mods: &[Box<dyn Hooks>]) -> Result<(), Vec<String>> {
    let mut registrar = Registrar::default();
    for (index, hooks) in super::entered(mods, "register").enumerate() {
        registrar.mod_index = index;
        hooks.register(&mut registrar);
    }
    if !registrar.refused.is_empty() {
        return Err(registrar.refused);
    }
    let installed = Installed {
        keys: registrar.parts,
        tags: registrar.tags,
    };
    if INSTALLED.set(installed).is_err() {
        log::warn!("Mod parts already installed; keeping the first table");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use super::{Attached, KnownIds, mark_game_thread, resolve_parts};
    use std::any::{TypeId, type_name};
    use std::collections::{HashMap, HashSet};
    use std::sync::Arc;

    /// A comparator reading of 5 more than it was.
    struct Gauge;
    impl BlockPart for Gauge {
        fn analog_output(&self, _: &dyn LevelHost, event: &mut AnalogOutputEvent) {
            event.output += 5;
        }
    }

    /// The block entity of type `test:press_entity`, held by block `test:press`: it ticks, and
    /// each tick adds a data slot.
    struct Press;
    impl BlockPart for Press {}
    impl BlockEntityPart for Press {
        fn block_entity_ticks(&self, _: &dyn Host, event: &mut BlockEntityTicksEvent<'_>) {
            event.ticks = true;
        }
        fn block_entity_tick(&self, _: &mut dyn Host, event: &mut BlockEntityTickEvent<'_>) {
            event.entity.data.push(1);
        }
    }
    struct NoHost;
    impl Host for NoHost {}

    /// A comparator reading of 100 more, for the blocks in tag `test:gauged`.
    struct TagGauge;
    impl BlockPart for TagGauge {
        fn analog_output(&self, _: &dyn LevelHost, event: &mut AnalogOutputEvent) {
            event.output += 100;
        }
    }

    /// Doubles every comparator reading, and attaches `Gauge` to `test:gauge`, `TagGauge` to
    /// tag `test:gauged` and `Press` to `test:press`.
    struct Doubler;
    impl Hooks for Doubler {
        fn register(&self, r: &mut Registrar) {
            r.block_part("test:gauge", Gauge);
            r.block_part_tag("test:gauged", TagGauge);
            r.block_entity_part("test:press_entity", &["test:press"], Press);
        }
        fn analog_output(&self, _: &dyn LevelHost, event: &mut AnalogOutputEvent) {
            event.output *= 2;
        }
    }

    /// State 1 is a `test:gauge`, state 2 a `test:other`.
    struct Level;
    impl LevelHost for Level {
        fn block_state(&self, _: [i32; 3]) -> BlockStateId {
            unimplemented!()
        }
        fn is_client_side(&self) -> bool {
            false
        }
        fn state_with_property(&self, _: BlockStateId, _: &str, _: &str) -> Option<BlockStateId> {
            None
        }
        fn state_block(&self, state: BlockStateId) -> Option<&str> {
            ["test:gauge", "test:other"]
                .get(state.0 as usize - 1)
                .copied()
        }
    }

    /// The test mods, their parts installed, on a thread that runs a game.
    fn game_thread() -> Vec<Box<dyn Hooks>> {
        let mods: Vec<Box<dyn Hooks>> = vec![Box::new(Doubler)];
        install_parts(&mods).unwrap();
        mark_game_thread();
        mods
    }

    /// The comparator reading of state `state`, from 1.
    fn read(mods: &[Box<dyn Hooks>], state: u32) -> i32 {
        let mut output = 1;
        dispatch_analog_output(
            &Level,
            mods,
            LevelDimension::Overworld,
            BlockPos::new(0, 64, 0),
            BlockStateId(state),
            Direction::North,
            &mut output,
        );
        output
    }

    /// Resolve this thread's parts with tag `test:gauged` holding `members`.
    fn resolve(members: &[&str]) -> Result<(), Vec<String>> {
        let tags = HashMap::from([(
            "test:gauged".to_owned(),
            members.iter().map(|m| m.to_string()).collect(),
        )]);
        resolve_parts(
            &tags,
            &HashMap::new(),
            KnownIds {
                block: &|_| true,
                item: &|_| true,
            },
        )
    }

    #[test]
    fn a_block_part_answers_for_its_block_only_after_the_mods() {
        let mods = game_thread();
        // Doubled by the mod, then 5 more from the part: (1 * 2) + 5. No world is open, so the
        // tag part is not attached yet.
        assert_eq!(read(&mods, 1), 7);
        assert_eq!(read(&mods, 2), 2);
    }

    #[test]
    fn each_game_answers_from_its_own_tags_and_reload_drops_what_left() {
        let mods = game_thread();
        // Both games resolve before either reads, so one shared table would fail one of them.
        let resolved = std::sync::Barrier::new(2);
        std::thread::scope(|scope| {
            let client = scope.spawn(|| {
                mark_game_thread();
                resolve(&["test:other"]).unwrap();
                resolved.wait();
                (read(&mods, 1), read(&mods, 2))
            });
            let server = scope.spawn(|| {
                mark_game_thread();
                resolve(&["test:gauge"]).unwrap();
                resolved.wait();
                let before = (read(&mods, 1), read(&mods, 2));
                // `/reload` with the tag emptied.
                resolve(&[]).unwrap();
                (before, read(&mods, 1))
            });
            // The tag part comes after the key part of the same mod: (1 * 2) + 5 + 100.
            assert_eq!(client.join().unwrap(), (7, 102));
            assert_eq!(server.join().unwrap(), ((107, 2), 7));
        });
    }

    #[test]
    fn a_key_the_game_lacks_fails_naming_the_mod() {
        game_thread();
        let refused = resolve_parts(
            &HashMap::new(),
            &HashMap::new(),
            KnownIds {
                block: &|id| id != "test:gauge",
                item: &|_| true,
            },
        )
        .unwrap_err();
        assert_eq!(refused.len(), 1, "{refused:?}");
        assert!(refused[0].contains("Gauge to block test:gauge, which this game does not have"));
    }

    #[test]
    fn a_tag_giving_a_part_twice_keeps_the_earlier_mod() {
        let part = |mod_index| Attached::<dyn BlockPart> {
            mod_index,
            order: 0,
            kind: TypeId::of::<Gauge>(),
            type_name: "Gauge",
            part: Arc::new(Gauge),
        };
        let mut table = HashMap::from([("test:gauge".into(), vec![part(1)])]);
        let tags = HashMap::from([(
            "test:gauged".to_owned(),
            HashSet::from(["test:gauge".to_owned()]),
        )]);
        super::add_tagged(
            &mut table,
            &[("test:gauged".into(), part(0))],
            &tags,
            "block",
        );
        let kept: Vec<usize> = table["test:gauge"].iter().map(|a| a.mod_index).collect();
        assert_eq!(kept, [0]);
    }

    #[test]
    fn a_mods_tag_part_registered_first_runs_before_its_key_part() {
        let mut r = Registrar::default();
        r.block_part_tag("test:gauged", TagGauge);
        r.block_part("test:gauge", Gauge);
        let tags = HashMap::from([(
            "test:gauged".to_owned(),
            HashSet::from(["test:gauge".to_owned()]),
        )]);
        super::add_tagged(&mut r.parts.blocks, &r.tags.blocks, &tags, "block");
        let order: Vec<&str> = r.parts.blocks["test:gauge"]
            .iter()
            .map(|a| a.type_name)
            .collect();
        assert_eq!(order, [type_name::<TagGauge>(), type_name::<Gauge>()]);
    }

    #[test]
    fn a_block_entity_part_answers_by_block_and_by_entity_type() {
        let mods = game_thread();
        let ticks = |block| {
            let mut ticks = false;
            dispatch_block_entity_ticks(&NoHost, &mods, block, &mut ticks);
            ticks
        };
        // The ticker is asked by block: the type id is not a block.
        assert!(ticks("test:press"));
        assert!(!ticks("test:press_entity"));
        let tick = |kind: &str| {
            let mut entity = ModBlockEntity::default();
            entity.id = kind.into();
            dispatch_block_entity_tick(&mut NoHost, &mods, [0, 64, 0], &mut entity);
            entity.data.len()
        };
        // The tick is asked by the entity's type, which differs from its block's id.
        assert_eq!(tick("test:press_entity"), 1);
        assert_eq!(tick("test:press"), 0);
    }

    #[test]
    fn with_no_parts_the_block_is_never_named() {
        let parts = Parts::default();
        let named = std::cell::Cell::new(false);
        let id = || {
            named.set(true);
            Some("test:gauge")
        };
        assert!(parts.block(id).is_empty());
        assert!(!named.get(), "an empty table resolved the state's block");
    }

    #[test]
    fn the_same_part_twice_on_one_block_is_refused() {
        struct Other;
        impl BlockPart for Other {}
        let mut r = Registrar::default();
        r.block_part("test:gauge", Gauge);
        r.mod_index = 1;
        r.block_part("test:gauge", Other);
        r.block_part("test:gauge", Gauge);
        r.block_part("gauge", Other);
        r.block_part_tag("#test:gauged", Other);
        assert_eq!(r.parts.block(|| Some("test:gauge")).len(), 2);
        assert_eq!(r.refused.len(), 3, "{:?}", r.refused);
        assert!(r.refused[0].contains("Gauge is attached to test:gauge twice"));
        assert!(r.refused[1].contains("not a namespaced id"));
        assert!(r.refused[2].contains("write it without #"));
    }
}
