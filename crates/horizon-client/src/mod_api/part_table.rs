//! Parts (arismoko/horizon#41): code a mod attaches to one block, block entity, item or mod
//! menu type at startup. The scoped hooks ask the parts attached to the thing they are about,
//! after every mod's `Hooks` method. Parts attached by tag are resolved by each game against its
//! own tags, at world open and on `/reload`, into a table kept by that game's thread.
use super::{
    BlockEntityPart, BlockPart, CapabilityKey, Hooks, ItemCapabilityKey, ItemPart, MenuPart,
    PlaceProvider,
};
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
    capabilities: Capabilities,
    /// Item capability handlers by capability name, then by item: one each.
    item_capabilities: Capabilities,
    /// The mods' own item places (`ItemPlace::Mod`), by provider name.
    place_providers: HashMap<&'static str, (usize, Arc<dyn PlaceProvider>)>,
}

type Capabilities = HashMap<&'static str, HashMap<Box<str>, CapabilityPart>>;

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
    capability_part(&INSTALLED.get()?.keys.capabilities, key.name, kind)
}

/// The handler for `key` a mod registered for item `item`.
pub(crate) fn item_capability<'k, T: ?Sized + 'static>(
    key: &ItemCapabilityKey<T>,
    item: impl FnOnce() -> Option<&'k str>,
) -> Option<Arc<T>> {
    capability_part(&INSTALLED.get()?.keys.item_capabilities, key.name, item)
}

/// The mod's provider of the item places named `provider`.
pub(crate) fn place_provider(provider: &str) -> Option<Arc<dyn PlaceProvider>> {
    let (_, provider) = INSTALLED.get()?.keys.place_providers.get(provider)?;
    Some(provider.clone())
}

/// The mod that registered `handler` for a capability; none for the game's own.
pub(crate) fn capability_mod<T: ?Sized + 'static>(handler: &Arc<T>) -> Option<usize> {
    let keys = &INSTALLED.get()?.keys;
    let tables = [&keys.capabilities, &keys.item_capabilities];
    let parts = tables
        .into_iter()
        .flat_map(HashMap::values)
        .flat_map(HashMap::values);
    parts
        .into_iter()
        .find(|part| {
            part.handler
                .downcast_ref::<Arc<T>>()
                .is_some_and(|h| std::ptr::addr_eq(Arc::as_ptr(h), Arc::as_ptr(handler)))
        })
        .map(|part| part.mod_index)
}

