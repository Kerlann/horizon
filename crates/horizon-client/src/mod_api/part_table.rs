//! Parts (arismoko/horizon#41): code a mod attaches to one block, block entity, item or mod
//! menu type at startup. The scoped hooks ask the parts attached to the thing they are about,
//! after every mod's `Hooks` method. Parts attached by tag are resolved by each game against its
//! own tags, at world open and on `/reload`, into a table kept by that game's thread.
use super::{BlockEntityPart, BlockPart, CapabilityKey, Hooks, ItemPart, MenuPart};
use std::{
    any::{Any, TypeId, type_name},
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
    /// Capability handlers by capability name, then by block entity type: one each.
    capabilities: HashMap<&'static str, HashMap<Box<str>, CapabilityPart>>,
}

/// A capability handler a mod registered: an `Arc` of the capability's handler trait.
#[derive(Clone)]
struct CapabilityPart {
    mod_index: usize,
    handler: Arc<dyn Any + Send + Sync>,
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

/// The handler for `key` a mod registered for the block entity type `kind`. With none registered
/// for any type, `kind` is never worked out.
pub fn block_entity_capability<'k, T: ?Sized + 'static>(
    key: &CapabilityKey<T>,
    kind: impl FnOnce() -> Option<&'k str>,
) -> Option<Arc<T>> {
    let handlers = INSTALLED.get()?.keys.capabilities.get(key.name)?;
    let part = handlers.get(kind()?)?;
    part.handler.downcast_ref::<Arc<T>>().cloned()
}

/// The mod that registered `handler` for a capability; none for the game's own.
pub(crate) fn capability_mod<T: ?Sized + 'static>(handler: &Arc<T>) -> Option<usize> {
    let parts = INSTALLED.get()?.keys.capabilities.values();
    parts
        .flat_map(HashMap::values)
        .find(|part| {
            part.handler
                .downcast_ref::<Arc<T>>()
                .is_some_and(|h| std::ptr::addr_eq(Arc::as_ptr(h), Arc::as_ptr(handler)))
        })
        .map(|part| part.mod_index)
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
    /// Answer capability `key` for the mod block entity type `kind` (`mymod:press`) with
    /// `handler`, in place of the game's own answer: `r.block_entity_capability(&ITEMS,
    /// "mymod:press", Arc::new(PressItems))`. One handler a capability and type.
    pub fn block_entity_capability<T: ?Sized + Send + Sync + 'static>(
        &mut self,
        key: &CapabilityKey<T>,
        kind: &str,
        handler: Arc<T>,
    ) {
        if !namespaced(kind) {
            self.refused.push(format!(
                "{} answered {} for {kind:?}, which is not a namespaced id",
                super::mod_name(self.mod_index),
                key.name
            ));
            return;
        }
        let handlers = self.parts.capabilities.entry(key.name).or_default();
        if let Some(other) = handlers.get(kind) {
            self.refused.push(format!(
                "{} is answered for {kind} twice, by {} and {}",
                key.name,
                super::mod_name(other.mod_index),
                super::mod_name(self.mod_index)
            ));
            return;
        }
        handlers.insert(
            kind.into(),
            CapabilityPart {
                mod_index: self.mod_index,
                handler: Arc::new(handler),
            },
        );
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
            r.block_entity_capability(&ITEMS, "test:pipe", Arc::new(Pipe));
            r.block_entity_capability(&FLUIDS, "test:tank", Arc::new(Tank));
            r.block_entity_capability(&FLUIDS, "test:liar", Arc::new(Liar));
        }
        fn analog_output(&self, _: &dyn LevelHost, event: &mut AnalogOutputEvent) {
            event.output *= 2;
        }
    }

    /// A pipe's item capability: one slot from every side but its top, and what goes in is
    /// handed to the container east of it, through that container's west side.
    struct Pipe;
    impl ItemHandler for Pipe {
        fn slots(&self, _: &dyn Host, _: [i32; 3], side: Option<Direction>) -> usize {
            usize::from(side != Some(Direction::Up))
        }
        fn stack(
            &self,
            _: &dyn Host,
            _: [i32; 3],
            _: Option<Direction>,
            _: usize,
        ) -> Option<ItemStack> {
            None
        }
        fn insert(
            &self,
            host: &mut dyn Host,
            at: [i32; 3],
            _: Option<Direction>,
            _: usize,
            stack: ItemStack,
            simulate: bool,
        ) -> Option<ItemStack> {
            let east = [at[0] + 1, at[1], at[2]];
            let Some(next) = host.capability(&ITEMS, east) else {
                return Some(stack);
            };
            next.insert(host, east, Some(Direction::West), 0, stack, simulate)
        }
        fn extract(
            &self,
            _: &mut dyn Host,
            _: [i32; 3],
            _: Option<Direction>,
            _: usize,
            _: u32,
            _: bool,
        ) -> Option<ItemStack> {
            None
        }
    }

    /// A pipe at (0, 64, 0), and a one-slot vanilla chest east of it, answered by the game.
    #[derive(Default)]
    struct PipeWorld {
        chest: Option<ItemStack>,
    }
    const PIPE: [i32; 3] = [0, 64, 0];
    const CHEST: [i32; 3] = [1, 64, 0];
    impl Host for PipeWorld {
        fn block_entity_type(&self, pos: [i32; 3]) -> Option<String> {
            (pos == PIPE).then(|| "test:pipe".into())
        }
        fn container_size(&self, pos: [i32; 3], _: Option<Direction>) -> Option<usize> {
            (pos == CHEST).then_some(1)
        }
        fn container_stack(
            &self,
            pos: [i32; 3],
            _: Option<Direction>,
            slot: usize,
        ) -> Option<ItemStack> {
            self.chest.clone().filter(|_| pos == CHEST && slot == 0)
        }
        fn container_insert(
            &mut self,
            pos: [i32; 3],
            _: Option<Direction>,
            slot: usize,
            stack: ItemStack,
            simulate: bool,
        ) -> Option<ItemStack> {
            if pos != CHEST || slot != 0 || self.chest.is_some() {
                return Some(stack);
            }
            if !simulate {
                self.chest = Some(stack);
            }
            None
        }
    }

    fn stick() -> ItemStack {
        ItemStack {
            item: Arc::from("minecraft:stick"),
            count: 1,
            damage: 0,
            extra: None,
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
    fn a_capability_part_answers_by_side_and_forwards_to_its_neighbours_capability() {
        game_thread();
        let mut world = PipeWorld::default();
        let host: &mut dyn Host = &mut world;
        let pipe = host.capability(&ITEMS, PIPE).expect("the pipe's part");
        assert_eq!(pipe.slots(host, PIPE, Some(Direction::Up)), 0);
        assert_eq!(pipe.slots(host, PIPE, Some(Direction::North)), 1);
        // Simulated, it would fit, and nothing moves.
        assert!(
            pipe.insert(host, PIPE, Some(Direction::North), 0, stick(), true)
                .is_none()
        );
        assert!(host.container_stack(CHEST, None, 0).is_none());
        // The pipe calls the chest's capability, the game's own, with the host it was given.
        assert!(
            pipe.insert(host, PIPE, Some(Direction::North), 0, stick(), false)
                .is_none()
        );
        assert_eq!(host.container_stack(CHEST, None, 0), Some(stick()));
        assert_eq!(
            pipe.insert(host, PIPE, Some(Direction::North), 0, stick(), false),
            Some(stick())
        );
        // No block entity with slots: no answer. A mod's own capability has no game answer.
        assert!(host.capability(&ITEMS, [5, 64, 0]).is_none());
        const CHEMICALS: CapabilityKey<dyn ItemHandler> = CapabilityKey::new("test:chemicals");
        assert!(host.capability(&CHEMICALS, CHEST).is_none());
    }

    #[test]
    fn a_capability_answered_twice_for_one_type_is_refused() {
        let mut r = Registrar::default();
        r.block_entity_capability(&ITEMS, "test:pipe", Arc::new(Pipe));
        r.mod_index = 1;
        r.block_entity_capability(&ITEMS, "test:pipe", Arc::new(Pipe));
        r.block_entity_capability(&ITEMS, "pipe", Arc::new(Pipe));
        assert_eq!(r.refused.len(), 2, "{:?}", r.refused);
        assert!(r.refused[0].contains("horizon:items is answered for test:pipe twice"));
        assert!(r.refused[1].contains("not a namespaced id"));
    }

    /// A mod's tank: the game's tank at its position, reached from every side but its top.
    struct Tank;
    impl FluidHandler for Tank {
        fn tanks(&self, host: &dyn Host, at: [i32; 3], side: Option<Direction>) -> usize {
            if side == Some(Direction::Up) {
                return 0;
            }
            host.fluid_tank_count(at, None).unwrap_or(0)
        }
        fn fluid(
            &self,
            host: &dyn Host,
            at: [i32; 3],
            _: Option<Direction>,
            tank: usize,
        ) -> Option<FluidStack> {
            host.fluid_tank(at, None, tank)
        }
        fn capacity(
            &self,
            host: &dyn Host,
            at: [i32; 3],
            _: Option<Direction>,
            tank: usize,
            fluid: &FluidStack,
        ) -> u32 {
            host.fluid_tank_capacity(at, None, tank, fluid)
        }
        fn insert(
            &self,
            host: &mut dyn Host,
            at: [i32; 3],
            _: Option<Direction>,
            tank: usize,
            fluid: FluidStack,
            simulate: bool,
        ) -> Option<FluidStack> {
            host.fluid_tank_insert(at, None, tank, fluid, simulate)
        }
        fn extract(
            &self,
            host: &mut dyn Host,
            at: [i32; 3],
            _: Option<Direction>,
            tank: usize,
            max: u32,
            simulate: bool,
        ) -> Option<FluidStack> {
            host.fluid_tank_extract(at, None, tank, max, simulate)
        }
    }

    /// A tank whose simulation says it takes everything, and which takes nothing.
    struct Liar;
    impl FluidHandler for Liar {
        fn tanks(&self, _: &dyn Host, _: [i32; 3], _: Option<Direction>) -> usize {
            1
        }
        fn fluid(
            &self,
            _: &dyn Host,
            _: [i32; 3],
            _: Option<Direction>,
            _: usize,
        ) -> Option<FluidStack> {
            None
        }
        fn capacity(
            &self,
            _: &dyn Host,
            _: [i32; 3],
            _: Option<Direction>,
            _: usize,
            _: &FluidStack,
        ) -> u32 {
            1000
        }
        fn insert(
            &self,
            _: &mut dyn Host,
            _: [i32; 3],
            _: Option<Direction>,
            _: usize,
            fluid: FluidStack,
            simulate: bool,
        ) -> Option<FluidStack> {
            (!simulate).then_some(fluid)
        }
        fn extract(
            &self,
            _: &mut dyn Host,
            _: [i32; 3],
            _: Option<Direction>,
            _: usize,
            _: u32,
            _: bool,
        ) -> Option<FluidStack> {
            None
        }
    }

    /// One tank, as the game answers it: what it holds, its capacity, and whether it fills and
    /// drains. Truthfully, their simulations answering as their real calls: a `halving` tank
    /// takes half of each offer, a `half_room` one at most half its room, a `short` one gives
    /// one less than asked, and a `quarters` one gives and takes only whole quarter buckets, as
    /// a cauldron its thirds.
    struct GameTank {
        held: Option<FluidStack>,
        capacity: u32,
        fills: bool,
        drains: bool,
        halving: bool,
        half_room: bool,
        short: bool,
        quarters: bool,
    }
    /// Game tanks by position, and the mod block entity types standing at some.
    #[derive(Default)]
    struct TankWorld {
        tanks: HashMap<[i32; 3], GameTank>,
        types: HashMap<[i32; 3], &'static str>,
    }
    const SOURCE: [i32; 3] = [0, 64, 0];
    const TANK: [i32; 3] = [1, 64, 0];
    impl TankWorld {
        fn tank(&mut self, pos: [i32; 3], held: Option<FluidStack>, fills: bool) -> &mut Self {
            let tank = GameTank {
                held,
                capacity: 1000,
                fills,
                drains: true,
                halving: false,
                half_room: false,
                short: false,
                quarters: false,
            };
            self.tanks.insert(pos, tank);
            self
        }
        fn held(&self, pos: [i32; 3]) -> Option<FluidStack> {
            self.tanks[&pos].held.clone()
        }
        fn amount(&self, pos: [i32; 3]) -> u32 {
            self.tanks[&pos].held.as_ref().map_or(0, |held| held.amount)
        }
    }
    impl Host for TankWorld {
        fn block_entity_type(&self, pos: [i32; 3]) -> Option<String> {
            self.types.get(&pos).map(|kind| kind.to_string())
        }
        fn fluid_tank_count(&self, pos: [i32; 3], _: Option<Direction>) -> Option<usize> {
            self.tanks.contains_key(&pos).then_some(1)
        }
        fn fluid_tank(&self, pos: [i32; 3], _: Option<Direction>, _: usize) -> Option<FluidStack> {
            self.tanks.get(&pos)?.held.clone()
        }
        fn fluid_tank_capacity(
            &self,
            pos: [i32; 3],
            _: Option<Direction>,
            _: usize,
            _: &FluidStack,
        ) -> u32 {
            self.tanks
                .get(&pos)
                .filter(|tank| tank.fills)
                .map_or(0, |tank| tank.capacity)
        }
        fn fluid_tank_insert(
            &mut self,
            pos: [i32; 3],
            _: Option<Direction>,
            _: usize,
            fluid: FluidStack,
            simulate: bool,
        ) -> Option<FluidStack> {
            let Some(tank) = self.tanks.get_mut(&pos).filter(|tank| tank.fills) else {
                return Some(fluid);
            };
            let held = tank.held.as_ref().map_or(0, |held| held.amount);
            if tank
                .held
                .as_ref()
                .is_some_and(|held| !held.same_fluid_same_components(&fluid))
            {
                return Some(fluid);
            }
            let offered = if tank.halving {
                fluid.amount / 2
            } else {
                fluid.amount
            };
            let room = if tank.half_room {
                (tank.capacity - held) / 2
            } else {
                tank.capacity - held
            };
            let mut add = offered.min(room);
            if tank.quarters {
                add -= add % 250;
            }
            if !simulate && add > 0 {
                tank.held = Some(fluid.with_amount(held + add));
            }
            (add < fluid.amount).then(|| fluid.with_amount(fluid.amount - add))
        }
        fn fluid_tank_extract(
            &mut self,
            pos: [i32; 3],
            _: Option<Direction>,
            _: usize,
            max: u32,
            simulate: bool,
        ) -> Option<FluidStack> {
            let tank = self.tanks.get_mut(&pos).filter(|tank| tank.drains)?;
            let held = tank.held.clone()?;
            let asked = if tank.short {
                max.saturating_sub(1)
            } else {
                max
            };
            let mut take = held.amount.min(asked);
            if tank.quarters {
                take -= take % 250;
            }
            if take == 0 {
                return None;
            }
            if !simulate {
                tank.held = (take < held.amount).then(|| held.with_amount(held.amount - take));
            }
            Some(held.with_amount(take))
        }
    }

    fn potion(amount: u32, potion: &str) -> FluidStack {
        let components = crate::item_data::StackData {
            potion: Some(Arc::from(potion)),
            ..Default::default()
        };
        FluidStack {
            components: Some(Box::new(components)),
            ..FluidStack::new("test:potion", amount)
        }
    }

    #[test]
    fn a_fluid_move_plans_both_ends_and_fills_only_with_the_same_fluid_and_components() {
        game_thread();
        let mut world = TankWorld::default();
        world.tank(SOURCE, Some(potion(1500, "minecraft:healing")), true);
        world.tank(TANK, None, true).types.insert(TANK, "test:tank");
        let host: &mut dyn Host = &mut world;
        // The mod's tank takes nothing from above.
        assert!(
            host.transfer_fluids(SOURCE, None, TANK, Some(Direction::Up), 4000)
                .is_none()
        );
        // The source is asked for no more than the tank takes.
        let moved = host.transfer_fluids(SOURCE, None, TANK, Some(Direction::West), 4000);
        assert_eq!(moved, Some(potion(1000, "minecraft:healing")));
        let moved = host.transfer_fluids(TANK, Some(Direction::West), SOURCE, None, 400);
        assert_eq!(moved, Some(potion(400, "minecraft:healing")));
        assert_eq!(world.held(SOURCE), Some(potion(900, "minecraft:healing")));
        assert_eq!(world.held(TANK), Some(potion(600, "minecraft:healing")));
        // The same fluid with other components, or another fluid, doesn't go in.
        for other in [
            potion(500, "minecraft:poison"),
            FluidStack::new("test:potion", 500),
        ] {
            world.tank(SOURCE, Some(other.clone()), true);
            let host: &mut dyn Host = &mut world;
            assert!(
                host.transfer_fluids(SOURCE, None, TANK, None, 4000)
                    .is_none()
            );
            assert_eq!(world.held(SOURCE), Some(other));
            assert_eq!(world.held(TANK), Some(potion(600, "minecraft:healing")));
        }
        // A drain-only tank has no capacity, and nothing moves.
        let water = FluidStack::new("minecraft:water", 1000);
        world.tank(SOURCE, Some(water.clone()), true);
        world.tank(TANK, None, false);
        let host: &mut dyn Host = &mut world;
        assert_eq!(host.fluid_tank_capacity(TANK, None, 0, &water), 0);
        assert!(
            host.transfer_fluids(SOURCE, None, TANK, None, 1000)
                .is_none()
        );
        assert_eq!(world.held(SOURCE), Some(water));
    }

    #[test]
    fn truthful_tanks_lose_nothing_and_are_not_warned_whatever_they_take_or_give() {
        game_thread();
        let water = |amount| FluidStack::new("minecraft:water", amount);
        type Setup = fn(&mut GameTank, &mut GameTank);
        let cases: [(&str, Setup, u32); 6] = [
            // Half of each offer: no request is ever taken whole, so nothing moves.
            ("half of each offer", |_, t| t.halving = true, 0),
            (
                "half of each offer, from quarters",
                |s, t| (s.quarters, t.halving) = (true, true),
                0,
            ),
            (
                "half of each offer, from a drain-only tank",
                |s, t| (s.fills, t.halving) = (false, true),
                0,
            ),
            // Half its room: rounds of 500, 250, then 125, which quarters can't give.
            (
                "half its room, from quarters",
                |s, t| (s.quarters, t.half_room) = (true, true),
                750,
            ),
            (
                "half its room, from a drain-only tank",
                |s, t| (s.fills, t.half_room) = (false, true),
                999,
            ),
            // One less than asked: the real extract is asked what its simulation was.
            ("a source giving one less", |s, _| s.short = true, 998),
        ];
        for (case, setup, moves) in cases {
            let mut world = TankWorld::default();
            world.tank(SOURCE, Some(water(1000)), true);
            world.tank(TANK, None, true);
            let [source, target] = world.tanks.get_disjoint_mut([&SOURCE, &TANK]);
            setup(source.unwrap(), target.unwrap());
            let host: &mut dyn Host = &mut world;
            let moved = host.transfer_fluids(SOURCE, None, TANK, None, 1000);
            assert_eq!(moved, (moves > 0).then(|| water(moves)), "{case}");
            assert_eq!(
                (world.amount(SOURCE), world.amount(TANK)),
                (1000 - moves, moves),
                "{case}"
            );
        }
        assert!(!super::super::fluid_capability::warned_about("the game"));
    }

    #[test]
    fn the_api_55_move_leaves_a_fluid_with_components_where_it_is() {
        game_thread();
        let mut world = TankWorld::default();
        world.tank(SOURCE, Some(potion(500, "minecraft:healing")), true);
        world.tank(TANK, None, true);
        let host: &mut dyn Host = &mut world;
        let plain = |fluid: &FluidStack| !fluid.has_components();
        assert!(
            host.transfer_fluids_where(SOURCE, None, TANK, None, 1000, &plain)
                .is_none()
        );
        assert_eq!(world.held(SOURCE), Some(potion(500, "minecraft:healing")));
        assert_eq!(world.held(TANK), None);
    }

    #[test]
    fn a_tank_whose_simulation_lies_gets_back_what_it_refused_or_loses_it_and_is_named() {
        game_thread();
        let water = FluidStack::new("minecraft:water", 1000);
        let mut world = TankWorld::default();
        world.tank(SOURCE, Some(water.clone()), true);
        world.types.insert(TANK, "test:liar");
        let host: &mut dyn Host = &mut world;
        // Its simulation took it all and its real call took none: it goes back into the source.
        assert!(
            host.transfer_fluids(SOURCE, None, TANK, None, 1000)
                .is_none()
        );
        assert_eq!(world.held(SOURCE), Some(water.clone()));
        assert!(super::super::fluid_capability::warned_about(
            "native mod #0"
        ));
        // A source that can't take it back: fluid has no item form to drop, so it is lost.
        world.tanks.get_mut(&SOURCE).unwrap().fills = false;
        let host: &mut dyn Host = &mut world;
        assert!(
            host.transfer_fluids(SOURCE, None, TANK, None, 1000)
                .is_none()
        );
        assert_eq!(world.held(SOURCE), None);
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