fn capability_part<'k, T: ?Sized + 'static>(
    table: &Capabilities,
    name: &str,
    id: impl FnOnce() -> Option<&'k str>,
) -> Option<Arc<T>> {
    let part = table.get(name)?.get(id()?)?;
    part.handler.downcast_ref::<Arc<T>>().cloned()
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
        self.capability(|p| &mut p.capabilities, key.name, kind, handler);
    }
    /// Answer item capability `key` for item `item` (`mymod:backpack`) with `handler`, in place
    /// of the game's own answer: `r.item_capability(&ITEM_ITEMS, "mymod:backpack",
    /// Arc::new(Backpack))`. One handler a capability and item.
    pub fn item_capability<T: ?Sized + Send + Sync + 'static>(
        &mut self,
        key: &ItemCapabilityKey<T>,
        item: &str,
        handler: Arc<T>,
    ) {
        self.capability(|p| &mut p.item_capabilities, key.name, item, handler);
    }
    /// Read and write the item places `ItemPlace::Mod { provider: name, .. }` with `provider`,
    /// as an accessory mod exposes its slots. One provider a name.
    pub fn place_provider(&mut self, name: &'static str, provider: Arc<dyn PlaceProvider>) {
        if !namespaced(name) {
            self.refused.push(format!(
                "{} provides item places {name:?}, which is not a namespaced name",
                super::mod_name(self.mod_index)
            ));
            return;
        }
        if let Some((other, _)) = self.parts.place_providers.get(name) {
            self.refused.push(format!(
                "item places {name} are provided twice, by {} and {}",
                super::mod_name(*other),
                super::mod_name(self.mod_index)
            ));
            return;
        }
        self.parts
            .place_providers
            .insert(name, (self.mod_index, provider));
    }

    /// Answer capability `name` for `id` in `table` with `handler`; refused twice or for an id
    /// with no namespace.
    fn capability<T: ?Sized + Send + Sync + 'static>(
        &mut self,
        table: fn(&mut Parts) -> &mut Capabilities,
        name: &'static str,
        id: &str,
        handler: Arc<T>,
    ) {
        if !namespaced(id) {
            self.refused.push(format!(
                "{} answered {name} for {id:?}, which is not a namespaced id",
                super::mod_name(self.mod_index)
            ));
            return;
        }
        let handlers = table(&mut self.parts).entry(name).or_default();
        if let Some(other) = handlers.get(id) {
            self.refused.push(format!(
                "{name} is answered for {id} twice, by {} and {}",
                super::mod_name(other.mod_index),
                super::mod_name(self.mod_index)
            ));
            return;
        }
        handlers.insert(
            id.into(),
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
            r.block_entity_capability(&ITEMS, "test:liar", Arc::new(Liar));
            r.block_entity_capability(&ITEMS, "test:item_overstates", Arc::new(ItemOverstates));
            r.block_entity_capability(&ITEMS, "test:item_swaps", Arc::new(ItemSwaps));
            r.item_capability(&ITEM_ITEMS, "test:pouch", Arc::new(Pouch));
            r.item_capability(&ITEM_ITEMS, "test:bag", Arc::new(Bag));
            r.place_provider("test:charms", Arc::new(Charms));
            r.item_capability(&ITEM_FLUIDS, "test:canister", Arc::new(Canister));
            r.block_entity_capability(&FLUIDS, "test:tank", Arc::new(Tank));
            r.block_entity_capability(&FLUIDS, "test:fluid_liar", Arc::new(FluidLiar));
            for (id, overstates) in [
                ("test:gives_more", Overstates::GivesMore),
                ("test:refuses_more", Overstates::RefusesMore),
                ("test:gives_lava", Overstates::GivesLava),
            ] {
                r.block_entity_capability(&FLUIDS, id, Arc::new(overstates));
            }
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
        fn capacity(
            &self,
            _: &dyn Host,
            _: [i32; 3],
            _: Option<Direction>,
            _: usize,
            _: &ItemStack,
        ) -> u32 {
            0
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

    fn stack(item: &str, count: u32, damage: u32) -> ItemStack {
        ItemStack {
            item: Arc::from(item),
            count,
            damage,
            extra: None,
        }
    }

    /// A pouch of sticks: one slot, its stick count kept in the pouch's own stack (its damage, for
    /// the test), so the handler holds nothing.
    struct Pouch;
    impl StackItemHandler for Pouch {
        fn slots(&self, _: &mut dyn Host, _: &ItemPlace) -> usize {
            1
        }
        fn stack(&self, host: &mut dyn Host, at: &ItemPlace, _: usize) -> Option<ItemStack> {
            let sticks = host.place_stack(at)?.damage;
            (sticks > 0).then(|| stack("minecraft:stick", sticks, 0))
        }
        fn insert(
            &self,
            host: &mut dyn Host,
            at: &ItemPlace,
            _: usize,
            sticks: ItemStack,
            simulate: bool,
        ) -> Option<ItemStack> {
            let Some(pouch) = host.place_stack(at) else {
                return Some(sticks);
            };
            let filled = stack("test:pouch", 1, pouch.damage + sticks.count);
            host.set_place_stack(at, filled, true, simulate)
                .map(|_| sticks)
        }
        fn extract(
            &self,
            _: &mut dyn Host,
            _: &ItemPlace,
            _: usize,
            _: u32,
            _: bool,
        ) -> Option<ItemStack> {
            None
        }
        fn capacity(&self, _: &mut dyn Host, _: &ItemPlace, _: usize, stack: &ItemStack) -> u32 {
            if &*stack.item == "minecraft:stick" {
                64
            } else {
                0
            }
        }
    }

    /// A bag holding one pouch: the pouch's sticks plus one kept in the bag's damage, 0 when
    /// empty.
    struct Bag;
    impl StackItemHandler for Bag {
        fn slots(&self, _: &mut dyn Host, _: &ItemPlace) -> usize {
            1
        }
        fn stack(&self, host: &mut dyn Host, at: &ItemPlace, _: usize) -> Option<ItemStack> {
            let held = host.place_stack(at)?.damage;
            (held > 0).then(|| stack("test:pouch", 1, held - 1))
        }
        fn insert(
            &self,
            host: &mut dyn Host,
            at: &ItemPlace,
            _: usize,
            pouch: ItemStack,
            simulate: bool,
        ) -> Option<ItemStack> {
            match host.place_stack(at) {
                Some(bag) if bag.damage == 0 && &*pouch.item == "test:pouch" => {
                    let full = stack("test:bag", 1, pouch.damage + 1);
                    host.set_place_stack(at, full, true, simulate)
                        .map(|_| pouch)
                }
                _ => Some(pouch),
            }
        }
        fn extract(
            &self,
            host: &mut dyn Host,
            at: &ItemPlace,
            slot: usize,
            _: u32,
            simulate: bool,
        ) -> Option<ItemStack> {
            let pouch = self.stack(host, at, slot)?;
            if host
                .set_place_stack(at, stack("test:bag", 1, 0), true, simulate)
                .is_some()
            {
                return None;
            }
            Some(pouch)
        }
        fn capacity(&self, _: &mut dyn Host, _: &ItemPlace, _: usize, stack: &ItemStack) -> u32 {
            u32::from(&*stack.item == "test:pouch")
        }
    }

    /// An accessory mod's charm slots: charm `key` is kept in player 1's inventory slot 20 +
    /// `key`. A charm slot has nowhere to put an item split off it, so it refuses those writes.
    struct Charms;
    impl PlaceProvider for Charms {
        fn stack(&self, host: &mut dyn Host, key: u64) -> Option<ItemStack> {
            host.game_stack(&charm_slot(key))
        }
        fn set_stack(
            &self,
            host: &mut dyn Host,
            key: u64,
            stack: ItemStack,
            one_by_one: bool,
            simulate: bool,
        ) -> bool {
            let place = charm_slot(key);
            if one_by_one && host.game_stack(&place).is_some_and(|held| held.count > 1) {
                return false;
            }
            host.set_game_stack(&place, stack, false, simulate)
        }
    }
    fn charm_slot(key: u64) -> ItemPlace {
        ItemPlace::PlayerSlot {
            player: 1,
            slot: 20 + key as i32,
        }
    }

    /// Player 1's hands and slots, and what was given to them: an item split off their hand.
    #[derive(Default)]
    struct Player {
        main_hand: Option<ItemStack>,
        slots: HashMap<i32, ItemStack>,
        given: Vec<ItemStack>,
    }
    const HAND: ItemPlace = ItemPlace::PlayerHand {
        player: 1,
        hand: Hand::Main,
    };
    impl Host for Player {
        fn item_container_slots(&self, _: &ItemStack) -> Option<Vec<Option<ItemStack>>> {
            None
        }
        // Buckets, all or nothing, of water alone.
        fn item_fluid_tanks(&self, stack: &ItemStack) -> usize {
            usize::from(stack.item.ends_with("bucket"))
        }
        fn item_fluid_held(&self, stack: &ItemStack) -> Option<FluidStack> {
            (&*stack.item == "minecraft:water_bucket")
                .then(|| FluidStack::new("minecraft:water", 1000))
        }
        fn item_fluid_capacity(&self, _: &ItemStack, _: &FluidStack) -> u32 {
            1000
        }
        fn item_fluid_filled(
            &self,
            held: &ItemStack,
            fluid: &FluidStack,
        ) -> Option<(u32, ItemStack)> {
            let takes = &*held.item == "minecraft:bucket"
                && &*fluid.fluid == "minecraft:water"
                && fluid.amount >= 1000;
            Some(match takes {
                true => (1000, stack("minecraft:water_bucket", 1, 0)),
                false => (
                    0,
                    ItemStack {
                        count: 1,
                        ..held.clone()
                    },
                ),
            })
        }
        fn item_fluid_drained(
            &self,
            held: &ItemStack,
            max: u32,
        ) -> Option<(FluidStack, ItemStack)> {
            (&*held.item == "minecraft:water_bucket" && max >= 1000).then(|| {
                let water = FluidStack::new("minecraft:water", 1000);
                (water, stack("minecraft:bucket", 1, 0))
            })
        }
        fn game_stack(&mut self, place: &ItemPlace) -> Option<ItemStack> {
            match place {
                ItemPlace::PlayerHand { player: 1, .. } => self.main_hand.clone(),
                ItemPlace::PlayerSlot { player: 1, slot } => self.slots.get(slot).cloned(),
                _ => None,
            }
        }
        fn set_game_stack(
            &mut self,
            place: &ItemPlace,
            stack: ItemStack,
            one_by_one: bool,
            simulate: bool,
        ) -> bool {
            let known = matches!(
                place,
                ItemPlace::PlayerHand { player: 1, .. } | ItemPlace::PlayerSlot { player: 1, .. }
            );
            if !known || simulate {
                return known;
            }
            if one_by_one
                && *place == HAND
                && let Some(held) = self.main_hand.as_mut().filter(|held| held.count > 1)
            {
                held.count -= 1;
                self.given.extend((stack.count > 0).then_some(stack));
                return true;
            }
            let stack = (stack.count > 0).then_some(stack);
            match place {
                ItemPlace::PlayerHand { player: 1, .. } => self.main_hand = stack,
                ItemPlace::PlayerSlot { player: 1, slot } => {
                    self.slots.remove(slot);
                    self.slots.extend(stack.map(|stack| (*slot, stack)));
                }
                _ => return false,
            }
            true
        }
    }

    /// A mod's two-tank canister, empty: it answers in place of the game's.
    struct Canister;
    impl StackFluidHandler for Canister {
        fn tanks(&self, _: &mut dyn Host, _: &ItemPlace) -> usize {
            2
        }
        fn fluid(&self, _: &mut dyn Host, _: &ItemPlace, _: usize) -> Option<FluidStack> {
            None
        }
        fn capacity(&self, _: &mut dyn Host, _: &ItemPlace, _: usize, _: &FluidStack) -> u32 {
            0
        }
        fn insert(
            &self,
            _: &mut dyn Host,
            _: &ItemPlace,
            _: usize,
            fluid: FluidStack,
            _: bool,
        ) -> Option<FluidStack> {
            Some(fluid)
        }
        fn extract(
            &self,
            _: &mut dyn Host,
            _: &ItemPlace,
            _: usize,
            _: u32,
            _: bool,
        ) -> Option<FluidStack> {
            None
        }
    }

    /// Items counted with what they hold: a pouch's sticks, a bag's pouch and its sticks.
    fn items(stacks: impl IntoIterator<Item = ItemStack>) -> u32 {
        stacks
            .into_iter()
            .map(|s| match &*s.item {
                "test:pouch" => s.count * (1 + s.damage),
                "test:bag" => s.count * (1 + s.damage),
                _ => s.count,
            })
            .sum()
    }
    impl Player {
        fn items(&self) -> u32 {
            let held = self.main_hand.iter().chain(self.slots.values());
            items(held.chain(&self.given).cloned())
        }
    }

    /// Three shelf slots at (2, 64, 0), answered by the game's handler, else by `Liar`. A slot
    /// takes up to `limit` of one item, the `output` slot none, and gives at most one item per
    /// extract, unless `whole_extract`; with `one_per_insert`, it takes at most one per insert.
    struct Shelf {
        slots: [Option<ItemStack>; 3],
        limit: u32,
        output: Option<usize>,
        one_per_insert: bool,
        whole_extract: bool,
        liar: bool,
        overstates: bool,
        swaps: bool,
        dropped: Vec<ItemStack>,
    }
    fn shelf(slots: [Option<ItemStack>; 3]) -> Shelf {
        Shelf {
            slots,
            limit: 64,
            output: None,
            one_per_insert: false,
            whole_extract: false,
            liar: false,
            overstates: false,
            swaps: false,
            dropped: Vec::new(),
        }
    }
    const SHELF: [i32; 3] = [2, 64, 0];
    fn shelf_slot(slot: usize) -> ItemPlace {
        ItemPlace::BlockEntity {
            pos: SHELF,
            side: None,
            slot,
        }
    }
    impl Host for Shelf {
        fn block_entity_type(&self, _: [i32; 3]) -> Option<String> {
            match (self.liar, self.overstates, self.swaps) {
                (true, ..) => Some("test:liar".into()),
                (_, true, _) => Some("test:item_overstates".into()),
                (.., true) => Some("test:item_swaps".into()),
                _ => None,
            }
        }
        fn container_capacity(
            &self,
            _: [i32; 3],
            _: Option<Direction>,
            slot: usize,
            _: &ItemStack,
        ) -> u32 {
            if self.output == Some(slot) {
                0
            } else {
                self.limit
            }
        }
        fn drop_at(&mut self, _: [i32; 3], stack: ItemStack) {
            self.dropped.push(stack);
        }
        fn container_size(&self, pos: [i32; 3], _: Option<Direction>) -> Option<usize> {
            (pos == SHELF).then_some(3)
        }
        fn container_stack(
            &self,
            _: [i32; 3],
            _: Option<Direction>,
            slot: usize,
        ) -> Option<ItemStack> {
            self.slots.get(slot)?.clone()
        }
        fn container_insert(
            &mut self,
            _: [i32; 3],
            _: Option<Direction>,
            slot: usize,
            stack: ItemStack,
            simulate: bool,
        ) -> Option<ItemStack> {
            let held = self.slots.get(slot)?.clone();
            let room = match &held {
                _ if self.output == Some(slot) => 0,
                None => self.limit,
                Some(held) if held.item == stack.item && held.damage == stack.damage => {
                    self.limit - held.count
                }
                Some(_) => 0,
            };
            let most = if self.one_per_insert { 1 } else { u32::MAX };
            let put = room.min(stack.count).min(most);
            if !simulate && put > 0 {
                self.slots[slot] = Some(ItemStack {
                    count: held.map_or(0, |h| h.count) + put,
                    ..stack.clone()
                });
            }
            (put < stack.count).then(|| ItemStack {
                count: stack.count - put,
                ..stack
            })
        }
        fn container_extract(
            &mut self,
            _: [i32; 3],
            _: Option<Direction>,
            slot: usize,
            max: u32,
            simulate: bool,
        ) -> Option<ItemStack> {
            let most = if self.whole_extract { max } else { 1 };
            let held = self.slots.get_mut(slot)?.as_mut()?;
            let out = ItemStack {
                count: most.min(held.count),
                ..held.clone()
            };
            if !simulate {
                held.count -= out.count;
                if held.count == 0 {
                    self.slots[slot] = None;
                }
            }
            Some(out)
        }
    }
    impl Shelf {
        fn items(&self) -> u32 {
            items(self.slots.iter().flatten().chain(&self.dropped).cloned())
        }
    }

    /// A mod's handler for the shelf whose simulation lies: everything fits and comes out whole,
    /// while its real calls are the shelf's.
    struct Liar;
    impl ItemHandler for Liar {
        fn slots(&self, host: &dyn Host, at: [i32; 3], side: Option<Direction>) -> usize {
            host.container_size(at, side).unwrap_or(0)
        }
        fn stack(
            &self,
            host: &dyn Host,
            at: [i32; 3],
            side: Option<Direction>,
            slot: usize,
        ) -> Option<ItemStack> {
            host.container_stack(at, side, slot)
        }
        fn insert(
            &self,
            host: &mut dyn Host,
            at: [i32; 3],
            side: Option<Direction>,
            slot: usize,
            stack: ItemStack,
            simulate: bool,
        ) -> Option<ItemStack> {
            if simulate {
                return None;
            }
            host.container_insert(at, side, slot, stack, false)
        }
        fn extract(
            &self,
            host: &mut dyn Host,
            at: [i32; 3],
            side: Option<Direction>,
            slot: usize,
            max: u32,
            simulate: bool,
        ) -> Option<ItemStack> {
            if simulate {
                let held = host.container_stack(at, side, slot)?;
                return Some(ItemStack {
                    count: max.min(held.count),
                    ..held
                });
            }
            host.container_extract(at, side, slot, max, false)
        }
        fn capacity(
            &self,
            _: &dyn Host,
            _: [i32; 3],
            _: Option<Direction>,
            _: usize,
            _: &ItemStack,
        ) -> u32 {
            u32::MAX
        }
    }

    /// A mod's handler for the shelf whose real calls overstate: an extract gives 5 more than
    /// asked, and an insert keeps nothing and answers 5 more than was offered.
    struct ItemOverstates;
    impl ItemHandler for ItemOverstates {
        fn slots(&self, host: &dyn Host, at: [i32; 3], side: Option<Direction>) -> usize {
            host.container_size(at, side).unwrap_or(0)
        }
        fn stack(
            &self,
            host: &dyn Host,
            at: [i32; 3],
            side: Option<Direction>,
            slot: usize,
        ) -> Option<ItemStack> {
            host.container_stack(at, side, slot)
        }
        fn insert(
            &self,
            host: &mut dyn Host,
            at: [i32; 3],
            side: Option<Direction>,
            slot: usize,
            stack: ItemStack,
            simulate: bool,
        ) -> Option<ItemStack> {
            if simulate {
                return host.container_insert(at, side, slot, stack, true);
            }
            Some(ItemStack {
                count: stack.count + 5,
                ..stack
            })
        }
        fn extract(
            &self,
            host: &mut dyn Host,
            at: [i32; 3],
            side: Option<Direction>,
            slot: usize,
            max: u32,
            simulate: bool,
        ) -> Option<ItemStack> {
            let more = if simulate { 0 } else { 5 };
            host.container_extract(at, side, slot, max + more, simulate)
        }
        fn capacity(
            &self,
            host: &dyn Host,
            at: [i32; 3],
            side: Option<Direction>,
            slot: usize,
            stack: &ItemStack,
        ) -> u32 {
            host.container_capacity(at, side, slot, stack)
        }
    }

    /// A mod's handler for the shelf whose real calls answer in dirt: an extract takes what it was
    /// asked and gives it as dirt, and an insert keeps nothing and answers the offer as dirt.
    struct ItemSwaps;
    impl ItemHandler for ItemSwaps {
        fn slots(&self, host: &dyn Host, at: [i32; 3], side: Option<Direction>) -> usize {
            host.container_size(at, side).unwrap_or(0)
        }
        fn stack(
            &self,
            host: &dyn Host,
            at: [i32; 3],
            side: Option<Direction>,
            slot: usize,
        ) -> Option<ItemStack> {
            host.container_stack(at, side, slot)
        }
        fn insert(
            &self,
            host: &mut dyn Host,
            at: [i32; 3],
            side: Option<Direction>,
            slot: usize,
            stack: ItemStack,
            simulate: bool,
        ) -> Option<ItemStack> {
            if simulate {
                return host.container_insert(at, side, slot, stack, true);
            }
            Some(self::stack("minecraft:dirt", stack.count, 0))
        }
        fn extract(
            &self,
            host: &mut dyn Host,
            at: [i32; 3],
            side: Option<Direction>,
            slot: usize,
            max: u32,
            simulate: bool,
        ) -> Option<ItemStack> {
            let out = host.container_extract(at, side, slot, max, simulate)?;
            match simulate {
                true => Some(out),
                false => Some(stack("minecraft:dirt", out.count, 0)),
            }
        }
        fn capacity(
            &self,
            host: &dyn Host,
            at: [i32; 3],
            side: Option<Direction>,
            slot: usize,
            stack: &ItemStack,
        ) -> u32 {
            host.container_capacity(at, side, slot, stack)
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
    fn an_item_handler_writes_its_stack_back_where_it_lives_splitting_one_off() {
        game_thread();
        let mut player = Player {
            main_hand: Some(stack("test:pouch", 2, 0)),
            ..Player::default()
        };
        let host: &mut dyn Host = &mut player;
        let pouch = host
            .item_capability(&ITEM_ITEMS, &HAND)
            .expect("the pouch's part");
        // Simulated, nothing changes.
        assert!(
            pouch
                .insert(host, &HAND, 0, stack("minecraft:stick", 3, 0), true)
                .is_none()
        );
        assert_eq!(host.place_stack(&HAND), Some(stack("test:pouch", 2, 0)));
        // One of the two pouches is filled: it leaves the hand for the player, the other stays.
        assert!(
            pouch
                .insert(host, &HAND, 0, stack("minecraft:stick", 3, 0), false)
                .is_none()
        );
        assert_eq!(host.place_stack(&HAND), Some(stack("test:pouch", 1, 0)));
        assert_eq!(player.given, [stack("test:pouch", 1, 3)]);
        assert_eq!(player.items(), 2 + 3);
        // An item with no part and no game slots has no answer.
        player.main_hand = Some(stack("minecraft:stone", 1, 0));
        let host: &mut dyn Host = &mut player;
        assert!(host.item_capability(&ITEM_ITEMS, &HAND).is_none());
    }

    #[test]
    fn a_stack_inside_an_item_and_a_mods_place_are_reached_through_their_handlers() {
        game_thread();
        let mut player = Player {
            main_hand: Some(stack("test:bag", 1, 1)),
            ..Player::default()
        };
        player.slots.insert(20 + 4, stack("test:pouch", 1, 2));
        let host: &mut dyn Host = &mut player;
        // The empty pouch in the bag in the hand takes 3 sticks: the pouch is rewritten in the
        // bag, and the bag in the hand.
        let in_bag = ItemPlace::Nested {
            parent: Box::new(HAND),
            slot: 0,
        };
        let pouch = host
            .item_capability(&ITEM_ITEMS, &in_bag)
            .expect("the pouch in the bag");
        assert!(
            pouch
                .insert(host, &in_bag, 0, stack("minecraft:stick", 3, 0), false)
                .is_none()
        );
        assert_eq!(host.place_stack(&in_bag), Some(stack("test:pouch", 1, 3)));
        assert_eq!(host.place_stack(&HAND), Some(stack("test:bag", 1, 4)));
        // A charm slot is a mod's place, read and written by its provider.
        let charm = ItemPlace::Mod {
            provider: "test:charms",
            key: 4,
        };
        let pouch = host
            .item_capability(&ITEM_ITEMS, &charm)
            .expect("the pouch in the charm slot");
        assert!(
            pouch
                .insert(host, &charm, 0, stack("minecraft:stick", 1, 0), false)
                .is_none()
        );
        assert_eq!(host.place_stack(&charm), Some(stack("test:pouch", 1, 3)));
        let unknown = ItemPlace::Mod {
            provider: "test:none",
            key: 4,
        };
        assert!(host.place_stack(&unknown).is_none());
        assert_eq!(
            host.set_place_stack(&unknown, stack("minecraft:stick", 1, 0), false, false),
            Some(stack("minecraft:stick", 1, 0))
        );
    }

    #[test]
    fn a_write_that_cant_go_through_changes_nothing_and_no_item_is_made_or_lost() {
        game_thread();
        let apples = || stack("minecraft:apple", 10, 0);
        let bread = |count| stack("minecraft:bread", count, 0);
        // The shelf gives one apple per extract: its simulation says the 10 can't all come out,
        // so they aren't replaced, and nothing moves.
        let mut shelf_ = shelf([Some(apples()), None, None]);
        let host: &mut dyn Host = &mut shelf_;
        assert_eq!(
            host.set_place_stack(&shelf_slot(0), bread(1), false, false),
            Some(bread(1))
        );
        assert_eq!(shelf_.slots, [Some(apples()), None, None]);
        // One by one, one apple becomes the bread, which goes into the next slot. Simulated
        // first, nothing changes.
        let host: &mut dyn Host = &mut shelf_;
        assert!(
            host.set_place_stack(&shelf_slot(0), bread(1), true, true)
                .is_none()
        );
        assert_eq!(shelf_.slots, [Some(apples()), None, None]);
        let host: &mut dyn Host = &mut shelf_;
        assert!(
            host.set_place_stack(&shelf_slot(0), bread(1), true, false)
                .is_none()
        );
        assert_eq!(
            shelf_.slots,
            [Some(stack("minecraft:apple", 9, 0)), Some(bread(1)), None]
        );
        assert_eq!(shelf_.items(), 10);

        // An output slot, as a furnace's: its iron comes out, but nothing may go in, so the
        // iron stays.
        let iron = || Some(stack("minecraft:iron_ingot", 1, 0));
        let mut shelf_ = shelf([iron(), None, None]);
        shelf_.output = Some(0);
        let host: &mut dyn Host = &mut shelf_;
        assert_eq!(
            host.set_place_stack(&shelf_slot(0), bread(1), false, false),
            Some(bread(1))
        );
        assert_eq!(shelf_.slots, [iron(), None, None]);
        // A slot of 2 takes 2 of 3 bread: refused before any goes in.
        let mut shelf_ = shelf([None, None, None]);
        shelf_.limit = 2;
        let host: &mut dyn Host = &mut shelf_;
        assert_eq!(
            host.set_place_stack(&shelf_slot(0), bread(3), false, false),
            Some(bread(3))
        );
        assert_eq!(shelf_.slots, [None, None, None]);

        // Two empty pouches on a full shelf: a filled one has nowhere to go, so neither changes
        // and the sticks come back.
        let sticks = || stack("minecraft:stick", 3, 0);
        let stone = || Some(stack("minecraft:stone", 64, 0));
        let mut shelf = shelf([Some(stack("test:pouch", 2, 0)), stone(), stone()]);
        let before = shelf.items();
        let host: &mut dyn Host = &mut shelf;
        let pouch = host
            .item_capability(&ITEM_ITEMS, &shelf_slot(0))
            .expect("the pouch's part");
        assert_eq!(
            pouch.insert(host, &shelf_slot(0), 0, sticks(), false),
            Some(sticks())
        );
        assert_eq!(shelf.items(), before);
        assert_eq!(shelf.slots[0], Some(stack("test:pouch", 2, 0)));
        // With a slot free, one pouch is filled there.
        shelf.slots[2] = None;
        let before = shelf.items();
        let host: &mut dyn Host = &mut shelf;
        assert!(
            pouch
                .insert(host, &shelf_slot(0), 0, sticks(), false)
                .is_none()
        );
        assert_eq!(
            shelf.slots,
            [
                Some(stack("test:pouch", 1, 0)),
                stone(),
                Some(stack("test:pouch", 1, 3))
            ]
        );
        assert_eq!(shelf.items(), before + 3);

        // Two pouches in a charm slot, which can't split: nothing changes, and the sticks come
        // back. So with a bag in that slot holding the pouch filled.
        let mut player = Player::default();
        player.slots.insert(20 + 1, stack("test:pouch", 2, 0));
        player.slots.insert(20 + 2, stack("test:bag", 2, 1));
        let before = player.items();
        let host: &mut dyn Host = &mut player;
        for charm in [
            ItemPlace::Mod {
                provider: "test:charms",
                key: 1,
            },
            ItemPlace::Nested {
                parent: Box::new(ItemPlace::Mod {
                    provider: "test:charms",
                    key: 2,
                }),
                slot: 0,
            },
        ] {
            let pouch = host
                .item_capability(&ITEM_ITEMS, &charm)
                .expect("the pouch");
            assert_eq!(
                pouch.insert(host, &charm, 0, sticks(), false),
                Some(sticks())
            );
        }
        assert_eq!(player.items(), before);
        assert_eq!(player.slots[&21], stack("test:pouch", 2, 0));
        assert_eq!(player.slots[&22], stack("test:bag", 2, 1));
    }

    #[test]
    fn a_truthful_handler_short_of_room_refuses_a_swap_cleanly() {
        game_thread();
        // A slot of 64 that takes one item per insert: its capacity, a hint, says 2 bread fit,
        // but once the 3 apples are out, the empty slot takes one per insert. The apples go
        // back in passes, one each, and none is dropped.
        let apples = || Some(stack("minecraft:apple", 3, 0));
        let mut shelf_ = shelf([apples(), None, None]);
        shelf_.one_per_insert = true;
        shelf_.whole_extract = true;
        let host: &mut dyn Host = &mut shelf_;
        let bread = |count| stack("minecraft:bread", count, 0);
        assert_eq!(
            host.set_place_stack(&shelf_slot(0), bread(2), false, false),
            Some(bread(2))
        );
        assert_eq!(shelf_.slots, [apples(), None, None]);
        assert!(shelf_.dropped.is_empty());
        assert!(!super::super::item_place::warned_about("the game"));
    }

    #[test]
    fn a_handler_whose_simulation_lies_loses_and_makes_nothing_and_is_named() {
        game_thread();
        let apples = || Some(stack("minecraft:apple", 10, 0));
        let stone = || Some(stack("minecraft:stone", 64, 0));
        // The liar says all 10 apples come out; one does. It goes back, and nothing is replaced.
        let mut shelf_ = shelf([apples(), stone(), stone()]);
        shelf_.liar = true;
        let host: &mut dyn Host = &mut shelf_;
        let bread = |count| stack("minecraft:bread", count, 0);
        assert_eq!(
            host.set_place_stack(&shelf_slot(0), bread(1), false, false),
            Some(bread(1))
        );
        assert_eq!(shelf_.slots, [apples(), stone(), stone()]);
        assert!(shelf_.dropped.is_empty());
        // It says 100 bread fit an empty slot of 64: the 36 left out are dropped at the shelf,
        // and the write stands.
        shelf_.slots[0] = None;
        let host: &mut dyn Host = &mut shelf_;
        assert!(
            host.set_place_stack(&shelf_slot(0), bread(100), false, false)
                .is_none()
        );
        assert_eq!(shelf_.slots[0], Some(bread(64)));
        assert_eq!(shelf_.dropped, [bread(36)]);
        assert_eq!(shelf_.items(), 64 * 3 + 36);
        // Warned once, naming the mod that registered it.
        assert!(super::super::item_place::warned_about(
            &super::super::mod_name(0)
        ));
    }

    #[test]
    fn an_item_handler_answering_more_than_it_was_asked_or_offered_moves_no_more() {
        game_thread();
        let apples = |count| stack("minecraft:apple", count, 0);
        let mut shelf_ = shelf([Some(apples(10)), None, None]);
        shelf_.whole_extract = true;
        shelf_.overstates = true;
        let host: &mut dyn Host = &mut shelf_;
        let handler = host.capability(&ITEMS, SHELF).unwrap();
        // Asked for 3, it gives 8: 3 come out, and the 5 more, which its insert won't keep, are
        // dropped at the shelf.
        let out = host.extract_checked(&handler, SHELF, None, 0, &apples(3));
        assert_eq!(out, Some(apples(3)));
        // Offered 2, it keeps none and answers 7 left: 2 come back, not 7.
        let rest = host.insert_checked(&handler, SHELF, None, 1, apples(2));
        assert_eq!(rest, Some(apples(2)));
        // Offered 2 through a set, the same: the old stack stays and nothing is made.
        assert_eq!(
            host.set_place_stack(&shelf_slot(1), apples(2), false, false),
            Some(apples(2))
        );
        assert_eq!(shelf_.slots, [Some(apples(2)), None, None]);
        assert_eq!(shelf_.dropped, [apples(5)]);
        assert_eq!(shelf_.items() + 3, 10);
        assert!(super::super::item_place::warned_about(
            &super::super::mod_name(0)
        ));
    }

    #[test]
    fn an_item_handler_answering_with_another_item_moves_nothing_and_is_named() {
        game_thread();
        let apples = |count| stack("minecraft:apple", count, 0);
        let dirt = |count| stack("minecraft:dirt", count, 0);
        let mut shelf_ = shelf([Some(apples(10)), None, None]);
        shelf_.whole_extract = true;
        shelf_.swaps = true;
        let host: &mut dyn Host = &mut shelf_;
        let handler = host.capability(&ITEMS, SHELF).unwrap();
        // Simulated, 3 apples; really, 3 dirt: none comes out, and the dirt, which its insert
        // won't keep, is dropped at the shelf.
        let simulated = handler.extract(host, SHELF, None, 0, 3, true).unwrap();
        assert_eq!(simulated, apples(3));
        assert_eq!(
            host.extract_checked(&handler, SHELF, None, 0, &simulated),
            None
        );
        // A set into an empty slot whose real insert answers 2 dirt left: the 2 apples come back,
        // and the slot stays empty.
        assert_eq!(
            host.set_place_stack(&shelf_slot(1), apples(2), false, false),
            Some(apples(2))
        );
        assert_eq!(shelf_.slots, [Some(apples(7)), None, None]);
        assert_eq!(shelf_.dropped, [dirt(3)]);
        assert!(super::super::item_place::warned_about(
            &super::super::mod_name(0)
        ));
    }

    #[test]
    fn a_change_inside_one_of_two_bags_splits_that_bag_off_whole() {
        game_thread();
        // Two bags in the hand, each holding an empty pouch.
        let mut player = Player {
            main_hand: Some(stack("test:bag", 2, 1)),
            ..Player::default()
        };
        let before = player.items();
        let host: &mut dyn Host = &mut player;
        let in_bag = ItemPlace::Nested {
            parent: Box::new(HAND),
            slot: 0,
        };
        let pouch = host
            .item_capability(&ITEM_ITEMS, &in_bag)
            .expect("the pouch in the bag");
        assert!(
            pouch
                .insert(host, &in_bag, 0, stack("minecraft:stick", 3, 0), false)
                .is_none()
        );
        // One bag leaves the hand holding the filled pouch; the other stays as it was, and no
        // pouch is loose.
        assert_eq!(player.main_hand, Some(stack("test:bag", 1, 1)));
        assert_eq!(player.given, [stack("test:bag", 1, 4)]);
        assert_eq!(player.items(), before + 3);
    }

    #[test]
    fn a_bucket_fills_one_item_off_a_stack_and_takes_a_whole_bucket_or_nothing() {
        game_thread();
        let water = |amount| FluidStack::new("minecraft:water", amount);
        let mut player = Player {
            main_hand: Some(stack("minecraft:bucket", 3, 0)),
            ..Player::default()
        };
        let host: &mut dyn Host = &mut player;
        let buckets = host
            .item_capability(&ITEM_FLUIDS, &HAND)
            .expect("the game's buckets");
        assert_eq!(buckets.tanks(host, &HAND), 1);
        // Less than a bucket goes nothing in; simulated, a bucket changes nothing.
        assert_eq!(
            buckets.insert(host, &HAND, 0, water(999), false),
            Some(water(999))
        );
        assert!(buckets.insert(host, &HAND, 0, water(1000), true).is_none());
        assert_eq!(player.main_hand, Some(stack("minecraft:bucket", 3, 0)));
        assert!(player.given.is_empty());
        // A bucket of water fills one bucket, split off the stack as `createFilledResult`.
        let host: &mut dyn Host = &mut player;
        assert!(buckets.insert(host, &HAND, 0, water(1500), false) == Some(water(500)));
        assert_eq!(player.main_hand, Some(stack("minecraft:bucket", 2, 0)));
        assert_eq!(player.given, [stack("minecraft:water_bucket", 1, 0)]);
        // A water bucket alone in the hand empties into the hand's bucket.
        player.main_hand = Some(stack("minecraft:water_bucket", 1, 0));
        let host: &mut dyn Host = &mut player;
        assert_eq!(buckets.fluid(host, &HAND, 0), Some(water(1000)));
        assert_eq!(buckets.extract(host, &HAND, 0, 999, false), None);
        assert_eq!(
            buckets.extract(host, &HAND, 0, 1000, false),
            Some(water(1000))
        );
        assert_eq!(player.main_hand, Some(stack("minecraft:bucket", 1, 0)));
        // A mod's canister answers in place of the game; stone has no answer.
        for (item, tanks) in [("test:canister", Some(2)), ("minecraft:stone", None)] {
            player.main_hand = Some(stack(item, 1, 0));
            let host: &mut dyn Host = &mut player;
            let handler = host.item_capability(&ITEM_FLUIDS, &HAND);
            assert_eq!(handler.map(|h| h.tanks(host, &HAND)), tanks, "{item}");
        }
    }

    #[test]
    fn an_item_capability_or_place_provider_given_twice_is_refused() {
        let mut r = Registrar::default();
        r.item_capability(&ITEM_ITEMS, "test:pouch", Arc::new(Pouch));
        r.place_provider("test:charms", Arc::new(Charms));
        r.mod_index = 1;
        r.item_capability(&ITEM_ITEMS, "test:pouch", Arc::new(Bag));
        r.place_provider("test:charms", Arc::new(Charms));
        r.place_provider("charms", Arc::new(Charms));
        assert_eq!(r.refused.len(), 3, "{:?}", r.refused);
        assert!(r.refused[0].contains("horizon:item_items is answered for test:pouch twice"));
        assert!(r.refused[1].contains("item places test:charms are provided twice"));
        assert!(r.refused[2].contains("not a namespaced name"));
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
    struct FluidLiar;
    impl FluidHandler for FluidLiar {
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

    /// The game's tank under it, but its real calls misanswer: an extract gives 100 mB more than
    /// asked (`GivesMore`) or its amount as lava (`GivesLava`), or an insert keeps nothing and
    /// answers 100 mB more than offered (`RefusesMore`).
    #[derive(PartialEq)]
    enum Overstates {
        GivesMore,
        RefusesMore,
        GivesLava,
    }
    impl FluidHandler for Overstates {
        fn tanks(&self, host: &dyn Host, at: [i32; 3], side: Option<Direction>) -> usize {
            host.fluid_tank_count(at, side).unwrap_or(0)
        }
        fn fluid(
            &self,
            host: &dyn Host,
            at: [i32; 3],
            side: Option<Direction>,
            tank: usize,
        ) -> Option<FluidStack> {
            host.fluid_tank(at, side, tank)
        }
        fn capacity(
            &self,
            host: &dyn Host,
            at: [i32; 3],
            side: Option<Direction>,
            tank: usize,
            fluid: &FluidStack,
        ) -> u32 {
            host.fluid_tank_capacity(at, side, tank, fluid)
        }
        fn insert(
            &self,
            host: &mut dyn Host,
            at: [i32; 3],
            side: Option<Direction>,
            tank: usize,
            fluid: FluidStack,
            simulate: bool,
        ) -> Option<FluidStack> {
            if *self == Self::RefusesMore && !simulate {
                return Some(fluid.with_amount(fluid.amount + 100));
            }
            host.fluid_tank_insert(at, side, tank, fluid, simulate)
        }
        fn extract(
            &self,
            host: &mut dyn Host,
            at: [i32; 3],
            side: Option<Direction>,
            tank: usize,
            max: u32,
            simulate: bool,
        ) -> Option<FluidStack> {
            if simulate {
                return host.fluid_tank_extract(at, side, tank, max, true);
            }
            let more = if *self == Self::GivesMore { 100 } else { 0 };
            let out = host.fluid_tank_extract(at, side, tank, max + more, false)?;
            match self {
                Self::GivesLava => Some(FluidStack::new("minecraft:lava", out.amount)),
                _ => Some(out),
            }
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
        world.types.insert(TANK, "test:fluid_liar");
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
    fn a_handler_answering_more_than_it_was_asked_or_offered_moves_no_more_than_agreed() {
        game_thread();
        let water = |amount| FluidStack::new("minecraft:water", amount);
        // A source giving 100 more than asked: the target gets only what both ends agreed on, the
        // move never passes its max, and the excess goes back into the source.
        let mut world = TankWorld::default();
        world.tank(SOURCE, Some(water(1000)), true);
        world.tank(TANK, None, true);
        world.types.insert(SOURCE, "test:gives_more");
        let host: &mut dyn Host = &mut world;
        assert_eq!(
            host.transfer_fluids(SOURCE, None, TANK, None, 300),
            Some(water(300))
        );
        assert_eq!((world.amount(SOURCE), world.amount(TANK)), (700, 300));
        // With no room for the excess in the source, the target's room takes it.
        world.tanks.get_mut(&SOURCE).unwrap().fills = false;
        let host: &mut dyn Host = &mut world;
        assert_eq!(
            host.transfer_fluids(SOURCE, None, TANK, None, 300),
            Some(water(300))
        );
        assert_eq!((world.amount(SOURCE), world.amount(TANK)), (300, 700));
        // The API 54 drain's excess goes back into the tank it came from.
        world.tanks.get_mut(&SOURCE).unwrap().fills = true;
        let host: &mut dyn Host = &mut world;
        let handler = host.capability(&FLUIDS, SOURCE).unwrap();
        assert_eq!(
            host.drain_one_named_fluid(&handler, SOURCE, None, 100),
            Some(water(100))
        );
        assert_eq!(world.amount(SOURCE), 200);
        // One giving lava from a water tank: the lava is given back, and nothing comes out.
        let mut world = TankWorld::default();
        world.tank(SOURCE, Some(water(1000)), true);
        world.types.insert(SOURCE, "test:gives_lava");
        let host: &mut dyn Host = &mut world;
        let handler = host.capability(&FLUIDS, SOURCE).unwrap();
        assert_eq!(
            host.drain_one_named_fluid(&handler, SOURCE, None, 300),
            None
        );
        assert_eq!(world.amount(SOURCE), 700);
        // A target that keeps nothing and answers more than it was offered: it all goes back.
        let mut world = TankWorld::default();
        world.tank(SOURCE, Some(water(1000)), true);
        world.tank(TANK, None, true);
        world.types.insert(TANK, "test:refuses_more");
        let host: &mut dyn Host = &mut world;
        assert_eq!(host.transfer_fluids(SOURCE, None, TANK, None, 300), None);
        assert_eq!((world.amount(SOURCE), world.amount(TANK)), (1000, 0));
        assert!(super::super::fluid_capability::warned_about(
            "native mod #0"
        ));
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
