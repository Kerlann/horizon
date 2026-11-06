//! Parts (arismoko/horizon#41): code a mod attaches to one block, block entity, item or mod
//! menu type at startup. The scoped hooks ask the parts attached to the thing they are about,
//! after every mod's `Hooks` method. Parts attached by tag are resolved by each game against its
//! own tags, at world open and on `/reload`, into a table kept by that game's thread; that table
//! also indexes the block parts by state, so the state-keyed hooks look theirs up without the
//! block's name.
use super::ticket_controller::{self, ControllerInfo};
use super::{
    BlockEntityPart, BlockPart, BlockSide, BlockStateId, Capability, CapabilityKey,
    EntityCapabilityKey, EntityRef, HOOK_COUNT, HookId, Host, ItemCapabilityKey, ItemPart,
    ItemPlace, MenuPart, ModEntry, PlaceProvider, ServerHost, TicketController,
};
use horizon_mods::content::BlockEntityType;
use std::{
    any::{Any, type_name},
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    marker::PhantomData,
    num::NonZeroU32,
    rc::Rc,
    sync::{
        Arc, LazyLock, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};

/// A part, with the mod that attached it.
// exhaustive: its fields are private, so mods cannot build or destructure it
pub struct Attached<P: ?Sized> {
    /// The mod's bundle id, retained for diagnostics after registration.
    pub(super) mod_id: &'static str,
    /// The mod's place in dispatch order.
    pub(super) mod_index: usize,
    /// Its place in its mod's registrations: a mod's parts on one thing keep this order, key or
    /// tag (#41 §3).
    order: usize,
    /// The part's kind, its Rust type's name: one part of each kind per thing.
    pub(super) type_name: &'static str,
    pub(super) part: Arc<P>,
}

impl<P: ?Sized> Attached<P> {
    fn with_part<Q: ?Sized>(&self, part: Arc<Q>) -> Attached<Q> {
        Attached {
            mod_id: self.mod_id,
            mod_index: self.mod_index,
            order: self.order,
            type_name: self.type_name,
            part,
        }
    }
}

impl<P: ?Sized> Clone for Attached<P> {
    fn clone(&self) -> Self {
        self.with_part(self.part.clone())
    }
}

fn insert_part<P: ?Sized>(
    table: &mut HashMap<Box<str>, Vec<Attached<P>>>,
    id: &str,
    part: Attached<P>,
    refused: &mut Vec<String>,
) -> bool {
    let list = table.entry(id.into()).or_default();
    if let Some(other) = list.iter().find(|other| other.type_name == part.type_name) {
        refused.push(format!(
            "{} is attached to {id} twice, by {} and {}",
            part.type_name, other.mod_id, part.mod_id
        ));
        return false;
    }
    list.push(part);
    true
}

/// Every mod's parts, by the id of the thing they are attached to, in dispatch order.
#[derive(Clone, Default)]
// exhaustive: its fields are private, so mods cannot build or destructure it
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
    /// Capability providers by capability name, then by block entity type, in registration
    /// order.
    capabilities: Capabilities<BlockProviderOf>,
    /// Item capability providers by capability name, then by item, in registration order.
    item_capabilities: Capabilities<ItemProviderOf>,
    /// Entity capability providers by capability name, then by entity type, in registration
    /// order.
    entity_capabilities: Capabilities<EntityProviderOf>,
    /// The mods' own item places (`ItemPlace::Mod`), by provider name.
    place_providers: HashMap<&'static str, PlaceProviderPart>,
    /// Only the parts that override each scoped hook, resolved at load or reload.
    hooks: Vec<Option<HookParts>>,
}

#[derive(Clone, Default)]
/// The Parts attached to blocks for one hook, with which hooks have a subscriber.
// exhaustive: its fields are private, so mods cannot build or destructure it
pub struct HookParts {
    blocks: HashMap<Box<str>, Vec<Attached<dyn BlockPart>>>,
    block_states: Vec<Option<NonZeroU32>>,
    block_slices: Vec<Arc<[Attached<dyn BlockPart>]>>,
    block_entities: HashMap<Box<str>, Vec<Attached<dyn BlockEntityPart>>>,
    block_entity_kinds: HashMap<Box<str>, Vec<Attached<dyn BlockEntityPart>>>,
    items: HashMap<Box<str>, Vec<Attached<dyn ItemPart>>>,
    menus: HashMap<Box<str>, Vec<Attached<dyn MenuPart>>>,
}

type Capabilities<P> = HashMap<&'static str, HashMap<Box<str>, Vec<CapabilityPart<P>>>>;

enum ProviderTarget<'a> {
    BlockEntity(&'a super::BlockEntityTypeKey),
    Id(&'a str),
}

/// A capability provider a mod registered: a [`BlockProviderOf`], [`ItemProviderOf`] or
/// [`EntityProviderOf`].
#[derive(Clone)]
pub(crate) struct CapabilityPart<P> {
    mod_id: &'static str,
    mod_index: usize,
    /// The handler type it gives, for refusing a capability name answered with two.
    type_name: &'static str,
    /// Asked ahead of the game's own provider.
    before_game: bool,
    provider: P,
}

#[derive(Clone)]
struct PlaceProviderPart {
    mod_id: &'static str,
    mod_index: usize,
    provider: Arc<dyn PlaceProvider>,
}

/// What answers a capability for a block entity of one type: given the block entity and the
/// side asked about, its handler, or none to leave it to the next provider. A closure
/// `|host, at| Some(Box::new(Tank(at)))` is one.
pub trait BlockCapabilityProvider<T: ?Sized>: Send + Sync {
    /// The handler for the block entity at `at`, or none.
    fn provide(&self, host: &mut dyn ServerHost, at: BlockSide) -> Option<Box<T>>;
}
impl<T: ?Sized, F> BlockCapabilityProvider<T> for F
where
    F: Fn(&mut dyn ServerHost, BlockSide) -> Option<Box<T>> + Send + Sync,
{
    fn provide(&self, host: &mut dyn ServerHost, at: BlockSide) -> Option<Box<T>> {
        self(host, at)
    }
}
/// What answers an item capability for one item: given where the stack lives, its handler, or
/// none to leave it to the next provider. A closure is one.
pub trait ItemCapabilityProvider<T: ?Sized>: Send + Sync {
    /// The handler for the stack at `at`, or none.
    fn provide(&self, host: &mut dyn ServerHost, at: &ItemPlace) -> Option<Box<T>>;
}
impl<T: ?Sized, F> ItemCapabilityProvider<T> for F
where
    F: Fn(&mut dyn ServerHost, &ItemPlace) -> Option<Box<T>> + Send + Sync,
{
    fn provide(&self, host: &mut dyn ServerHost, at: &ItemPlace) -> Option<Box<T>> {
        self(host, at)
    }
}
/// What answers an entity capability for one entity type: given the entity and the lookup's
/// context (its [`EntityCapabilityKey`]'s `C`), its handler, or none to leave it to the next
/// provider. A closure is one.
pub trait EntityCapabilityProvider<T: ?Sized, C>: Send + Sync {
    /// The handler for `entity`, asked with `context`, or none.
    fn provide(&self, host: &mut dyn ServerHost, entity: EntityRef, context: C) -> Option<Box<T>>;
}
impl<T: ?Sized, C, F> EntityCapabilityProvider<T, C> for F
where
    F: Fn(&mut dyn ServerHost, EntityRef, C) -> Option<Box<T>> + Send + Sync,
{
    fn provide(&self, host: &mut dyn ServerHost, entity: EntityRef, context: C) -> Option<Box<T>> {
        self(host, entity, context)
    }
}
/// A block capability provider, as the registrar keeps it.
pub type BlockProvider<T> = dyn BlockCapabilityProvider<T>;
/// An item capability provider, as the registrar keeps it.
pub type ItemProvider<T> = dyn ItemCapabilityProvider<T>;
/// An entity capability provider, as the registrar keeps it.
pub type EntityProvider<T, C = ()> = dyn EntityCapabilityProvider<T, C>;

/// A capability provider whose handler type is a mod's own (a key a library's `[api]` crate
/// publishes), erased: only mods linked with that crate know its handler type.
// hidden: the registrar's erased provider, not part of the mod surface
// exhaustive: its fields are private, so mods cannot build or destructure it
#[doc(hidden)]
#[derive(Clone)]
pub struct OwnProvider {
    /// The `Arc` of the provider trait object, which a lookup of that handler type downcasts:
    /// a wasm mod's, in the game or another group, is the game's proxy, which no lookup takes.
    pub(crate) provider: Arc<dyn Any + Send + Sync>,
    pub(crate) handler: &'static str,
}

/// How a lookup reads the capability providers mods registered, in the order they are asked
/// (those registered `before_game`, then the others, each in registration order): the game's
/// table, which a wasm mod reads through the game.
// hidden: a lookup's view of the registrar's table, not part of the mod surface
#[doc(hidden)]
pub trait CapabilityProviders {
    /// Whether any mod registered capability `name` for the things `by` names. This presence
    /// check includes disabled mods; enumeration skips their providers.
    fn provided(&self, by: ProvidedBy, name: &str) -> bool;
    /// The next enabled provider of block entity capability `name` for block entity type `kind`,
    /// with its mod and whether it is asked ahead of the game. `index` starts at zero and advances
    /// past every visited registration, including disabled ones.
    fn block_entity_provider(
        &self,
        name: &str,
        kind: super::BlockEntityTypeKey,
        index: &mut u32,
    ) -> Option<(BlockProviderOf, &'static str, bool)>;
    /// The next enabled provider of item capability `name` for item `item`, advancing `index`.
    fn item_provider(
        &self,
        name: &str,
        item: &str,
        index: &mut u32,
    ) -> Option<(ItemProviderOf, &'static str, bool)>;
    /// The next enabled provider of entity capability `name` for entity type `kind`, advancing `index`.
    fn entity_provider(
        &self,
        name: &str,
        kind: &str,
        index: &mut u32,
    ) -> Option<(EntityProviderOf, &'static str, bool)>;
    /// The enabled provider of the item places named `name`, with its mod.
    fn place_provider(&self, name: &str) -> Option<(Arc<dyn PlaceProvider>, &'static str)>;
}

/// The registrar's table, as the game reads it.
pub(crate) struct InstalledProviders;

impl CapabilityProviders for InstalledProviders {
    fn provided(&self, by: ProvidedBy, name: &str) -> bool {
        installed().is_some_and(|installed| match by {
            ProvidedBy::BlockEntity => installed.keys.capabilities.contains_key(name),
            ProvidedBy::Item => installed.keys.item_capabilities.contains_key(name),
            ProvidedBy::Entity => installed.keys.entity_capabilities.contains_key(name),
        })
    }
    fn block_entity_provider(
        &self,
        name: &str,
        kind: super::BlockEntityTypeKey,
        index: &mut u32,
    ) -> Option<(BlockProviderOf, &'static str, bool)> {
        let kind = crate::registry_keys::block_entity_type_name(kind)?;
        next(&installed()?.keys.capabilities, name, kind, index)
    }
    fn item_provider(
        &self,
        name: &str,
        item: &str,
        index: &mut u32,
    ) -> Option<(ItemProviderOf, &'static str, bool)> {
        next(&installed()?.keys.item_capabilities, name, item, index)
    }
    fn entity_provider(
        &self,
        name: &str,
        kind: &str,
        index: &mut u32,
    ) -> Option<(EntityProviderOf, &'static str, bool)> {
        next(&installed()?.keys.entity_capabilities, name, kind, index)
    }
    fn place_provider(&self, name: &str) -> Option<(Arc<dyn PlaceProvider>, &'static str)> {
        let part = installed()?.keys.place_providers.get(name)?;
        super::mod_index_on(part.mod_index).then(|| (part.provider.clone(), part.mod_id))
    }
}

fn next<P: Clone>(
    table: &Capabilities<P>,
    name: &str,
    id: &str,
    index: &mut u32,
) -> Option<(P, &'static str, bool)> {
    next_provider(table.get(name)?.get(id)?, index)
}

/// A provider by its handler type: one case for each handler type `mod_api`'s keys name
/// (generated from them), and [`OwnProvider`] for a mod's own.
#[allow(missing_docs)]
mod providers_of {
    use super::*;
    // The keys' handler types and contexts, as the generated cases name them.
    #[allow(unused_imports)]
    use crate::*;
    include!(concat!(
        env!("OUT_DIR"),
        "/capability_providers_generated.rs"
    ));
}
pub use providers_of::{BlockProviderOf, EntityProviderOf, ItemProviderOf};

/// Which things a capability's providers are registered by.
// hidden: a lookup's view of the registrar's table, not part of the mod surface
// exhaustive: the three kinds of thing a capability is asked of
#[doc(hidden)]
#[derive(Clone, Copy, Debug)]
pub enum ProvidedBy {
    BlockEntity,
    Item,
    Entity,
}

/// The parts attached by tag, by tag name, in registration order.
#[derive(Default)]
struct TagParts {
    blocks: Vec<(Box<str>, Attached<dyn BlockPart>)>,
    items: Vec<(Box<str>, Attached<dyn ItemPart>)>,
}

/// What every mod's [`Hooks::register`](super::Hooks::register) attached.
#[derive(Default)]
struct Installed {
    keys: Parts,
    tags: TagParts,
    block_entity_types: Vec<BlockEntityType>,
    registry_keys: super::registry_keys::Requests,
    effects: OnceLock<super::registry_keys::Effects>,
    resolved_block_entity_types: OnceLock<super::registry_keys::Names>,
    fluids: OnceLock<super::registry_keys::Names>,
    /// The ticket controllers the mods registered; a [`TicketController`]'s slot indexes it.
    controllers: Vec<ControllerInfo>,
    structure_types: Vec<super::structure_type::StructureTypeInfo>,
    hook_bits: Vec<bool>,
}

/// The block entity types mods registered.
pub fn block_entity_types() -> &'static [BlockEntityType] {
    installed().map_or(&[], |installed| &installed.block_entity_types)
}

pub(crate) fn requested_keys() -> &'static super::registry_keys::Requests {
    static EMPTY: LazyLock<super::registry_keys::Requests> = LazyLock::new(Default::default);
    installed().map_or(&EMPTY, |installed| &installed.registry_keys)
}

thread_local! {
    // A reference to the registrar's canonical requests while Hooks::register is in flight.
    // The table is moved, not copied, into Installed when registration finishes.
    static REGISTERING_KEYS: RefCell<Option<Rc<RefCell<super::registry_keys::Requests>>>> = const { RefCell::new(None) };
}

pub(crate) fn with_registering_keys<T>(
    read: impl FnOnce(Option<&super::registry_keys::Requests>) -> T,
) -> T {
    REGISTERING_KEYS.with(|keys| match keys.borrow().as_ref() {
        Some(keys) => read(Some(&keys.borrow())),
        None => read(None),
    })
}

pub(crate) fn resolved_effects() -> &'static OnceLock<super::registry_keys::Effects> {
    static EMPTY: OnceLock<super::registry_keys::Effects> = OnceLock::new();
    installed().map_or(&EMPTY, |installed| &installed.effects)
}

pub(crate) fn resolved_fluids() -> &'static OnceLock<super::registry_keys::Names> {
    static EMPTY: OnceLock<super::registry_keys::Names> = OnceLock::new();
    installed().map_or(&EMPTY, |installed| &installed.fluids)
}

pub(crate) fn resolved_block_entity_types() -> &'static OnceLock<super::registry_keys::Names> {
    static EMPTY: OnceLock<super::registry_keys::Names> = OnceLock::new();
    installed().map_or(&EMPTY, |installed| &installed.resolved_block_entity_types)
}

/// The ticket controllers registered at startup.
pub(super) fn installed_controllers() -> &'static [ControllerInfo] {
    INSTALLED
        .get()
        .map_or(&[], |installed| installed.controllers.as_slice())
}

/// The structure types registered at startup, for the executable to hand to
/// the engine's `StructureRegistry`.
pub(super) fn installed_structure_type_list() -> &'static [super::structure_type::StructureTypeInfo]
{
    INSTALLED
        .get()
        .map_or(&[], |installed| installed.structure_types.as_slice())
}

static INSTALLED: OnceLock<Installed> = OnceLock::new();
#[cfg(any(test, feature = "testing"))]
thread_local! {
    /// Tests install their own parts per thread, as they do their mods (`TEST_MODS`).
    static TEST_PARTS: Cell<Option<&'static Installed>> = const { Cell::new(None) };
    static TEST_PART_LOOKUPS: Cell<usize> = const { Cell::new(0) };
}
static NO_PARTS: LazyLock<Parts> = LazyLock::new(Parts::default);
static OFF_THREAD_WARNED: AtomicBool = AtomicBool::new(false);

thread_local! {
    /// This thread's game's parts, its tags resolved; none before its world opens.
    static GAME_PARTS: RefCell<Option<Rc<Parts>>> = const { RefCell::new(None) };
    /// Whether a `Game` runs on this thread.
    static GAME_THREAD: Cell<bool> = const { Cell::new(false) };
}

fn installed() -> Option<&'static Installed> {
    #[cfg(any(test, feature = "testing"))]
    if let Some(installed) = TEST_PARTS.get() {
        return Some(installed);
    }
    INSTALLED.get()
}

/// Whether any installed part subscribes to a scoped hook.
pub fn has_part_hook(hook: HookId) -> bool {
    installed().is_some_and(|installed| installed.hook_bits[hook as usize])
}

/// Whether a part listens to the block hook `hook` (named `name`) for `state`: one table
/// lookup, and only once some part listens at all.
pub fn block_listens(hook: HookId, name: &'static str, state: BlockStateId) -> bool {
    has_scoped_hook(hook)
        && parts(name)
            .for_hook(hook)
            .is_some_and(|table| !table.block_state(state).is_empty())
}

/// Preserve the no-mod fast path for level hooks.
pub fn has_scoped_hook(hook: HookId) -> bool {
    !super::mods().is_empty() && has_part_hook(hook)
}

/// The parts table a dispatch reads: the process-wide one, or this game thread's.
#[non_exhaustive]
pub enum PartsRef {
    /// The process-wide table.
    Shared(&'static Parts),
    /// This game thread's table.
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

/// The parts hook `hook` asks: this game thread's resolved table, then the shared key parts
/// when there are no tag parts, then the off-thread path.
pub fn parts(hook: &'static str) -> PartsRef {
    #[cfg(any(test, feature = "testing"))]
    TEST_PART_LOOKUPS.set(TEST_PART_LOOKUPS.get() + 1);
    let Some(installed) = installed() else {
        return PartsRef::Shared(&NO_PARTS);
    };
    if let Some(parts) = GAME_PARTS.with_borrow(Option::clone) {
        return PartsRef::Game(parts);
    }
    if installed.tags.blocks.is_empty() && installed.tags.items.is_empty() {
        return PartsRef::Shared(&installed.keys);
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
pub fn mark_game_thread() {
    GAME_THREAD.set(true);
}

/// The ids a game knows, which a part's key must name.
// exhaustive: the game builds it with a literal; mods only read it
pub struct KnownIds<'a> {
    /// Whether a block id is known.
    pub block: &'a dyn Fn(&str) -> bool,
    /// Whether an item id is known.
    pub item: &'a dyn Fn(&str) -> bool,
    /// How many block states the game has.
    pub state_count: usize,
    /// The block id of a block state, by state index.
    pub state_block: &'a dyn Fn(usize) -> &'a str,
}

/// Resolve this thread's game's parts against its tags (`block_tags`, `item_tags`: flattened,
/// tag name without `#` to members). Each run starts again from the key parts, so `/reload` drops
/// what left a tag. A key naming a block or item the game doesn't know is an error, naming the
/// mod; a missing tag or a part a tag gives twice is a warning, and the earlier mod's part stays.
pub fn resolve_parts(
    block_tags: &HashMap<String, HashSet<String>>,
    item_tags: &HashMap<String, HashSet<String>>,
    known: KnownIds<'_>,
) -> Result<(), Vec<String>> {
    let Some(installed) = installed() else {
        return Ok(());
    };
    let mut unknown: Vec<String> = unknown_keys(&installed.keys.blocks, known.block, "block")
        .chain(unknown_keys(&installed.keys.items, known.item, "item"))
        .collect();
    if !unknown.is_empty() {
        unknown.sort();
        return Err(unknown);
    }
    let mut parts = installed.keys.clone();
    add_tagged(
        &mut parts.blocks,
        &installed.tags.blocks,
        block_tags,
        "block",
    );
    add_tagged(&mut parts.items, &installed.tags.items, item_tags, "item");
    parts.index_hooks();
    for table in parts.hooks.iter_mut().flatten() {
        if table.blocks.is_empty() {
            continue;
        }
        let mut names = HashMap::with_capacity(table.blocks.len());
        for (name, attached) in &table.blocks {
            names.insert(
                name.as_ref(),
                NonZeroU32::new(table.block_slices.len() as u32 + 1).unwrap(),
            );
            table.block_slices.push(Arc::from(attached.clone()));
        }
        table.block_states = (0..known.state_count)
            .map(|state| names.get((known.state_block)(state)).copied())
            .collect();
    }
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
                    part.mod_id, part.type_name
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
                part.mod_id,
                part.type_name
            );
            continue;
        };
        for member in members {
            let list = table.entry(member.as_str().into()).or_default();
            match list.iter_mut().find(|a| a.type_name == part.type_name) {
                Some(other) => {
                    log::warn!(
                        "{} is attached to {member} twice, by {} and {} (#{tag}); {}'s is kept",
                        part.type_name,
                        other.mod_id,
                        part.mod_id,
                        if other.mod_index < part.mod_index {
                            other.mod_id
                        } else {
                            part.mod_id
                        },
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

impl HookParts {
    /// The block Parts attached to a block state, in dispatch order.
    pub fn block_state(&self, state: BlockStateId) -> &[Attached<dyn BlockPart>] {
        self.block_states
            .get(state.0 as usize)
            .and_then(|index| *index)
            .map_or(
                &[],
                |index| &self.block_slices[index.get() as usize - 1][..],
            )
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

impl Parts {
    pub(crate) fn for_hook(&self, hook: HookId) -> Option<&HookParts> {
        self.hooks.get(hook as usize).and_then(Option::as_ref)
    }
    fn index_hooks(&mut self) {
        self.hooks = vec![None; HOOK_COUNT];
        macro_rules! index {
            ($field:ident, $method:ident) => {
                for (key, parts) in &self.$field {
                    for part in parts {
                        for hook in part.part.$method().iter().flatten() {
                            let table =
                                self.hooks[*hook as usize].get_or_insert_with(HookParts::default);
                            table
                                .$field
                                .entry(key.clone())
                                .or_default()
                                .push(part.clone());
                        }
                    }
                }
            };
        }
        index!(blocks, block_subscribed);
        index!(block_entities, block_entity_subscribed);
        index!(block_entity_kinds, block_entity_subscribed);
        index!(items, item_subscribed);
        index!(menus, menu_subscribed);
    }
    #[cfg(test)]
    pub(crate) fn block_entity<'k>(
        &self,
        id: impl FnOnce() -> Option<&'k str>,
    ) -> &[Attached<dyn BlockEntityPart>] {
        attached(&self.block_entities, id)
    }
}

/// Who answers a capability: a mod's provider, the game's own, or nobody.
pub(crate) enum Answer<T: ?Sized> {
    Mod(Capability<T>),
    Game,
    Nobody,
}

/// The handler `answer` names: a mod's, or `game`, the game's own when it answered.
pub(crate) fn answered<T: ?Sized>(
    answer: Answer<T>,
    game: Option<Box<T>>,
) -> Option<Capability<T>> {
    match answer {
        Answer::Mod(found) => Some(found),
        Answer::Game => game.map(|handler| Capability {
            handler,
            owner: None,
        }),
        Answer::Nobody => None,
    }
}

/// Whether any mod registered a provider of entity capability `key`, for any entity type: one
/// table lookup, no allocation, so the game's own lookups skip all their work where none did.
pub fn has_entity_providers<T: ?Sized + 'static, C: 'static>(
    key: &EntityCapabilityKey<T, C>,
) -> bool {
    installed()
        .and_then(|installed| installed.keys.entity_capabilities.get(key.name))
        .is_some_and(|by_kind| !by_kind.is_empty())
}

/// Asks the providers `next` gives (advancing a registration cursor in dispatch order: those
/// registered `before_game`, then the others), and the game's own (`game`: whether it answers) between the
/// two, in the fork's chosen order (NeoForge's javadoc doesn't state one between mods'
/// providers and the game's); the first to answer wins. `get` takes a provider of this
/// capability's handler type out of its case: a provider of a mod's own type the asker isn't
/// linked with is passed over.
pub(crate) fn ask_providers<O, P: ?Sized + 'static, T: ?Sized>(
    host: &mut dyn ServerHost,
    next: impl Fn(&mut u32) -> Option<(O, &'static str, bool)>,
    get: impl Fn(&O) -> Option<&Arc<P>>,
    call: impl Fn(&P, &mut dyn ServerHost) -> Option<Box<T>>,
    game: impl FnOnce(&mut dyn ServerHost) -> bool,
) -> Answer<T> {
    let mut game = Some(game);
    let mut index = 0;
    while let Some((provider, owner, before_game)) = next(&mut index) {
        if !before_game
            && let Some(game) = game.take()
            && game(host)
        {
            return Answer::Game;
        }
        if let Some(handler) = get(&provider).and_then(|provider| call(provider, host)) {
            return Answer::Mod(Capability {
                handler,
                owner: Some(owner),
            });
        }
    }
    match game.is_some_and(|game| game(host)) {
        true => Answer::Game,
        false => Answer::Nobody,
    }
}

/// The providers mods registered for capability `key` and the type of the block entity at
/// `at`, for the game's own lookups (a hopper's): none where no mod registered one, so a
/// vanilla container's lookup costs one table check and allocates nothing, and the game needn't
/// work out its own answer. [`BlockEntityProviders::ask`] then asks them.
pub fn block_entity_providers<T: ?Sized + 'static>(
    key: &CapabilityKey<T>,
    host: &dyn Host,
    at: BlockSide,
) -> Option<BlockEntityProviders<T>> {
    let parts = installed()?.keys.capabilities.get(key.name)?.get(
        super::registry_keys::block_entity_type_name(at.kind(host)?)?,
    )?;
    Some(BlockEntityProviders {
        parts,
        at,
        handler: PhantomData,
    })
}

/// A block entity's providers of one capability ([`block_entity_providers`]).
// exhaustive: its fields are private, so mods cannot build or destructure it
pub struct BlockEntityProviders<T: ?Sized + 'static> {
    parts: &'static [CapabilityPart<BlockProviderOf>],
    at: BlockSide,
    handler: PhantomData<fn() -> Box<T>>,
}

impl<T: ?Sized + 'static> BlockEntityProviders<T> {
    /// A mod's handler where the game would answer with its own unless a mod comes first: one
    /// registered `before_game`, else, where the game has no answer (`game`), the first mod's.
    pub fn ask(self, host: &mut dyn ServerHost, game: bool) -> Option<Capability<T>> {
        let at = self.at;
        let answer = ask_providers(
            host,
            |index| next_provider(self.parts, index),
            BlockProviderOf::get::<T>,
            |provider, host| provider.provide(host, at),
            |_| game,
        );
        match answer {
            Answer::Mod(found) => Some(found),
            Answer::Game | Answer::Nobody => None,
        }
    }
}

/// The providers mods registered for `key` and the type of `entity`, for the game's own
/// lookups (a hopper's): none where no mod registered one, so a vanilla container entity's
/// lookup costs one table check and allocates nothing, and the game needn't work out its own
/// answer. [`EntityProviders::ask`] then asks them, as the game's lookups for block entities do.
pub fn entity_providers<T: ?Sized + 'static, C: Copy + 'static>(
    key: &EntityCapabilityKey<T, C>,
    host: &dyn Host,
    entity: EntityRef,
    context: C,
) -> Option<EntityProviders<T, C>> {
    let parts = installed()?
        .keys
        .entity_capabilities
        .get(key.name)?
        .get(&*entity.kind(host)?)?;
    Some(EntityProviders {
        parts,
        entity,
        context,
        handler: PhantomData,
    })
}

/// An entity's providers of one capability ([`entity_providers`]).
// exhaustive: its fields are private, so mods cannot build or destructure it
pub struct EntityProviders<T: ?Sized + 'static, C: 'static> {
    parts: &'static [CapabilityPart<EntityProviderOf>],
    entity: EntityRef,
    context: C,
    handler: PhantomData<fn() -> Box<T>>,
}

impl<T: ?Sized + 'static, C: Copy + 'static> EntityProviders<T, C> {
    /// A mod's handler where the game would answer with its own unless a mod comes first: one
    /// registered `before_game`, else, where the game has no answer (`game`), the first mod's.
    pub fn ask(self, host: &mut dyn ServerHost, game: bool) -> Option<Capability<T>> {
        let (entity, context) = (self.entity, self.context);
        match ask_providers(
            host,
            |index| next_provider(self.parts, index),
            EntityProviderOf::get::<T, C>,
            |provider, host| provider.provide(host, entity, context),
            |_| game,
        ) {
            Answer::Mod(found) => Some(found),
            Answer::Game | Answer::Nobody => None,
        }
    }
}

/// Walk only this capability and type's registrations, once each even if a provider switches
/// a mod off during the lookup. The table and the game's direct lookups share this path.
fn next_provider<P: Clone>(
    parts: &[CapabilityPart<P>],
    index: &mut u32,
) -> Option<(P, &'static str, bool)> {
    while let Some(part) = parts.get(*index as usize) {
        *index += 1;
        if super::mod_index_on(part.mod_index) {
            return Some((part.provider.clone(), part.mod_id, part.before_game));
        }
    }
    None
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

/// What a mod's [`Hooks::register`](super::Hooks::register) attaches its parts with, and
/// declares its block entity types and ticket controllers on. A thing takes one part of each
/// kind: the same kind twice on one thing refuses the mods at startup, naming both.
///
/// Mods call the generic methods on `dyn Registrar` (`r.item_part("mymod:wrench", Wrench)`);
/// they reach the trait's own methods, one implementation for native and wasm mods.
pub trait Registrar {
    /// Look up a vanilla or another mod's type. The final registry validates it at load.
    fn block_entity_type_key(&mut self, id: &str) -> super::BlockEntityTypeKey;
    /// Request a block by canonical registry id. All mods' declarations are checked at load.
    fn block_key(&mut self, id: &str) -> super::BlockKey;
    /// Request an effect by canonical registry id. All mods' declarations are checked at load.
    fn effect_key(&mut self, id: &str) -> super::EffectKey;
    /// Request a fluid by canonical registry id, checked when the registry loads.
    fn fluid_key(&mut self, id: &str) -> super::FluidKey;
    /// Declare a tank-only fluid in this mod's namespace and return its key.
    fn declare_fluid(&mut self, path: &str) -> super::FluidKey;
    /// Declare block entity type `path` in this mod's namespace, held by `blocks`; mods usually
    /// reach it through `block_entity_type`.
    fn declare_block_entity_type(
        &mut self,
        path: &str,
        blocks: Vec<String>,
    ) -> super::BlockEntityTypeKey;
    /// Register ticket controller `name`, the way NeoForge's `RegisterTicketControllersEvent`
    /// does once at startup, and keep the handle for `chunk.set_mod_forced`. `name` is
    /// `quarry` or `mymod:quarry`; the controller's id is in the calling mod's namespace
    /// (`mymod:quarry`), and another mod's namespace refuses the mods at startup, as does an id
    /// registered twice. `validates` is whether it validates its saved tickets: the
    /// `validate_tickets` hook then runs for it when a level loads them (NeoForge's
    /// `LoadingValidationCallback`). A world's tickets of a controller no loaded mod registers are
    /// dropped when it loads.
    fn register_ticket_controller(&mut self, name: &str, validates: bool) -> TicketController;
    /// Attach `part`, of kind `kind` (its type's name), to block `block`.
    fn attach_block_part(
        &mut self,
        block: super::BlockKey,
        kind: &'static str,
        part: Arc<dyn BlockPart>,
    );
    /// Attach `part`, of kind `kind`, to the mod block entity type `key`.
    fn attach_block_entity_part(
        &mut self,
        key: &super::BlockEntityTypeKey,
        kind: &'static str,
        part: Arc<dyn BlockEntityPart>,
    ) -> Result<(), super::BlockEntityPartError>;
    /// Attach `part`, of kind `kind`, to item `item`.
    fn attach_item_part(&mut self, item: &str, kind: &'static str, part: Arc<dyn ItemPart>);
    /// Attach `part`, of kind `kind`, to the mod menu type `menu`.
    fn attach_menu_part(&mut self, menu: &str, kind: &'static str, part: Arc<dyn MenuPart>);
    /// Attach `part`, of kind `kind`, to every block in tag `tag`.
    fn attach_block_part_tag(&mut self, tag: &str, kind: &'static str, part: Arc<dyn BlockPart>);
    /// Attach `part`, of kind `kind`, to every item in tag `tag`.
    fn attach_item_part_tag(&mut self, tag: &str, kind: &'static str, part: Arc<dyn ItemPart>);
    /// Answer block entity capability `name` for type `kind` with `provider`, asked ahead of the
    /// game's own when `before_game`; mods reach it through `block_entity_capability`.
    fn provide_block_entity_capability(
        &mut self,
        name: &'static str,
        kind: &super::BlockEntityTypeKey,
        before_game: bool,
        provider: BlockProviderOf,
    );
    /// Answer item capability `name` for item `item` with `provider`; mods reach it through
    /// `item_capability`.
    fn provide_item_capability(
        &mut self,
        name: &'static str,
        item: &str,
        before_game: bool,
        provider: ItemProviderOf,
    );
    /// Answer entity capability `name` for entity type `kind` with `provider`; mods reach it
    /// through `entity_capability`.
    fn provide_entity_capability(
        &mut self,
        name: &'static str,
        kind: &str,
        before_game: bool,
        provider: EntityProviderOf,
    );
    /// Read and write the item places `ItemPlace::Mod { provider: name, .. }` with `provider`,
    /// as an accessory mod exposes its slots. One provider a name.
    fn place_provider(&mut self, name: &'static str, provider: Arc<dyn PlaceProvider>);
    /// Register structure type `name` with `ty`; mods reach it through
    /// `register_structure_type`.
    fn add_structure_type(&mut self, name: &str, ty: Arc<dyn super::StructureType>);
}

impl dyn Registrar + '_ {
    /// Register structure type `name` (`tower` or `mymod:tower`), the way
    /// NeoForge's registries `STRUCTURE_TYPE` and `STRUCTURE_PIECE` register
    /// once at startup: a data pack structure with `"type": "modid:tower"`
    /// then generates by `ty`'s code — its own start and pieces, placed by the
    /// game's own spacing, biome and terrain-adaptation checks. The type's id
    /// is in the calling mod's namespace (`mymod:tower`); another mod's
    /// namespace, an empty name and an id registered twice refuse the mods at
    /// startup, naming the mod. `ty`'s `load_piece` is its saved pieces'
    /// loader: keep the type for as long as the game runs.
    pub fn register_structure_type<T: super::StructureType>(&mut self, name: &str, ty: T) {
        self.add_structure_type(name, Arc::new(ty));
    }
    /// Declare a block entity type in this mod's namespace.
    pub fn block_entity_type(&mut self, id: &str) -> super::BlockEntityTypeBuilder<'_> {
        super::BlockEntityTypeBuilder {
            id: id.to_owned(),
            registrar: self,
            blocks: Vec::new(),
        }
    }
    /// Attach `part` to block `block` (`mymod:shaft`, `minecraft:oak_log`).
    pub fn block_part<P: BlockPart>(&mut self, block: super::BlockKey, part: P) {
        self.attach_block_part(block, type_name::<P>(), Arc::new(part));
    }
    /// Attach a Part to a native type, including one another mod declares later.
    /// Vanilla keys are usable for capabilities, but not for native payload Parts.
    pub fn block_entity_part<P: BlockEntityPart>(
        &mut self,
        kind: &super::BlockEntityTypeKey,
        part: P,
    ) -> Result<(), super::BlockEntityPartError> {
        self.attach_block_entity_part(kind, type_name::<P>(), Arc::new(part))
    }
    /// Attach `part` to every block in tag `tag` (`minecraft:logs`, without `#`), as each game's
    /// tags have it when its world opens and after `/reload`.
    pub fn block_part_tag<P: BlockPart>(&mut self, tag: &str, part: P) {
        self.attach_block_part_tag(tag, type_name::<P>(), Arc::new(part));
    }
    /// Attach `part` to every item in tag `tag` (`minecraft:axes`, without `#`).
    pub fn item_part_tag<P: ItemPart>(&mut self, tag: &str, part: P) {
        self.attach_item_part_tag(tag, type_name::<P>(), Arc::new(part));
    }
    /// Attach `part` to item `item` (`mymod:wrench`).
    pub fn item_part<P: ItemPart>(&mut self, item: &str, part: P) {
        self.attach_item_part(item, type_name::<P>(), Arc::new(part));
    }
    /// Attach `part` to the mod menu type `menu`.
    pub fn menu_part<P: MenuPart>(&mut self, menu: &str, part: P) {
        self.attach_menu_part(menu, type_name::<P>(), Arc::new(part));
    }
    /// Answer capability `key` for block entity type `kind` with `provider`, which gives
    /// a handler for one block entity and side, or none:
    /// `r.block_entity_capability(&ITEMS, &press, |_, _| Some(Box::new(PressItems)))`.
    /// Like NeoForge's `RegisterCapabilitiesEvent`, providers add up: in the fork's chosen order
    /// (NeoForge's javadoc states none), the game's own is asked first, then the mods' in
    /// registration order, and the first to answer wins.
    pub fn block_entity_capability<T: ?Sized + 'static>(
        &mut self,
        key: &CapabilityKey<T>,
        kind: &super::BlockEntityTypeKey,
        provider: impl Fn(&mut dyn ServerHost, BlockSide) -> Option<Box<T>> + Send + Sync + 'static,
    ) {
        self.provide_block_entity_capability(
            key.name,
            kind,
            false,
            BlockProviderOf::of(Arc::new(provider)),
        );
    }
    /// `block_entity_capability`, asked ahead of the game's own
    /// provider (NeoForge's priority), as a mod that replaces how a block entity's slots are
    /// reached.
    pub fn block_entity_capability_before_game<T: ?Sized + 'static>(
        &mut self,
        key: &CapabilityKey<T>,
        kind: &super::BlockEntityTypeKey,
        provider: impl Fn(&mut dyn ServerHost, BlockSide) -> Option<Box<T>> + Send + Sync + 'static,
    ) {
        self.provide_block_entity_capability(
            key.name,
            kind,
            true,
            BlockProviderOf::of(Arc::new(provider)),
        );
    }
    /// Answer item capability `key` for item `item` (`mymod:backpack`) with `provider`, which
    /// gives a handler for the stack at one place, or none: `r.item_capability(&ITEM_ITEMS,
    /// "mymod:backpack", |_, _| Some(Box::new(Backpack)))`. Asked as
    /// `block_entity_capability`'s are.
    pub fn item_capability<T: ?Sized + 'static>(
        &mut self,
        key: &ItemCapabilityKey<T>,
        item: &str,
        provider: impl Fn(&mut dyn ServerHost, &ItemPlace) -> Option<Box<T>> + Send + Sync + 'static,
    ) {
        self.provide_item_capability(
            key.name,
            item,
            false,
            ItemProviderOf::of(Arc::new(provider)),
        );
    }
    /// `item_capability`, asked ahead of the game's own provider.
    pub fn item_capability_before_game<T: ?Sized + 'static>(
        &mut self,
        key: &ItemCapabilityKey<T>,
        item: &str,
        provider: impl Fn(&mut dyn ServerHost, &ItemPlace) -> Option<Box<T>> + Send + Sync + 'static,
    ) {
        self.provide_item_capability(key.name, item, true, ItemProviderOf::of(Arc::new(provider)));
    }
    /// Answer entity capability `key` for entity type `kind` (`mymod:robot`,
    /// `minecraft:player`) with `provider`, which gives a handler for one entity, or none:
    /// `r.entity_capability(&ENTITY_ITEMS, "mymod:robot", |_, robot, ()| Some(Box::new(Cargo(robot))))`.
    /// Asked as `block_entity_capability`'s are.
    pub fn entity_capability<T: ?Sized + 'static, C: 'static>(
        &mut self,
        key: &EntityCapabilityKey<T, C>,
        kind: &str,
        provider: impl Fn(&mut dyn ServerHost, EntityRef, C) -> Option<Box<T>> + Send + Sync + 'static,
    ) {
        self.provide_entity_capability(
            key.name,
            kind,
            false,
            EntityProviderOf::of(Arc::new(provider)),
        );
    }
    /// `entity_capability`, asked ahead of the game's own provider,
    /// as a mod that replaces how a player's inventory is reached.
    pub fn entity_capability_before_game<T: ?Sized + 'static, C: 'static>(
        &mut self,
        key: &EntityCapabilityKey<T, C>,
        kind: &str,
        provider: impl Fn(&mut dyn ServerHost, EntityRef, C) -> Option<Box<T>> + Send + Sync + 'static,
    ) {
        self.provide_entity_capability(
            key.name,
            kind,
            true,
            EntityProviderOf::of(Arc::new(provider)),
        );
    }
}

/// The game's [`Registrar`]: every mod's registrations, checked as they come.
// exhaustive: its fields are private, so mods cannot build or destructure it
pub struct PartRegistrar {
    mod_id: &'static str,
    mod_index: usize,
    /// Parts registered so far, by every mod: the next part's `order`.
    registered: usize,
    parts: Parts,
    tags: TagParts,
    block_entity_types: Vec<BlockEntityType>,
    registry_keys: Rc<RefCell<super::registry_keys::Requests>>,
    effects: OnceLock<super::registry_keys::Effects>,
    controllers: Vec<ControllerInfo>,
    structure_types: Vec<super::structure_type::StructureTypeInfo>,
    refused: Vec<String>,
}

impl PartRegistrar {
    fn new(mod_id: &'static str) -> Self {
        Self {
            mod_id,
            mod_index: 0,
            registered: 0,
            parts: Parts::default(),
            tags: TagParts::default(),
            block_entity_types: Vec::new(),
            registry_keys: Default::default(),
            effects: OnceLock::new(),
            controllers: Vec::new(),
            structure_types: Vec::new(),
            refused: Vec::new(),
        }
    }

    fn expand_block_entity_parts(&mut self) {
        for (id, parts) in &self.parts.block_entity_kinds {
            let Some(kind) = self.block_entity_types.iter().find(|kind| kind.id == **id) else {
                for part in parts {
                    self.refused.push(format!(
                        "mod {} attached {} to undeclared native block entity type {id}",
                        part.mod_id, part.type_name
                    ));
                }
                continue;
            };
            for block in &kind.blocks {
                for part in parts {
                    let block_part: Arc<dyn BlockPart> = part.part.clone();
                    if insert_part(
                        &mut self.parts.blocks,
                        block,
                        part.with_part(block_part),
                        &mut self.refused,
                    ) {
                        insert_part(
                            &mut self.parts.block_entities,
                            block,
                            part.clone(),
                            &mut self.refused,
                        );
                    }
                }
            }
        }
        for parts in self.parts.blocks.values_mut() {
            parts.sort_by_key(|part| (part.mod_index, part.order));
        }
        for parts in self.parts.block_entities.values_mut() {
            parts.sort_by_key(|part| (part.mod_index, part.order));
        }
    }
    /// Adds `provider` for capability `name` and `id` to `table`. Refused for a capability name
    /// or an id with no namespace, and for a capability name another provider answers with
    /// another handler type (two mods each declaring their own key under one name, rather than
    /// sharing the one the defining mod's `[api]` crate publishes): asked by one type, the other's
    /// providers would never answer.
    fn capability<P>(
        &mut self,
        table: fn(&mut Parts) -> &mut Capabilities<P>,
        name: &'static str,
        target: ProviderTarget<'_>,
        before_game: bool,
        (provider, handler): (P, &'static str),
    ) {
        let me = self.mod_id;
        if !namespaced(name) {
            self.refused.push(format!(
                "{me} answered capability {name:?}, which is not a namespaced name"
            ));
            return;
        }
        let block_entity_id;
        let id = match target {
            ProviderTarget::BlockEntity(kind) => {
                block_entity_id = self
                    .registry_keys
                    .borrow()
                    .block_entity_types
                    .id(kind.0)
                    .map(str::to_owned);
                let Some(id) = block_entity_id.as_deref() else {
                    return;
                };
                id
            }
            ProviderTarget::Id(id) => {
                if !namespaced(id) {
                    self.refused.push(format!(
                        "{me} answered {name} for {id:?}, which is not a namespaced id"
                    ));
                    return;
                }
                id
            }
        };
        let by_id = table(&mut self.parts).entry(name).or_default();
        if let Some(other) = by_id.values().flatten().next()
            && other.type_name != handler
        {
            self.refused.push(format!(
                "capability {name} is answered with two handler types, {} by {} and {handler} by {me}",
                other.type_name, other.mod_id,
            ));
            return;
        }
        let providers = by_id.entry(id.into()).or_default();
        // Those asked ahead of the game first, each kind in registration order.
        let at = if before_game {
            providers.partition_point(|part| part.before_game)
        } else {
            providers.len()
        };
        providers.insert(
            at,
            CapabilityPart {
                mod_id: me,
                mod_index: self.mod_index,
                type_name: handler,
                before_game,
                provider,
            },
        );
    }
    /// `part`, of kind `kind`, for tag `tag`; none if it is refused.
    fn tagged<P: ?Sized>(
        &mut self,
        kind: &'static str,
        tag: &str,
        part: Arc<P>,
    ) -> Option<(Box<str>, Attached<P>)> {
        if tag.starts_with('#') || !namespaced(tag) {
            self.refused.push(format!(
                "{} attached {kind} to tag {tag:?}, which is not a namespaced tag name (write it without #)",
                self.mod_id
            ));
            return None;
        }
        self.registered += 1;
        Some((
            tag.into(),
            Attached {
                mod_id: self.mod_id,
                mod_index: self.mod_index,
                order: self.registered,
                type_name: kind,
                part,
            },
        ))
    }

    /// Attach `part`, of kind `kind`, to `id` in `table`; false if it is refused.
    fn attach<P: ?Sized>(
        &mut self,
        table: fn(&mut Parts) -> &mut HashMap<Box<str>, Vec<Attached<P>>>,
        kind: &'static str,
        id: &str,
        part: Arc<P>,
    ) -> bool {
        if !namespaced(id) {
            self.refused.push(format!(
                "{} attached {kind} to {id:?}, which is not a namespaced id",
                self.mod_id
            ));
            return false;
        }
        self.attach_known(table, kind, id, part)
    }

    fn attach_known<P: ?Sized>(
        &mut self,
        table: fn(&mut Parts) -> &mut HashMap<Box<str>, Vec<Attached<P>>>,
        kind: &'static str,
        id: &str,
        part: Arc<P>,
    ) -> bool {
        let part = Attached {
            mod_id: self.mod_id,
            mod_index: self.mod_index,
            order: self.registered + 1,
            type_name: kind,
            part,
        };
        let inserted = insert_part(table(&mut self.parts), id, part, &mut self.refused);
        if inserted {
            self.registered += 1;
        }
        inserted
    }
}

impl Registrar for PartRegistrar {
    fn add_structure_type(&mut self, name: &str, ty: Arc<dyn super::StructureType>) {
        let (namespace, path) = name.split_once(':').unwrap_or((self.mod_id, name));
        let id = format!("{}:{}", self.mod_id, path);
        let refusal = if namespace != self.mod_id {
            Some(format!(
                "{} registered structure type {name:?} in the namespace {namespace:?}, which is not its own",
                self.mod_id
            ))
        } else if path.is_empty() {
            Some(format!(
                "{} registered a structure type with no name",
                self.mod_id
            ))
        } else {
            self.structure_types
                .iter()
                .any(|t| *t.id == *id)
                .then(|| format!("{} registered structure type {id} twice", self.mod_id))
        };
        if let Some(refusal) = refusal {
            self.refused.push(refusal);
            return;
        }
        let engine_type = super::structure_type::Adapter::new(&id, ty).into_engine_type();
        self.structure_types
            .push(super::structure_type::StructureTypeInfo {
                id: id.into(),
                ty: engine_type,
            });
    }

    fn block_entity_type_key(&mut self, id: &str) -> super::BlockEntityTypeKey {
        match self
            .registry_keys
            .borrow_mut()
            .block_entity_types
            .request(self.mod_id, id)
        {
            Ok(slot) => super::BlockEntityTypeKey(slot),
            Err(error) => {
                self.refused.push(error);
                super::BlockEntityTypeKey(u32::MAX)
            }
        }
    }

    fn block_key(&mut self, id: &str) -> super::BlockKey {
        match self
            .registry_keys
            .borrow_mut()
            .blocks
            .request(self.mod_id, id)
        {
            Ok(slot) => super::BlockKey(slot),
            Err(error) => {
                self.refused.push(error);
                super::BlockKey(u32::MAX)
            }
        }
    }

    fn effect_key(&mut self, id: &str) -> super::EffectKey {
        match self
            .registry_keys
            .borrow_mut()
            .effects
            .request(self.mod_id, id)
        {
            Ok(slot) => super::EffectKey(slot),
            Err(error) => {
                self.refused.push(error);
                super::EffectKey(u32::MAX)
            }
        }
    }

    fn fluid_key(&mut self, id: &str) -> super::FluidKey {
        match self
            .registry_keys
            .borrow_mut()
            .fluids
            .request(self.mod_id, id)
        {
            Ok(slot) => super::FluidKey(slot),
            Err(error) => {
                self.refused.push(error);
                super::FluidKey(u32::MAX)
            }
        }
    }

    fn declare_fluid(&mut self, path: &str) -> super::FluidKey {
        let id = format!("{}:{path}", self.mod_id);
        if path.contains(':') || !horizon_mods::content::namespaced_id(&id) {
            self.refused.push(format!(
                "mod {}: invalid fluid declaration {id:?}",
                self.mod_id
            ));
            return super::FluidKey(u32::MAX);
        }
        let previous = self
            .registry_keys
            .borrow_mut()
            .declared_fluids
            .insert(id.clone(), self.mod_id);
        if let Some(other) = previous {
            self.refused.push(format!(
                "fluid {id} declared by both {other} and {}",
                self.mod_id
            ));
        }
        self.fluid_key(&id)
    }

    fn declare_block_entity_type(
        &mut self,
        path: &str,
        blocks: Vec<String>,
    ) -> super::BlockEntityTypeKey {
        let id = format!("{}:{path}", self.mod_id);
        let key = self.block_entity_type_key(&id);
        self.block_entity_types.push(BlockEntityType { id, blocks });
        key
    }

    fn register_ticket_controller(&mut self, name: &str, validates: bool) -> TicketController {
        ticket_controller::register(
            &mut self.controllers,
            &mut self.refused,
            self.mod_id,
            name,
            validates,
        )
    }

    fn attach_block_part(
        &mut self,
        block: super::BlockKey,
        kind: &'static str,
        part: Arc<dyn BlockPart>,
    ) {
        let id = self
            .registry_keys
            .borrow()
            .blocks
            .id(block.0)
            .map(str::to_owned);
        let Some(id) = id else {
            self.refused.push(format!(
                "mod {}: invalid block key slot {}",
                self.mod_id, block.0
            ));
            return;
        };
        self.attach(|p| &mut p.blocks, kind, &id, part);
    }

    fn attach_block_entity_part(
        &mut self,
        key: &super::BlockEntityTypeKey,
        kind: &'static str,
        part: Arc<dyn BlockEntityPart>,
    ) -> Result<(), super::BlockEntityPartError> {
        let id = self
            .registry_keys
            .borrow()
            .block_entity_types
            .id(key.0)
            .map(str::to_owned)
            .unwrap_or_default();
        if id.starts_with("minecraft:") {
            return Err(super::BlockEntityPartError::VanillaType { kind: *key });
        }
        if self.attach_known(|p| &mut p.block_entity_kinds, kind, &id, part) {
            Ok(())
        } else {
            Err(super::BlockEntityPartError::Duplicate {
                kind: *key,
                part: kind,
            })
        }
    }

    fn attach_item_part(&mut self, item: &str, kind: &'static str, part: Arc<dyn ItemPart>) {
        self.attach(|p| &mut p.items, kind, item, part);
    }

    fn attach_menu_part(&mut self, menu: &str, kind: &'static str, part: Arc<dyn MenuPart>) {
        self.attach(|p| &mut p.menus, kind, menu, part);
    }

    fn attach_block_part_tag(&mut self, tag: &str, kind: &'static str, part: Arc<dyn BlockPart>) {
        let part = self.tagged(kind, tag, part);
        self.tags.blocks.extend(part);
    }

    fn attach_item_part_tag(&mut self, tag: &str, kind: &'static str, part: Arc<dyn ItemPart>) {
        let part = self.tagged(kind, tag, part);
        self.tags.items.extend(part);
    }

    fn provide_block_entity_capability(
        &mut self,
        name: &'static str,
        kind: &super::BlockEntityTypeKey,
        before_game: bool,
        provider: BlockProviderOf,
    ) {
        self.capability(
            |p| &mut p.capabilities,
            name,
            ProviderTarget::BlockEntity(kind),
            before_game,
            {
                let handler = provider.handler();
                (provider, handler)
            },
        );
    }

    fn provide_item_capability(
        &mut self,
        name: &'static str,
        item: &str,
        before_game: bool,
        provider: ItemProviderOf,
    ) {
        self.capability(
            |p| &mut p.item_capabilities,
            name,
            ProviderTarget::Id(item),
            before_game,
            {
                let handler = provider.handler();
                (provider, handler)
            },
        );
    }

    fn provide_entity_capability(
        &mut self,
        name: &'static str,
        kind: &str,
        before_game: bool,
        provider: EntityProviderOf,
    ) {
        self.capability(
            |p| &mut p.entity_capabilities,
            name,
            ProviderTarget::Id(kind),
            before_game,
            {
                let handler = provider.handler();
                (provider, handler)
            },
        );
    }

    /// Read and write the item places `ItemPlace::Mod { provider: name, .. }` with `provider`,
    /// as an accessory mod exposes its slots. One provider a name.
    fn place_provider(&mut self, name: &'static str, provider: Arc<dyn PlaceProvider>) {
        if !namespaced(name) {
            self.refused.push(format!(
                "{} provides item places {name:?}, which is not a namespaced name",
                self.mod_id
            ));
            return;
        }
        if let Some(other) = self.parts.place_providers.get(name) {
            self.refused.push(format!(
                "item places {name} are provided twice, by {} and {}",
                other.mod_id, self.mod_id
            ));
            return;
        }
        self.parts.place_providers.insert(
            name,
            PlaceProviderPart {
                mod_id: self.mod_id,
                mod_index: self.mod_index,
                provider,
            },
        );
    }
}

fn namespaced(id: &str) -> bool {
    id.split_once(':')
        .is_some_and(|(namespace, path)| !namespace.is_empty() && !path.is_empty())
}

/// Every mod's [`Hooks::register`](super::Hooks::register) run against one registrar, in dispatch order.
fn register_all(mods: &[ModEntry]) -> PartRegistrar {
    let mut registrar = PartRegistrar::new(mods.first().map_or("horizon", |entry| entry.id));
    struct RestoreKeys(Option<Rc<RefCell<super::registry_keys::Requests>>>);
    impl Drop for RestoreKeys {
        fn drop(&mut self) {
            REGISTERING_KEYS.set(self.0.take());
        }
    }
    let _scope = RestoreKeys(REGISTERING_KEYS.replace(Some(registrar.registry_keys.clone())));
    let context = super::RegistrationContext::new();
    for (index, hooks) in super::entered_registration(mods, "register").enumerate() {
        registrar.mod_id = mods[index].id;
        registrar.mod_index = index;
        hooks.register(&mut registrar, &context);
    }
    registrar.expand_block_entity_parts();
    registrar
}

/// The ticket controllers `mods` register, for a test's mods.
#[cfg(any(test, feature = "testing"))]
pub(super) fn registered_controllers(mods: &[ModEntry]) -> Vec<ControllerInfo> {
    if mods.is_empty() {
        return Vec::new();
    }
    register_all(mods).controllers
}

/// Builds the parts from every mod's [`Hooks::register`](super::Hooks::register), in dispatch order. Once, at startup;
/// the error lists every part refused, naming its mods.
pub fn install_parts(mods: &[ModEntry]) -> Result<(), Vec<String>> {
    install_registrar(register_all(mods))
}

fn install_registrar(registrar: PartRegistrar) -> Result<(), Vec<String>> {
    if !registrar.refused.is_empty() {
        return Err(registrar.refused);
    }
    let mut installed = Installed {
        keys: registrar.parts,
        tags: registrar.tags,
        block_entity_types: registrar.block_entity_types,
        resolved_block_entity_types: OnceLock::new(),
        registry_keys: Rc::try_unwrap(registrar.registry_keys)
            .unwrap_or_else(|_| unreachable!("registration scope has ended"))
            .into_inner(),
        effects: registrar.effects,
        fluids: OnceLock::new(),
        controllers: registrar.controllers,
        structure_types: registrar.structure_types,
        hook_bits: Vec::new(),
    };
    installed.keys.index_hooks();
    installed.hook_bits = installed.keys.hooks.iter().map(Option::is_some).collect();
    for (_, part) in &installed.tags.blocks {
        for hook in part.part.block_subscribed().iter().flatten() {
            installed.hook_bits[*hook as usize] = true;
        }
    }
    for (_, part) in &installed.tags.items {
        for hook in part.part.item_subscribed().iter().flatten() {
            installed.hook_bits[*hook as usize] = true;
        }
    }
    #[cfg(any(test, feature = "testing"))]
    TEST_PARTS.set(Some(Box::leak(Box::new(installed))));
    #[cfg(not(any(test, feature = "testing")))]
    if INSTALLED.set(installed).is_err() {
        log::warn!("Mod parts already installed; keeping the first table");
    }
    Ok(())
}

/// Fixtures that tests of other crates use to install Parts.
#[cfg(any(test, feature = "testing"))]
pub mod tests {
    use super::super::*;
    use super::mark_game_thread;
    #[cfg(test)]
    use super::{Attached, KnownIds, resolve_parts};
    #[cfg(test)]
    use std::any::type_name;
    #[cfg(test)]
    use std::collections::{HashMap, HashSet};
    use std::sync::Arc;

    /// Register the game fixture's final block registry through the actual registrar, retaining
    /// its installed test mods and Parts. Production mods request only the entries they use.
    pub fn register_game_keys(ids: impl IntoIterator<Item = String>) {
        let mut registrar = super::register_all(super::super::mods());
        for id in ids {
            registrar.block_key(&id);
        }
        super::install_registrar(registrar).unwrap();
        crate::registry_keys::resolve_effects().unwrap();
        crate::registry_keys::resolve_fluids(&[
            "minecraft:empty",
            "minecraft:water",
            "minecraft:flowing_water",
            "minecraft:lava",
            "minecraft:flowing_lava",
        ])
        .unwrap();
        if let Some(types) = horizon_client_core::block_entity_types::current() {
            crate::registry_keys::resolve_block_entity_types(types).unwrap();
        }
    }

    /// A previously requested block in the game fixture. Unknown names are a fixture error.
    pub fn block_key(id: &str) -> BlockKey {
        let canonical = if id.contains(':') {
            id.to_owned()
        } else {
            format!("minecraft:{id}")
        };
        BlockKey(
            *super::requested_keys()
                .blocks
                .slots
                .get(&canonical)
                .expect("fixture requested this block"),
        )
    }

    /// A registered effect exposed by the real load-owned mapping, also for unrequested vanilla entries.
    pub fn effect_key(id: &str) -> EffectKey {
        let index = horizon_client_core::potions::effect(id)
            .expect("fixture registered this effect")
            .index;
        crate::registry_keys::effect_key(index).expect("fixture resolved effect keys")
    }

    #[cfg(test)]
    fn type_key(id: &str) -> BlockEntityTypeKey {
        BlockEntityTypeKey(
            *super::requested_keys()
                .block_entity_types
                .slots
                .get(id)
                .expect("fixture requested this type"),
        )
    }

    /// The level the test worlds are.
    fn here() -> LevelRef {
        LevelRef::new(NativeWorldEpoch::new(1), "minecraft:overworld")
    }

    /// The block at `pos` in [`here`], through `side`.
    fn on(pos: [i32; 3], side: Option<Direction>) -> BlockSide {
        BlockSide {
            level: here(),
            pos,
            side,
        }
    }

    /// A comparator reading of 5 more than it was.
    struct Gauge;
    #[mod_api::horizon_part]
    impl BlockPart for Gauge {
        #[cfg_attr(test, inline)]
        fn analog_output(&self, _: &dyn LevelHost, event: &mut AnalogOutputEvent) {
            event.output += 5;
        }
        #[cfg_attr(test, cfg(any()))]
        fn should_check_weak_power(&self, _: &dyn LevelHost, _: &mut ShouldCheckWeakPowerEvent) {}
    }

    #[test]
    fn invalid_and_duplicate_fluid_declarations_are_refused() {
        let mut registrar = super::PartRegistrar::new("maker");
        registrar.declare_fluid("wrong:namespace");
        registrar.declare_fluid("chocolate");
        registrar.declare_fluid("chocolate");
        assert_eq!(registrar.refused.len(), 2);
        assert!(registrar.refused[0].contains("maker"));
        assert!(registrar.refused[1].contains("maker:chocolate"));
    }

    #[test]
    fn fluid_keys_resolve_declarations_after_requests_and_reject_unknown() {
        use crate::registry_keys::{fluid_id, fluid_key, resolve_fluids};
        struct Request;
        #[mod_api::__subscriptions]
        impl Hooks for Request {
            fn register(&self, r: &mut dyn Registrar, _: &RegistrationContext) {
                assert_eq!(r.fluid_key("water"), r.fluid_key("minecraft:water"));
                r.fluid_key("maker:chocolate");
            }
        }
        struct Declare;
        #[mod_api::__subscriptions]
        impl Hooks for Declare {
            fn register(&self, r: &mut dyn Registrar, _: &RegistrationContext) {
                r.declare_fluid("chocolate");
            }
        }
        let mods: Mods = vec![
            ModEntry::new("reader", Box::new(Request)),
            ModEntry::new("maker", Box::new(Declare)),
        ]
        .into();
        super::super::install(mods.clone());
        install_parts(&mods).unwrap();
        let error = resolve_fluids(&[]).unwrap_err();
        assert_eq!(
            error,
            vec![crate::UnknownFluid {
                mod_id: "reader",
                id: "minecraft:water".to_owned()
            }]
        );
        resolve_fluids(&["minecraft:water"]).unwrap();
        let chocolate = fluid_key("maker:chocolate").unwrap();
        assert_eq!(fluid_id(chocolate), Some("maker:chocolate"));
        assert_eq!(FluidKey::from_slot(chocolate.0), Ok(chocolate));
        assert!(fluid_key("missing:fluid").is_none());
    }

    #[test]
    fn registry_keys_resolve_reused_parts_and_rebind_without_changing_identity() {
        use crate::registry_keys::{ResolvedBlock, block, block_id, resolve_blocks};
        use horizon_world::level::states::BlockId;
        struct Keys(Arc<std::sync::Mutex<Vec<BlockKey>>>);
        #[mod_api::__subscriptions]
        impl Hooks for Keys {
            fn register(&self, r: &mut dyn Registrar, _: &RegistrationContext) {
                let vanilla = r.block_key("stone");
                let same = r.block_key("minecraft:stone");
                let later = r.block_key("later:machine");
                assert_eq!(vanilla, same);
                assert_eq!(BlockKey::from_slot(vanilla.0), Ok(vanilla));
                assert_eq!(BlockKey::from_slot(later.0), Ok(later));
                assert!(BlockKey::from_slot(later.0 + 1).is_err());
                r.block_part(vanilla, Gauge);
                r.block_part(same, Connector);
                r.block_part(later, Gauge);
                *self.0.lock().unwrap() = vec![vanilla, same, later];
            }
        }
        let requested = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mods: Mods = vec![ModEntry::new(
            "requester",
            Box::new(Keys(requested.clone())),
        )]
        .into();
        super::super::install(mods.clone());
        install_parts(&mods).unwrap();
        mark_game_thread();
        let keys = requested.lock().unwrap().clone();
        assert_eq!(BlockKey::from_slot(keys[0].0), Ok(keys[0]));
        assert!(BlockKey::from_slot(2).is_err());
        assert!(block(keys[0]).is_none(), "startup has no world mapping");
        // The final registry contains content declared after the requester's registration.
        let states = ["minecraft:stone", "later:machine", "later:machine"];
        resolve_parts(
            &HashMap::new(),
            &HashMap::new(),
            KnownIds {
                block: &|id| states.contains(&id),
                item: &|_| false,
                state_count: states.len(),
                state_block: &|i| states[i],
            },
        )
        .unwrap();
        assert_eq!(read(0), 6);
        assert_eq!(read(1), 6);
        assert_eq!(read(2), 6);
        let bind = |offset: u32| {
            resolve_blocks(|id| {
                let index: u32 = if id == "minecraft:stone" { 0 } else { 1 };
                Some(ResolvedBlock {
                    default: BlockStateId(index + offset),
                    states: BlockStateRange::new(BlockStateId(index + offset), 1),
                    block: BlockId(index as u16),
                })
            })
        };
        bind(0).unwrap();
        assert_eq!(block(keys[0]).unwrap().default, BlockStateId(0));
        let key = keys[0];
        // Declarations are load-owned, but a mapping belongs to its resolving thread.
        let installed = super::TEST_PARTS.get();
        std::thread::spawn(move || {
            super::TEST_PARTS.set(installed);
            assert_eq!(BlockKey::from_slot(key.0), Ok(key));
            assert_eq!(block_id(key), Some("minecraft:stone"));
            assert!(block(key).is_none());
        })
        .join()
        .unwrap();
        bind(20).unwrap();
        assert_eq!(block(keys[0]).unwrap().default, BlockStateId(20));
        assert_eq!(block(keys[2]).unwrap().default, BlockStateId(21));
        assert_eq!(block_id(keys[0]), Some("minecraft:stone"));
        assert_eq!(block_id(keys[2]), Some("later:machine"));
        let errors = resolve_blocks(|_| None).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error == "mod requester requested unknown block later:machine")
        );
        assert!(
            block(keys[0]).is_none(),
            "a refused reload must not leave stale state mappings"
        );
        assert!(BlockKey::from_slot(u32::MAX).is_err());
        assert_eq!(BlockKey::from_slot(keys[0].0), Ok(keys[0]));
        assert_eq!(block_id(keys[0]), Some("minecraft:stone"));
    }

    #[test]
    fn registry_keys_resolve_effects_declared_later_and_generated_name_reads() {
        struct Keys(Arc<std::sync::Mutex<Vec<EffectKey>>>);
        #[mod_api::__subscriptions]
        impl Hooks for Keys {
            fn register(&self, r: &mut dyn Registrar, _: &RegistrationContext) {
                let speed = r.effect_key("speed");
                let later = r.effect_key("key_test:later");
                assert_eq!(EffectKey::from_slot(speed.0), Ok(speed));
                assert_eq!(EffectKey::from_slot(later.0), Ok(later));
                assert!(EffectKey::from_slot(later.0 + 1).is_err());
                self.0.lock().unwrap().extend([speed, later]);
                assert_eq!(speed, r.effect_key("minecraft:speed"));
            }
        }
        let requested = Arc::new(std::sync::Mutex::new(Vec::new()));
        install_parts(&[
            ModEntry::new("first", Box::new(Keys(requested.clone()))),
            ModEntry::new("second", Box::new(Keys(requested.clone()))),
        ])
        .unwrap();
        let speed_key = requested.lock().unwrap()[0];
        assert_eq!(EffectKey::from_slot(speed_key.0), Ok(speed_key));
        assert!(EffectKey::from_slot(2).is_err());
        assert!(
            crate::registry_keys::effect(speed_key).is_none(),
            "startup has no resolved effects"
        );
        assert!(horizon_client_core::potions::register_mod_effects([(
            "key_test:later".to_owned(),
            0x123456,
            horizon_client_core::potions::Category::Neutral
        )]));
        crate::registry_keys::resolve_effects().unwrap();
        let keys = requested.lock().unwrap();
        assert_eq!(keys[0], keys[2]);
        assert_eq!(keys[1], keys[3]);
        let speed = crate::registry_keys::effect(keys[0]).unwrap();
        let later = crate::registry_keys::effect(keys[1]).unwrap();
        assert_eq!(speed.id, "speed");
        assert_eq!(later.id, "key_test:later");
        assert_eq!(later.color, 0x123456);
        assert_eq!(crate::registry_keys::effect_key(later.index), Some(keys[1]));
        for def in horizon_client_core::potions::all_effects() {
            let key = crate::registry_keys::effect_key(def.index).unwrap();
            assert_eq!(crate::registry_keys::effect(key).unwrap().id, def.id);
            assert_eq!(
                crate::registry_keys::effect_id(key),
                Some(horizon_client_core::potions::namespaced(def.id))
            );
        }

        assert!(EffectKey::from_slot(u32::MAX).is_err());
        struct Table;
        impl Backend for Table {
            fn effect_key_id(&self, key: EffectKey) -> String {
                crate::registry_keys::effect_id(key)
                    .expect("invalid effect key slot")
                    .to_owned()
            }
        }
        impl Host for Table {
            fn backend(&self) -> Option<BackendRef<'_>> {
                Some(BackendRef::of(self))
            }
        }
        assert_eq!(keys[0].id(&Table), "minecraft:speed");
        assert_eq!(keys[1].id(&Table), "key_test:later");
        struct NoGame;
        impl Host for NoGame {}
        assert!(std::panic::catch_unwind(|| keys[0].id(&NoGame)).is_err());
    }

    #[test]
    fn block_entity_slots_resolve_forward_declarations_and_refuse_unknown() {
        struct Request(Arc<std::sync::Mutex<Vec<BlockEntityTypeKey>>>);
        #[mod_api::__subscriptions]
        impl Hooks for Request {
            fn register(&self, r: &mut dyn Registrar, _: &RegistrationContext) {
                let later = r.block_entity_type_key("owner:press");
                assert_eq!(later, r.block_entity_type_key("owner:press"));
                assert_eq!(BlockEntityTypeKey::from_slot(later.0), Ok(later));
                assert!(BlockEntityTypeKey::from_slot(later.0 + 1).is_err());
                self.0.lock().unwrap().push(later);
            }
        }
        struct Declare(Arc<std::sync::Mutex<Vec<BlockEntityTypeKey>>>);
        #[mod_api::__subscriptions]
        impl Hooks for Declare {
            fn register(&self, r: &mut dyn Registrar, _: &RegistrationContext) {
                let declared = r.block_entity_type("press").build();
                let chest = r.block_entity_type_key("chest");
                self.0.lock().unwrap().extend([declared, chest]);
            }
        }
        let keys = Arc::new(std::sync::Mutex::new(Vec::new()));
        install_parts(&[
            ModEntry::new("first", Box::new(Request(keys.clone()))),
            ModEntry::new("owner", Box::new(Declare(keys.clone()))),
        ])
        .unwrap();
        let types = |names: &[&str]| {
            horizon_client_core::block_entity_types::BlockEntityTypes::new(
                Default::default(),
                names.iter().map(|s| (*s).to_owned()).collect(),
            )
        };
        assert_eq!(
            crate::registry_keys::resolve_block_entity_types(&types(&["minecraft:chest"]))
                .unwrap_err(),
            [
                "mod first requested unknown block entity type owner:press",
                "mod owner requested unknown block entity type owner:press"
            ]
        );
        crate::registry_keys::resolve_block_entity_types(&types(&[
            "minecraft:chest",
            "owner:press",
            "minecraft:furnace",
        ]))
        .unwrap();
        let keys = keys.lock().unwrap();
        assert_eq!(keys[0], keys[1]);
        assert_ne!(keys[0], keys[2]);
        assert_eq!(
            crate::registry_keys::block_entity_type_key("owner:press"),
            Some(keys[0])
        );
        assert_eq!(
            crate::registry_keys::block_entity_type_name(keys[2]),
            Some("minecraft:chest")
        );
        let furnace = crate::registry_keys::block_entity_type_key("minecraft:furnace").unwrap();
        assert_ne!(furnace, keys[2]);
        assert_eq!(BlockEntityTypeKey::from_slot(furnace.0), Ok(furnace));
        assert!(BlockEntityTypeKey::from_slot(u32::MAX).is_err());
        struct Table;
        impl Backend for Table {
            fn block_entity_type_key_id(&self, key: BlockEntityTypeKey) -> String {
                crate::registry_keys::block_entity_type_name(key)
                    .unwrap()
                    .to_owned()
            }
        }
        impl Host for Table {
            fn backend(&self) -> Option<BackendRef<'_>> {
                Some(BackendRef::of(self))
            }
        }
        assert_eq!(keys[0].id(&Table), "owner:press");
    }

    #[test]
    fn registry_keys_reject_unknown_unattached_requests_with_the_requesting_mod() {
        struct Unknown;
        #[mod_api::__subscriptions]
        impl Hooks for Unknown {
            fn register(&self, r: &mut dyn Registrar, _: &RegistrationContext) {
                r.block_key("absent:block");
                r.effect_key("absent:effect");
            }
        }
        install_parts(&[ModEntry::new("lookup", Box::new(Unknown))]).unwrap();
        assert_eq!(
            crate::registry_keys::resolve_blocks(|_| None).unwrap_err(),
            ["mod lookup requested unknown block absent:block"]
        );
        assert_eq!(
            crate::registry_keys::resolve_effects().unwrap_err(),
            ["mod lookup requested unknown effect absent:effect"]
        );
    }

    #[test]
    fn invalid_block_entity_type_paths_refuse_registration() {
        struct Invalid;
        #[mod_api::__subscriptions]
        impl super::super::Hooks for Invalid {
            fn register(&self, r: &mut dyn Registrar, _: &crate::RegistrationContext) {
                for path in ["", "other:press", "Uppercase", "has space"] {
                    r.block_entity_type(path).build();
                }
            }
        }
        let errors = install_parts(&[ModEntry::new("test", Box::new(Invalid))]).unwrap_err();
        assert_eq!(errors.len(), 4);
        assert!(
            errors
                .iter()
                .all(|error| error.starts_with("mod test: invalid registry key"))
        );
    }

    #[test]
    fn invalid_lookup_ids_refuse_registration_naming_the_requesting_mod() {
        struct Invalid;
        #[mod_api::__subscriptions]
        impl Hooks for Invalid {
            fn register(&self, r: &mut dyn Registrar, _: &mod_api::RegistrationContext) {
                for id in ["minecraft:", "Upper:chest", "test:has space"] {
                    r.block_entity_type_key(id);
                }
            }
        }
        let errors = install_parts(&[ModEntry::new("lookup", Box::new(Invalid))]).unwrap_err();
        assert_eq!(errors.len(), 3);
        assert!(
            errors
                .iter()
                .all(|error| error.starts_with("mod lookup: invalid registry key"))
        );
    }

    /// A structure type twice, one in another mod's namespace and one without
    /// a name: every registration is refused at startup, naming the mod.
    #[test]
    fn duplicate_and_foreign_structure_types_refuse_registration() {
        struct Tower;
        impl mod_api::StructureType for Tower {
            fn find_generation_point(
                &self,
                _: &mut dyn mod_api::GenerationContext,
            ) -> Option<mod_api::FoundStructure> {
                None
            }
            fn load_piece(
                &self,
                _: &mod_api::Compound,
            ) -> Option<std::sync::Arc<dyn mod_api::StructurePiece>> {
                None
            }
        }
        struct Invalid;
        #[mod_api::__subscriptions]
        impl Hooks for Invalid {
            fn register(&self, r: &mut dyn Registrar, _: &mod_api::RegistrationContext) {
                r.register_structure_type("tower", Tower);
                r.register_structure_type("tower", Tower);
                r.register_structure_type("other:tower", Tower);
                r.register_structure_type("", Tower);
            }
        }
        let errors = install_parts(&[ModEntry::new("test", Box::new(Invalid))]).unwrap_err();
        assert_eq!(errors.len(), 3);
        assert!(errors[0].contains("test registered structure type test:tower twice"));
        assert!(errors[1].contains("namespace"));
        assert!(errors[2].contains("no name"));
    }

    /// Connects redstone wire to its block from any side.
    struct Connector;
    #[mod_api::horizon_part]
    impl BlockPart for Connector {
        fn should_redstone_wire_connect_to(
            &self,
            _: &dyn LevelHost,
            event: &mut ShouldRedstoneWireConnectToEvent,
        ) {
            event.connected = true;
        }

        fn connects_to(&self, _: &dyn LevelHost, event: &mut ConnectsToEvent) {
            event.connected = event.kind == ConnectionKind::Wall;
        }
    }

    /// Makes a fence, wall or pane flip vanilla's answer toward the east and refuse every other
    /// side.
    struct ConnectionRule;
    #[mod_api::horizon_part]
    impl BlockPart for ConnectionRule {
        fn connects_to(&self, _: &dyn LevelHost, event: &mut ConnectsToEvent) {
            event.connected = event.direction == Direction::East && !event.connected;
        }
    }

    /// The block entity of type `doubler:press_entity`, held by block `test:press`: it ticks, and
    /// each tick adds a data slot.
    struct Press;
    #[mod_api::horizon_part]
    impl BlockPart for Press {
        fn analog_output(&self, _: &dyn LevelHost, event: &mut AnalogOutputEvent) {
            event.output += 40;
        }
    }
    #[mod_api::horizon_part]
    impl BlockEntityPart for Press {
        fn block_entity_ticks(&self, _: &dyn Host, event: &mut BlockEntityTicksEvent<'_>) {
            event.ticks = true;
        }
        fn block_entity_tick(&self, _: &mut dyn ServerHost, event: &mut BlockEntityTickEvent<'_>) {
            event.entity.data.push(1);
        }
        fn block_entity_view_distance(
            &self,
            _: &dyn ClientHost,
            event: &mut BlockEntityViewDistanceEvent<'_>,
        ) {
            event.distance = 128.0;
        }
    }
    /// The gold a `doubler:hoard` block entity holds, kept beside it.
    pub const HOARD: AttachmentType<u32> = AttachmentType {
        namespace: "test",
        name: "hoard",
        default: || 0,
        codec: None,
        copy_on_death: false,
        sync: false,
    };
    /// How many times a `doubler:hoard` was asked about its removal.
    pub static HOARD_ASKED: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    /// The block entity of type `doubler:hoard`, held by block `test:hoard`: when it is
    /// removed it gives back the gold its `HOARD` counts.
    struct Hoard(BlockKey);
    #[mod_api::horizon_part]
    impl BlockPart for Hoard {}
    #[mod_api::horizon_part]
    impl BlockEntityPart for Hoard {
        /// With data `[1]`, it replaces its own block with granite.
        fn block_entity_tick(
            &self,
            host: &mut dyn ServerHost,
            event: &mut BlockEntityTickEvent<'_>,
        ) {
            if event.entity.data == [1] {
                let level = host.level("minecraft:overworld").unwrap();
                let state = host.block_state_default(self.0).unwrap();
                assert!(level.set_block(
                    host,
                    event.pos,
                    state,
                    NativeBlockUpdateFlags::KNOWN_SHAPE
                ));
            }
        }
        fn pre_remove_side_effects(
            &self,
            level: &mut dyn LevelRandomHost,
            event: &mut PreRemoveSideEffectsEvent<'_>,
        ) {
            HOARD_ASKED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let BlockPos { x, y, z } = event.pos;
            if let Ok(Some(count @ 1..)) = HOARD.get_existing(level, Holder::BlockEntity([x, y, z]))
            {
                event
                    .drops
                    .push(ItemStack::new("minecraft:gold_ingot", count));
            }
        }
    }
    #[cfg(test)]
    struct NoHost;
    #[cfg(test)]
    impl Host for NoHost {}
    #[cfg(test)]
    impl ServerHost for NoHost {}
    #[cfg(test)]
    impl ClientHost for NoHost {}

    #[test]
    fn a_host_without_a_backend_refuses_handle_calls() {
        // NoHost implements nothing, so Host::backend keeps its None default:
        // every handle answers its typed refusal, never a no-op success.
        let level = LevelRef::new(NativeWorldEpoch::new(1), "minecraft:overworld");
        assert!(!level.in_place(&NoHost));
        assert_eq!(
            level.entities_in(&NoHost, [0.0, 0.0, 0.0], [1.0, 1.0, 1.0]),
            None
        );
    }

    /// A comparator reading of 100 more, for the blocks in tag `test:gauged`.
    struct TagGauge;
    #[mod_api::horizon_part]
    impl BlockPart for TagGauge {
        fn analog_output(&self, _: &dyn LevelHost, event: &mut AnalogOutputEvent) {
            event.output += 100;
        }
    }

    /// Doubles every comparator reading, and attaches `Gauge` to `test:gauge`, `TagGauge` to
    /// tag `test:gauged` and `Press` to `test:press`.
    struct Doubler;
    #[mod_api::__subscriptions]
    impl Hooks for Doubler {
        fn register(&self, r: &mut dyn Registrar, _: &crate::RegistrationContext) {
            let block_key = r.block_key("test:gauge");
            r.block_part(block_key, Gauge);
            let block_key = r.block_key("test:connector");
            r.block_part(block_key, Connector);
            r.block_part_tag("test:gauged", TagGauge);
            let kind = r
                .block_entity_type("press_entity")
                .blocks(&["test:press"])
                .build();
            r.block_entity_part(&kind, Press).expect("native type");
            let hoard = r.block_entity_type("hoard").blocks(&["test:hoard"]).build();
            let granite = r.block_key("minecraft:granite");
            r.block_entity_part(&hoard, Hoard(granite))
                .expect("native type");
            // These handlers wrap the test hosts' own slots and tanks, as a mod replacing how a
            // block entity's are reached does: ahead of the game's.
            let pipe = r.block_entity_type_key("test:pipe");
            r.block_entity_capability_before_game(&ITEMS, &pipe, |_, at| Some(Box::new(Pipe(at))));
            let greedy = r.block_entity_type_key("test:greedy");
            r.block_entity_capability_before_game(&ITEMS, &greedy, |_, at| {
                Some(Box::new(Greedy(at)))
            });
            let item_overstates = r.block_entity_type_key("test:item_overstates");
            r.block_entity_capability_before_game(&ITEMS, &item_overstates, |_, _| {
                Some(Box::new(ItemOverstates))
            });
            r.entity_capability(&ENTITY_ITEMS, "test:robot", |_, _, ()| {
                Some(Box::new(Cargo))
            });
            // A battery reached from any side but below, as a mod's sided entity storage.
            r.entity_capability(
                &ENTITY_ENERGY,
                "minecraft:armor_stand",
                |_, entity, side| {
                    (side != Some(Direction::Down)).then(|| Box::new(EntityBattery(entity)) as _)
                },
            );
            r.entity_capability(&ENTITY_FLUIDS, "minecraft:armor_stand", |_, entity, _| {
                Some(Box::new(EntityTank(entity)))
            });
            r.item_capability_before_game(&ITEM_ITEMS, "test:pouch", |_, place| {
                Some(Box::new(Pouch(place.clone())))
            });
            r.item_capability(&ITEM_ENERGY, "test:cell", |_, place| {
                Some(Box::new(Cell(place.clone())))
            });
            r.item_capability_before_game(&ITEM_ITEMS, "test:bag", |_, place| {
                Some(Box::new(Bag(place.clone())))
            });
            r.item_capability_before_game(&ITEM_ITEMS, "test:case", |_, place| {
                Some(Box::new(Case(place.clone())))
            });
            r.place_provider("test:charms", Arc::new(Charms));
            r.place_provider("test:lying-place", Arc::new(deep_places::LyingPlace));
            r.item_capability_before_game(&ITEM_ITEMS, "test:outer", |_, place| {
                Some(Box::new(deep_places::Wrap(place.clone())))
            });
            // `test:ordered`: one provider that declines, two that answer, and one ahead of the
            // game that answers only at x = 9.
            let ordered = r.block_entity_type_key("test:ordered");
            r.block_entity_capability(&ITEMS, &ordered, |_, _| None);
            r.block_entity_capability(&ITEMS, &ordered, |_, _| Some(Box::new(Marked(1))));
            r.block_entity_capability(&ITEMS, &ordered, |_, _| Some(Box::new(Marked(2))));
            r.block_entity_capability_before_game(&ITEMS, &ordered, |_, at| {
                (at.pos[0] == 9).then(|| Box::new(Marked(3)) as _)
            });
            r.block_entity_capability_before_game(&ITEMS, &ordered, |_, at| {
                (at.pos[0] >= 9).then(|| Box::new(Marked(4)) as _)
            });
            // `test:ordered_item` the same, the one ahead of the game answering for two items.
            r.item_capability(&ITEM_ITEMS, "test:ordered_item", |_, _| None);
            r.item_capability(&ITEM_ITEMS, "test:ordered_item", |_, _| {
                Some(Box::new(Marked(1)))
            });
            r.item_capability(&ITEM_ITEMS, "test:ordered_item", |_, _| {
                Some(Box::new(Marked(2)))
            });
            r.item_capability_before_game(&ITEM_ITEMS, "test:ordered_item", |host, place| {
                let two = place.amount(host) == 2;
                two.then(|| Box::new(Marked(3)) as _)
            });
            for (id, lie) in [
                ("test:tank", None),
                ("test:gives_more", Some(Lie::GivesMore)),
                ("test:takes_more", Some(Lie::TakesMore)),
            ] {
                let kind = r.block_entity_type_key(id);
                r.block_entity_capability_before_game(&FLUIDS, &kind, move |_, at| {
                    Some(Box::new(ModTank { at, lie }))
                });
            }
            for (id, lie) in [
                ("test:battery_gives_more", Lie::GivesMore),
                ("test:battery_takes_more", Lie::TakesMore),
            ] {
                let kind = r.block_entity_type_key(id);
                r.block_entity_capability_before_game(&ENERGY, &kind, move |_, at| {
                    Some(Box::new(ModBattery { at, lie }))
                });
            }
            let chem_tank = r.block_entity_type_key("test:chem_tank");
            r.block_entity_capability(&CHEMICALS, &chem_tank, |_, at| Some(Box::new(ChemTank(at))));
        }
    }

    #[cfg(test)]
    /// A test host's whole state, kept on each transaction it opens and put back on an abort, as
    /// the game's journal keeps its storages.
    trait Journal: Clone {
        fn kept(&mut self) -> &mut Vec<Self>;
    }
    #[cfg(test)]
    fn open<J: Journal>(host: &mut J) {
        let mut now = host.clone();
        now.kept().clear();
        host.kept().push(now);
    }
    #[cfg(test)]
    fn close<J: Journal>(host: &mut J, commit: bool) {
        let Some(was) = host.kept().pop() else {
            return;
        };
        if !commit {
            let kept = std::mem::take(host.kept());
            *host = was;
            *host.kept() = kept;
        }
    }

    /// A pipe's item capability, reached through `.0`: one slot from every side but its top,
    /// and what goes in is handed to the container east of it, through that container's west
    /// side.
    struct Pipe(BlockSide);
    impl ResourceHandler<ItemResource> for Pipe {
        fn size(&self, _: &mut dyn ServerHost) -> usize {
            usize::from(self.0.side != Some(Direction::Up))
        }
        fn resource(&self, _: &mut dyn ServerHost, _: usize) -> Option<ItemResource> {
            None
        }
        fn amount(&self, _: &mut dyn ServerHost, _: usize) -> u32 {
            0
        }
        fn capacity(&self, _: &mut dyn ServerHost, _: usize, _: &ItemResource) -> u32 {
            0
        }
        fn insert(
            &self,
            host: &mut dyn ServerHost,
            tx: &Transaction,
            _: usize,
            resource: &ItemResource,
            amount: u32,
        ) -> u32 {
            let [x, y, z] = self.0.pos;
            let east = [x + 1, y, z];
            let Some(next) = self
                .0
                .level
                .capability(host, &ITEMS, east, Some(Direction::West))
            else {
                return 0;
            };
            next.insert(host, tx, 0, resource, amount)
        }
        fn extract(
            &self,
            _: &mut dyn ServerHost,
            _: &Transaction,
            _: usize,
            _: &ItemResource,
            _: u32,
        ) -> u32 {
            0
        }
    }

    /// A pipe that hands what it's offered on as [`Pipe`] does, and answers one more than went.
    struct Greedy(BlockSide);
    impl ResourceHandler<ItemResource> for Greedy {
        fn size(&self, _: &mut dyn ServerHost) -> usize {
            1
        }
        fn resource(&self, _: &mut dyn ServerHost, _: usize) -> Option<ItemResource> {
            None
        }
        fn amount(&self, _: &mut dyn ServerHost, _: usize) -> u32 {
            0
        }
        fn capacity(&self, _: &mut dyn ServerHost, _: usize, _: &ItemResource) -> u32 {
            0
        }
        fn insert(
            &self,
            host: &mut dyn ServerHost,
            tx: &Transaction,
            index: usize,
            resource: &ItemResource,
            amount: u32,
        ) -> u32 {
            Pipe(self.0).insert(host, tx, index, resource, amount) + 1
        }
        fn extract(
            &self,
            _: &mut dyn ServerHost,
            _: &Transaction,
            _: usize,
            _: &ItemResource,
            _: u32,
        ) -> u32 {
            0
        }
    }

    #[cfg(test)]
    /// A pipe at (0, 64, 0), and a one-slot vanilla chest east of it, answered by the game.
    #[derive(Clone, Default)]
    struct PipeWorld {
        chest: Option<ItemStack>,
        kept: Vec<Self>,
    }
    #[cfg(test)]
    const PIPE: [i32; 3] = [0, 64, 0];
    #[cfg(test)]
    const CHEST: [i32; 3] = [1, 64, 0];
    #[cfg(test)]
    impl Journal for PipeWorld {
        fn kept(&mut self) -> &mut Vec<Self> {
            &mut self.kept
        }
    }
    #[cfg(test)]
    impl Backend for PipeWorld {
        fn level_in_place(&self, level: LevelRef) -> bool {
            level == here()
        }
        fn block_entity_kind(
            &self,
            BlockEntityRef { position: pos, .. }: BlockEntityRef,
        ) -> Option<BlockEntityTypeKey> {
            (pos == PIPE).then(|| type_key("test:pipe"))
        }
        fn container_size(&self, BlockSide { pos, .. }: BlockSide) -> Option<usize> {
            (pos == CHEST).then_some(1)
        }
        fn container_stack(
            &self,
            BlockSide { pos, .. }: BlockSide,
            slot: usize,
        ) -> Option<ItemStack> {
            self.chest.clone().filter(|_| pos == CHEST && slot == 0)
        }
    }
    #[cfg(test)]
    impl BackendMut for PipeWorld {
        fn container_insert(
            &mut self,
            BlockSide { pos, .. }: BlockSide,
            slot: usize,
            stack: ItemStack,
            _: &Transaction,
        ) -> Option<ItemStack> {
            if pos != CHEST || slot != 0 || self.chest.is_some() {
                return Some(stack);
            }
            self.chest = Some(stack);
            None
        }
    }
    #[cfg(test)]
    impl Host for PipeWorld {
        fn backend(&self) -> Option<BackendRef<'_>> {
            Some(BackendRef::of(self))
        }
        fn block_entity(&self, level: LevelRef, position: [i32; 3]) -> Option<BlockEntityRef> {
            Some(BlockEntityRef { level, position })
        }
    }
    #[cfg(test)]
    impl ServerHost for PipeWorld {
        fn transaction_open(&mut self, _: TransactionSeal) {
            open(self);
        }
        fn transaction_close(&mut self, _: TransactionSeal, commit: bool) {
            close(self, commit);
        }
        fn backend_mut(&mut self) -> Option<BackendMutRef<'_>> {
            Some(BackendMutRef::of(self))
        }
    }

    fn stick() -> ItemStack {
        stack("minecraft:stick", 1, 0)
    }

    fn stack(item: &str, count: u32, damage: u32) -> ItemStack {
        ItemStack {
            item: Arc::from(item),
            count,
            damage,
            extra: None,
        }
    }

    /// A pouch of sticks at `.0`: one slot, its stick count kept in the pouch's own stack (its
    /// damage, for the test), so the handler holds nothing.
    struct Pouch(ItemPlace);
    impl ResourceHandler<ItemResource> for Pouch {
        fn size(&self, _: &mut dyn ServerHost) -> usize {
            1
        }
        fn resource(&self, host: &mut dyn ServerHost, index: usize) -> Option<ItemResource> {
            (self.amount(host, index) > 0).then(|| ItemResource::of(&stick()))
        }
        fn amount(&self, host: &mut dyn ServerHost, _: usize) -> u32 {
            self.0.stack(host).map_or(0, |pouch| pouch.damage)
        }
        fn capacity(&self, _: &mut dyn ServerHost, _: usize, resource: &ItemResource) -> u32 {
            if resource.matches(&stick()) { 64 } else { 0 }
        }
        fn insert(
            &self,
            host: &mut dyn ServerHost,
            tx: &Transaction,
            _: usize,
            resource: &ItemResource,
            amount: u32,
        ) -> u32 {
            let Some(pouch) = self.0.stack(host).filter(|_| resource.matches(&stick())) else {
                return 0;
            };
            let filled = ItemResource::of(&stack("test:pouch", 1, pouch.damage + amount));
            match self.0.exchange(host, tx, &filled, 1) {
                1 => amount,
                _ => 0,
            }
        }
        fn extract(
            &self,
            _: &mut dyn ServerHost,
            _: &Transaction,
            _: usize,
            _: &ItemResource,
            _: u32,
        ) -> u32 {
            0
        }
    }

    /// A bag at `.0` holding one pouch: the pouch's sticks plus one kept in the bag's damage, 0
    /// when empty.
    struct Bag(ItemPlace);
    impl ResourceHandler<ItemResource> for Bag {
        fn size(&self, _: &mut dyn ServerHost) -> usize {
            1
        }
        fn resource(&self, host: &mut dyn ServerHost, _: usize) -> Option<ItemResource> {
            let held = self.0.stack(host)?.damage;
            (held > 0).then(|| ItemResource::of(&stack("test:pouch", 1, held - 1)))
        }
        fn amount(&self, host: &mut dyn ServerHost, index: usize) -> u32 {
            u32::from(self.resource(host, index).is_some())
        }
        fn capacity(&self, _: &mut dyn ServerHost, _: usize, resource: &ItemResource) -> u32 {
            u32::from(resource.item() == "test:pouch")
        }
        fn insert(
            &self,
            host: &mut dyn ServerHost,
            tx: &Transaction,
            _: usize,
            resource: &ItemResource,
            amount: u32,
        ) -> u32 {
            let pouch = resource.to_stack(1);
            match self.0.stack(host) {
                Some(bag) if bag.damage == 0 && amount > 0 && resource.item() == "test:pouch" => {
                    let full = ItemResource::of(&stack("test:bag", 1, pouch.damage + 1));
                    self.0.exchange(host, tx, &full, 1)
                }
                _ => 0,
            }
        }
        fn extract(
            &self,
            host: &mut dyn ServerHost,
            tx: &Transaction,
            index: usize,
            resource: &ItemResource,
            amount: u32,
        ) -> u32 {
            if amount == 0 || self.resource(host, index).as_ref() != Some(resource) {
                return 0;
            }
            let empty = ItemResource::of(&stack("test:bag", 1, 0));
            self.0.exchange(host, tx, &empty, 1)
        }
    }

    /// A case at `.0` with two slots kept in its damage: diamonds in the hundreds, sticks in
    /// the units. Before taking sticks it tries emptying its diamond slot in a transaction of
    /// its own and aborts that, as a handler checking what a write would do may.
    struct Case(ItemPlace);
    impl Case {
        fn held(&self, host: &mut dyn ServerHost, index: usize) -> u32 {
            let damage = self.0.stack(host).map_or(0, |case| case.damage);
            [damage / 100, damage % 100][index]
        }
        fn rewrite(&self, host: &mut dyn ServerHost, tx: &Transaction, damage: u32) -> bool {
            let case = ItemResource::of(&stack("test:case", 1, damage));
            self.0.exchange(host, tx, &case, 1) == 1
        }
    }
    impl ResourceHandler<ItemResource> for Case {
        fn size(&self, _: &mut dyn ServerHost) -> usize {
            2
        }
        fn resource(&self, host: &mut dyn ServerHost, index: usize) -> Option<ItemResource> {
            let item = ["minecraft:diamond", "minecraft:stick"][index];
            (self.held(host, index) > 0).then(|| ItemResource::of(&stack(item, 1, 0)))
        }
        fn amount(&self, host: &mut dyn ServerHost, index: usize) -> u32 {
            self.held(host, index)
        }
        fn capacity(&self, _: &mut dyn ServerHost, index: usize, resource: &ItemResource) -> u32 {
            let item = ["minecraft:diamond", "minecraft:stick"][index];
            if resource.item() == item { 99 } else { 0 }
        }
        fn insert(
            &self,
            host: &mut dyn ServerHost,
            tx: &Transaction,
            index: usize,
            resource: &ItemResource,
            amount: u32,
        ) -> u32 {
            if index != 1 || resource.item() != "minecraft:stick" {
                return 0;
            }
            let damage = self.0.stack(host).map_or(0, |case| case.damage);
            match self.rewrite(host, tx, damage + amount) {
                true => amount,
                false => 0,
            }
        }
        fn extract(
            &self,
            host: &mut dyn ServerHost,
            tx: &Transaction,
            index: usize,
            resource: &ItemResource,
            amount: u32,
        ) -> u32 {
            if index != 1 || resource.item() != "minecraft:stick" {
                return 0;
            }
            let damage = self.0.stack(host).map_or(0, |case| case.damage);
            let tried: Result<(), ()> = host.transaction(|host, tx| {
                self.rewrite(host, tx, damage % 100);
                Err(())
            });
            assert!(tried.is_err());
            let damage = self.0.stack(host).unwrap().damage;
            assert_eq!(damage / 100, 1, "diamond restored after inner abort");
            let took = amount.min(damage % 100);
            match self.rewrite(host, tx, damage - took) {
                true => took,
                false => 0,
            }
        }
    }

    /// An accessory mod's charm slots: charm `key` is kept in player 1's inventory slot 20 +
    /// `key`. A charm slot has nowhere to put what doesn't stack with its charm, so it refuses
    /// that.
    struct Charms;
    impl PlaceProvider for Charms {
        fn stack(&self, host: &mut dyn ServerHost, key: u64) -> Option<ItemStack> {
            backend_mut_of(host).and_then(|backend| backend.game_stack(&charm_slot(key)))
        }
        fn insert(
            &self,
            host: &mut dyn ServerHost,
            tx: &Transaction,
            key: u64,
            resource: &ItemResource,
            amount: u32,
        ) -> u32 {
            let place = charm_slot(key);
            match place.stack(host) {
                Some(held) if !resource.matches(&held) => 0,
                held => {
                    let room = 64 - held.map_or(0, |held| held.count);
                    place.insert(host, tx, resource, amount.min(room))
                }
            }
        }
        fn extract(
            &self,
            host: &mut dyn ServerHost,
            tx: &Transaction,
            key: u64,
            resource: &ItemResource,
            amount: u32,
        ) -> u32 {
            charm_slot(key).extract(host, tx, resource, amount)
        }
    }
    fn charm_slot(key: u64) -> ItemPlace {
        ItemPlace::PlayerSlot {
            player: 1,
            slot: 20 + key as i32,
        }
    }

    #[cfg(test)]
    /// Player 1's hands and slots, and what was given to them: an item split off their hand.
    #[derive(Clone, Default)]
    struct Player {
        main_hand: Option<ItemStack>,
        slots: HashMap<i32, ItemStack>,
        given: Vec<ItemStack>,
        kept: Vec<Self>,
    }
    const HAND: ItemPlace = ItemPlace::PlayerHand {
        player: 1,
        hand: Hand::Main,
    };
    #[cfg(test)]
    impl Journal for Player {
        fn kept(&mut self) -> &mut Vec<Self> {
            &mut self.kept
        }
    }
    #[cfg(test)]
    impl Backend for Player {
        fn level_in_place(&self, level: LevelRef) -> bool {
            level == here()
        }
        // Entity 1 is the player, 2 a pig, 3 a mod's robot, 4 a player in another level.
        fn entity_kind(&self, entity: EntityRef) -> Option<String> {
            let kind = [
                "minecraft:player",
                "minecraft:pig",
                "test:robot",
                "minecraft:player",
            ];
            kind.get(entity.id().checked_sub(1)? as usize)
                .map(|&kind| kind.into())
        }
        fn entity_level(&self, entity: EntityRef) -> Option<LevelRef> {
            Some(match entity.id() {
                4 => LevelRef::new(NativeWorldEpoch::new(1), "minecraft:the_nether"),
                _ => here(),
            })
        }
        fn entity_container_size(&self, _: u32) -> Option<usize> {
            None
        }
        fn entity_automation_size(&self, _: u32) -> Option<usize> {
            None
        }
        // The player holds hotbar slot 5.
        fn player_selected_slot(&self, _: PlayerRef) -> Option<usize> {
            Some(5)
        }
        fn item_container_slots(&self, _: &ItemStack) -> Option<Vec<Option<ItemStack>>> {
            None
        }
    }
    #[cfg(test)]
    impl BackendMut for Player {
        fn entity_uuid(&mut self, entity: EntityRef) -> Option<u128> {
            Some(u128::from(entity.id()))
        }
        fn game_stack(&mut self, place: &ItemPlace) -> Option<ItemStack> {
            match place {
                ItemPlace::PlayerHand { player: 1, .. } => self.main_hand.clone(),
                ItemPlace::PlayerSlot { player: 1, slot } => self.slots.get(slot).cloned(),
                _ => None,
            }
        }
        fn game_insert(&mut self, place: &ItemPlace, stack: ItemStack, _: &Transaction) -> u32 {
            if !self.is_place(place) {
                return 0;
            }
            let mut rest = stack.clone();
            match self.game_stack(place) {
                None => {
                    self.put(place, Some(stack.clone()));
                    rest.count = 0;
                }
                Some(mut held) if held.same_item_same_components(&rest) => {
                    let put = (64 - held.count).min(rest.count);
                    held.count += put;
                    rest.count -= put;
                    self.put(place, Some(held));
                }
                Some(_) => {}
            }
            self.given.extend((rest.count > 0).then_some(rest));
            stack.count
        }
        fn game_extract(&mut self, place: &ItemPlace, stack: ItemStack, _: &Transaction) -> u32 {
            let Some(mut held) = self.game_stack(place) else {
                return 0;
            };
            if !held.same_item_same_components(&stack) {
                return 0;
            }
            let took = held.count.min(stack.count);
            held.count -= took;
            self.put(place, (held.count > 0).then_some(held));
            took
        }
    }
    #[cfg(test)]
    impl Host for Player {
        fn player(&self, uuid: u128) -> Option<PlayerRef> {
            let entity = EntityRef::new(NativeWorldEpoch::new(1), u32::try_from(uuid).ok()?);
            Some(PlayerRef { entity, uuid })
        }
        fn item_default_component(&self, item: &str, component: &str) -> Option<Tag> {
            let helmet = item == "minecraft:iron_helmet";
            match component {
                "minecraft:max_stack_size" => Some(Tag::Int(if helmet { 1 } else { 64 })),
                "minecraft:equippable" if helmet => Some(Tag::Compound(
                    [("slot".into(), Tag::String("head".into()))]
                        .into_iter()
                        .collect(),
                )),
                _ => None,
            }
        }
        fn backend(&self) -> Option<BackendRef<'_>> {
            Some(BackendRef::of(self))
        }
    }
    #[cfg(test)]
    impl ServerHost for Player {
        fn transaction_open(&mut self, _: TransactionSeal) {
            open(self);
        }
        fn transaction_close(&mut self, _: TransactionSeal, commit: bool) {
            close(self, commit);
        }
        fn backend_mut(&mut self) -> Option<BackendMutRef<'_>> {
            Some(BackendMutRef::of(self))
        }
    }
    #[cfg(test)]
    impl Player {
        #[cfg(test)]
        fn is_place(&self, place: &ItemPlace) -> bool {
            matches!(
                place,
                ItemPlace::PlayerHand { player: 1, .. } | ItemPlace::PlayerSlot { player: 1, .. }
            )
        }
        #[cfg(test)]
        fn put(&mut self, place: &ItemPlace, stack: Option<ItemStack>) {
            match place {
                ItemPlace::PlayerHand { .. } => self.main_hand = stack,
                ItemPlace::PlayerSlot { slot, .. } => {
                    self.slots.remove(slot);
                    self.slots.extend(stack.map(|stack| (*slot, stack)));
                }
                _ => {}
            }
        }
    }

    /// A mod robot's three empty slots.
    struct Cargo;
    impl ResourceHandler<ItemResource> for Cargo {
        fn size(&self, _: &mut dyn ServerHost) -> usize {
            3
        }
        fn resource(&self, _: &mut dyn ServerHost, _: usize) -> Option<ItemResource> {
            None
        }
        fn amount(&self, _: &mut dyn ServerHost, _: usize) -> u32 {
            0
        }
        fn capacity(&self, _: &mut dyn ServerHost, _: usize, _: &ItemResource) -> u32 {
            0
        }
        fn insert(
            &self,
            _: &mut dyn ServerHost,
            _: &Transaction,
            _: usize,
            _: &ItemResource,
            _: u32,
        ) -> u32 {
            0
        }
        fn extract(
            &self,
            _: &mut dyn ServerHost,
            _: &Transaction,
            _: usize,
            _: &ItemResource,
            _: u32,
        ) -> u32 {
            0
        }
    }

    #[test]
    fn a_players_items_are_their_inventory_and_each_index_takes_only_what_fits_there() {
        game_thread();
        let entity = |id| EntityRef::new(NativeWorldEpoch::new(1), id);
        let helmet = || stack("minecraft:iron_helmet", 1, 0);
        let mut player = Player {
            slots: [
                (3, stack("minecraft:stick", 10, 0)),
                (5, stack("minecraft:stick", 60, 0)),
            ]
            .into_iter()
            .collect(),
            ..Player::default()
        };
        let host: &mut dyn ServerHost = &mut player;
        let items = entity(1).capability(host, &ENTITY_ITEMS, ()).unwrap();
        assert_eq!(items.owner(), "the game");
        assert_eq!(items.size(host), 41);
        let (sticks, helmet_) = (ItemResource::of(&stick()), ItemResource::of(&helmet()));
        // The main inventory takes a stack of anything; an armour slot only what goes there,
        // a helmet on the head (index 39), not the feet (36).
        assert_eq!(items.capacity(host, 0, &sticks), 64);
        assert_eq!(items.capacity(host, 36, &sticks), 0);
        assert_eq!(items.capacity(host, 39, &helmet_), 1);
        assert_eq!(items.capacity(host, 36, &helmet_), 0);
        assert_eq!(items.capacity(host, 40, &helmet_), 1);
        assert_eq!(items.capacity(host, 39, &sticks), 0);
        assert_eq!(items.amount(host, 3), 10);
        // Into one index, no further: 54 of 60 fill slot 3, and nothing goes elsewhere.
        let insert = |host: &mut dyn ServerHost, index, resource: &ItemResource, amount| {
            let went = host.transaction(|host, tx| {
                Ok::<_, ()>(items.insert(host, tx, index, resource, amount))
            });
            went.unwrap()
        };
        assert_eq!(insert(host, 3, &sticks, 60), 54);
        assert_eq!(insert(host, 38, &sticks, 1), 0);
        assert_eq!(insert(host, 39, &helmet_, 1), 1);
        assert_eq!(player.slots[&3].count, 64);
        assert_eq!(player.slots[&103], helmet());
        assert!(player.given.is_empty());
        // Without an index, as `Inventory.add`: the selected slot's sticks first, then an
        // empty slot.
        let host: &mut dyn ServerHost = &mut player;
        let aborted = host.transaction(|host, tx| {
            assert_eq!(items.insert_any(host, tx, &sticks, 10), 10);
            Err::<(), ()>(())
        });
        assert!(aborted.is_err());
        assert_eq!(player.slots[&5], stack("minecraft:stick", 60, 0));
        let host: &mut dyn ServerHost = &mut player;
        let went =
            host.transaction(|host, tx| Ok::<_, ()>(items.insert_any(host, tx, &sticks, 10)));
        assert_eq!(went, Ok(10));
        assert_eq!(player.slots[&5], stack("minecraft:stick", 64, 0));
        assert_eq!(player.slots[&0], stack("minecraft:stick", 6, 0));
        assert!(player.given.is_empty());
    }

    #[test]
    fn an_entitys_items_come_from_its_types_provider_and_none_outside_the_level_in_place() {
        game_thread();
        let entity = |id| EntityRef::new(NativeWorldEpoch::new(1), id);
        let mut player = Player::default();
        let host: &mut dyn ServerHost = &mut player;
        // A pig has no items; a mod's robot has its provider's; a player in a parked level
        // is asked nothing.
        assert!(entity(2).capability(host, &ENTITY_ITEMS, ()).is_none());
        let robot = entity(3).capability(host, &ENTITY_ITEMS, ()).unwrap();
        assert_eq!((robot.owner(), robot.size(host)), ("doubler".to_owned(), 3));
        assert!(entity(4).capability(host, &ENTITY_ITEMS, ()).is_none());
    }

    #[cfg(test)]
    /// Items counted with what they hold: a pouch's sticks, a bag's pouch and its sticks.
    fn items(stacks: impl IntoIterator<Item = ItemStack>) -> u32 {
        stacks
            .into_iter()
            .map(|s| match &*s.item {
                "test:pouch" | "test:bag" => s.count * (1 + s.damage),
                _ => s.count,
            })
            .sum()
    }
    #[cfg(test)]
    impl Player {
        #[cfg(test)]
        fn items(&self) -> u32 {
            let held = self.main_hand.iter().chain(self.slots.values());
            items(held.chain(&self.given).cloned())
        }
    }

    #[cfg(test)]
    /// Three shelf slots at (2, 64, 0), answered by the game's handler, else by
    /// `ItemOverstates`. A slot takes up to `limit` of one item, the `output` slot none, and
    /// gives at most one item per extract, unless `whole_extract`; with `one_per_insert`, it
    /// takes at most one per insert.
    #[derive(Clone)]
    struct Shelf {
        slots: [Option<ItemStack>; 3],
        limit: u32,
        output: Option<usize>,
        one_per_insert: bool,
        whole_extract: bool,
        overstates: bool,
        kept: Vec<Self>,
    }
    #[cfg(test)]
    fn shelf(slots: [Option<ItemStack>; 3]) -> Shelf {
        Shelf {
            slots,
            limit: 64,
            output: None,
            one_per_insert: false,
            whole_extract: false,
            overstates: false,
            kept: Vec::new(),
        }
    }
    const SHELF: [i32; 3] = [2, 64, 0];
    #[cfg(test)]
    fn shelf_slot(slot: usize) -> ItemPlace {
        ItemPlace::BlockEntity {
            level: here(),
            pos: SHELF,
            side: None,
            slot,
        }
    }
    #[cfg(test)]
    impl Journal for Shelf {
        fn kept(&mut self) -> &mut Vec<Self> {
            &mut self.kept
        }
    }
    #[cfg(test)]
    impl Backend for Shelf {
        fn level_in_place(&self, level: LevelRef) -> bool {
            level == here()
        }
        fn block_entity_kind(&self, _: BlockEntityRef) -> Option<BlockEntityTypeKey> {
            self.overstates.then(|| type_key("test:item_overstates"))
        }
        fn container_capacity(&self, _: BlockSide, slot: usize, _: &ItemStack) -> u32 {
            if self.output == Some(slot) {
                0
            } else {
                self.limit
            }
        }
        fn container_size(&self, BlockSide { pos, .. }: BlockSide) -> Option<usize> {
            (pos == SHELF).then_some(3)
        }
        fn container_stack(&self, _: BlockSide, slot: usize) -> Option<ItemStack> {
            self.slots.get(slot)?.clone()
        }
    }
    #[cfg(test)]
    impl BackendMut for Shelf {
        fn container_insert(
            &mut self,
            _: BlockSide,
            slot: usize,
            stack: ItemStack,
            _: &Transaction,
        ) -> Option<ItemStack> {
            let held = self.slots.get(slot)?.clone();
            let room = match &held {
                _ if self.output == Some(slot) => 0,
                None => self.limit,
                Some(held) if held.same_item_same_components(&stack) => self.limit - held.count,
                Some(_) => 0,
            };
            let most = if self.one_per_insert { 1 } else { u32::MAX };
            let put = room.min(stack.count).min(most);
            if put > 0 {
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
            _: BlockSide,
            slot: usize,
            max: u32,
            _: &Transaction,
        ) -> Option<ItemStack> {
            let most = if self.whole_extract { max } else { 1 };
            let held = self.slots.get_mut(slot)?.as_mut()?;
            let out = ItemStack {
                count: most.min(held.count),
                ..held.clone()
            };
            held.count -= out.count;
            if held.count == 0 {
                self.slots[slot] = None;
            }
            Some(out)
        }
    }
    #[cfg(test)]
    impl Host for Shelf {
        fn backend(&self) -> Option<BackendRef<'_>> {
            Some(BackendRef::of(self))
        }
        fn item_default_component(&self, item: &str, component: &str) -> Option<Tag> {
            // An iron sword's, as its item report gives them.
            let default = match component {
                "minecraft:max_damage" => Tag::Int(250),
                "minecraft:damage" => Tag::Int(0),
                "minecraft:enchantable" => {
                    Tag::Compound([("value".into(), Tag::Int(14))].into_iter().collect())
                }
                _ => return None,
            };
            (item == "minecraft:iron_sword").then_some(default)
        }
        fn block_entity(&self, level: LevelRef, position: [i32; 3]) -> Option<BlockEntityRef> {
            Some(BlockEntityRef { level, position })
        }
    }
    #[cfg(test)]
    impl ServerHost for Shelf {
        fn transaction_open(&mut self, _: TransactionSeal) {
            open(self);
        }
        fn transaction_close(&mut self, _: TransactionSeal, commit: bool) {
            close(self, commit);
        }
        fn backend_mut(&mut self) -> Option<BackendMutRef<'_>> {
            Some(BackendMutRef::of(self))
        }
    }
    #[cfg(test)]
    impl Shelf {
        #[cfg(test)]
        fn items(&self) -> u32 {
            items(self.slots.iter().flatten().cloned())
        }
    }

    /// A mod's handler for the shelf whose calls overstate: an extract takes and answers 5 more
    /// than asked, and an insert keeps nothing and answers 5 more than was offered.
    struct ItemOverstates;
    impl ResourceHandler<ItemResource> for ItemOverstates {
        fn size(&self, host: &mut dyn ServerHost) -> usize {
            backend_of(&*host)
                .and_then(|backend| backend.container_size(on(SHELF, None)))
                .unwrap_or(0)
        }
        fn resource(&self, host: &mut dyn ServerHost, index: usize) -> Option<ItemResource> {
            Some(ItemResource::of(
                &backend_of(&*host)?.container_stack(on(SHELF, None), index)?,
            ))
        }
        fn amount(&self, host: &mut dyn ServerHost, index: usize) -> u32 {
            backend_of(&*host)
                .and_then(|backend| backend.container_stack(on(SHELF, None), index))
                .map_or(0, |held| held.count)
        }
        fn capacity(
            &self,
            host: &mut dyn ServerHost,
            index: usize,
            resource: &ItemResource,
        ) -> u32 {
            backend_of(&*host).map_or(0, |backend| {
                backend.container_capacity(on(SHELF, None), index, &resource.to_stack(1))
            })
        }
        fn insert(
            &self,
            _: &mut dyn ServerHost,
            _: &Transaction,
            _: usize,
            _: &ItemResource,
            amount: u32,
        ) -> u32 {
            amount + 5
        }
        fn extract(
            &self,
            host: &mut dyn ServerHost,
            tx: &Transaction,
            index: usize,
            _: &ItemResource,
            amount: u32,
        ) -> u32 {
            let Some(backend) = backend_mut_of(host) else {
                return 0;
            };
            backend
                .container_extract(on(SHELF, None), index, amount + 5, tx)
                .map_or(0, |out| out.count)
        }
    }

    #[cfg(test)]
    /// State 1 is a `test:gauge`, state 2 a `test:other`.
    struct Level;
    #[cfg(test)]
    impl LevelHost for Level {
        fn block_state(&self, _: [i32; 3]) -> BlockStateId {
            unimplemented!()
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

    /// A chemical, as Mekanism's: the test mods' own resource, which the game knows nothing of.
    #[derive(Clone, Debug, PartialEq)]
    // exhaustive: the game builds it with a literal; mods only read it
    pub struct Chemical(pub String);

    /// The test mods' own capability, as Mekanism's `Capabilities.CHEMICAL`: the game has no
    /// answer for it, so only their providers do.
    pub const CHEMICALS: CapabilityKey<dyn ResourceHandler<Chemical>> =
        CapabilityKey::new("test:chemical");

    /// A mod block entity's one tank of 1000 of a chemical, kept in its saved data (`chemical`
    /// and `amount`), whose writes the game's journal keeps for a transaction.
    // exhaustive: the game builds it with a literal; mods only read it
    pub struct ChemTank(pub BlockSide);
    impl ChemTank {
        fn entity(&self, host: &dyn Host) -> Option<BlockEntityRef> {
            host.block_entity(self.0.level, self.0.pos)
        }
        fn held(&self, host: &dyn Host) -> Option<(Chemical, u32)> {
            let data = self.entity(host)?.data(host)?;
            let Some(Tag::String(chemical)) = data.get(&"chemical".into()) else {
                return None;
            };
            let Some(&Tag::Int(amount)) = data.get(&"amount".into()) else {
                return None;
            };
            let amount = u32::try_from(amount).ok().filter(|&a| a > 0)?;
            Some((Chemical(chemical.to_string_lossy()), amount))
        }
        fn set(&self, host: &mut dyn ServerHost, chemical: &Chemical, amount: u32) {
            let Some(entity) = self.entity(host) else {
                return;
            };
            let mut data = Compound::new();
            data.insert("chemical".into(), Tag::String(chemical.0.as_str().into()));
            data.insert("amount".into(), Tag::Int(amount as i32));
            entity.merge_data(host, data);
        }
    }
    impl ResourceHandler<Chemical> for ChemTank {
        fn size(&self, _: &mut dyn ServerHost) -> usize {
            1
        }
        fn resource(&self, host: &mut dyn ServerHost, _: usize) -> Option<Chemical> {
            self.held(host).map(|(chemical, _)| chemical)
        }
        fn amount(&self, host: &mut dyn ServerHost, _: usize) -> u32 {
            self.held(host).map_or(0, |(_, amount)| amount)
        }
        fn capacity(&self, _: &mut dyn ServerHost, _: usize, _: &Chemical) -> u32 {
            1000
        }
        fn insert(
            &self,
            host: &mut dyn ServerHost,
            _: &Transaction,
            _: usize,
            chemical: &Chemical,
            amount: u32,
        ) -> u32 {
            let held = match self.held(host) {
                Some((other, _)) if other != *chemical => return 0,
                Some((_, held)) => held,
                None => 0,
            };
            let went = amount.min(1000 - held);
            if went > 0 {
                self.set(host, chemical, held + went);
            }
            went
        }
        fn extract(
            &self,
            host: &mut dyn ServerHost,
            _: &Transaction,
            _: usize,
            chemical: &Chemical,
            amount: u32,
        ) -> u32 {
            let Some((held_chemical, held)) = self.held(host).filter(|(c, _)| c == chemical) else {
                return 0;
            };
            let came = amount.min(held);
            self.set(host, &held_chemical, held - came);
            came
        }
    }

    const ROBOT_ENERGY: AttachmentType<u32> = AttachmentType {
        namespace: "test",
        name: "energy",
        default: || 0,
        codec: None,
        copy_on_death: false,
        sync: false,
    };
    /// A fluid attachment type for the tests.
    pub const ROBOT_FLUID: AttachmentType<u32> = AttachmentType {
        namespace: "test",
        name: "fluid",
        default: || 0,
        codec: None,
        copy_on_death: false,
        sync: false,
    };

    struct EntityBattery(EntityRef);
    impl EnergyHandler for EntityBattery {
        fn amount(&self, host: &mut dyn ServerHost) -> u32 {
            ROBOT_ENERGY
                .get_existing(host, Holder::Entity(self.0.id()))
                .unwrap()
                .unwrap_or(0)
        }
        fn capacity(&self, _: &mut dyn ServerHost) -> u32 {
            1000
        }
        fn insert(&self, host: &mut dyn ServerHost, _: &Transaction, amount: u32) -> u32 {
            let held = self.amount(host);
            let moved = amount.min(1000 - held);
            ROBOT_ENERGY
                .set(host, Holder::Entity(self.0.id()), held + moved)
                .unwrap();
            moved
        }
        fn extract(&self, host: &mut dyn ServerHost, _: &Transaction, amount: u32) -> u32 {
            let held = self.amount(host);
            let moved = amount.min(held);
            ROBOT_ENERGY
                .set(host, Holder::Entity(self.0.id()), held - moved)
                .unwrap();
            moved
        }
    }

    struct EntityTank(EntityRef);
    impl ResourceHandler<FluidResource> for EntityTank {
        fn size(&self, _: &mut dyn ServerHost) -> usize {
            1
        }
        fn resource(&self, host: &mut dyn ServerHost, _: usize) -> Option<FluidResource> {
            (self.amount(host, 0) > 0).then(|| {
                FluidResource::of(&FluidStack::new(
                    crate::registry_keys::fluid_key("minecraft:water").unwrap(),
                    1,
                ))
            })
        }
        fn amount(&self, host: &mut dyn ServerHost, _: usize) -> u32 {
            ROBOT_FLUID
                .get_existing(host, Holder::Entity(self.0.id()))
                .unwrap()
                .unwrap_or(0)
        }
        fn capacity(&self, _: &mut dyn ServerHost, index: usize, resource: &FluidResource) -> u32 {
            if index == 0
                && resource
                    == &FluidResource::of(&FluidStack::new(
                        crate::registry_keys::fluid_key("minecraft:water").unwrap(),
                        1,
                    ))
            {
                1000
            } else {
                0
            }
        }
        fn insert(
            &self,
            host: &mut dyn ServerHost,
            _: &Transaction,
            index: usize,
            resource: &FluidResource,
            amount: u32,
        ) -> u32 {
            let held = self.amount(host, index);
            let moved = amount.min(self.capacity(host, index, resource).saturating_sub(held));
            ROBOT_FLUID
                .set(host, Holder::Entity(self.0.id()), held + moved)
                .unwrap();
            moved
        }
        fn extract(
            &self,
            host: &mut dyn ServerHost,
            _: &Transaction,
            index: usize,
            resource: &FluidResource,
            amount: u32,
        ) -> u32 {
            let held = self.amount(host, index);
            let moved = if self.capacity(host, index, resource) > 0 {
                amount.min(held)
            } else {
                0
            };
            ROBOT_FLUID
                .set(host, Holder::Entity(self.0.id()), held - moved)
                .unwrap();
            moved
        }
    }

    /// A second test mod, sharing `CHEMICALS` and `Chemical` with the first, as a mod depending
    /// on another's `[api]` crate does: its vats answer the same capability.
    struct Chemist;
    #[mod_api::__subscriptions]
    impl Hooks for Chemist {
        fn register(&self, r: &mut dyn Registrar, _: &crate::RegistrationContext) {
            let chem_vat = r.block_entity_type_key("test:chem_vat");
            r.block_entity_capability(&CHEMICALS, &chem_vat, |_, at| Some(Box::new(ChemTank(at))));
        }
    }

    const CAPABILITY_TYPES: &[&str] = &[
        "pipe",
        "greedy",
        "item_overstates",
        "ordered",
        "tank",
        "gives_more",
        "takes_more",
        "battery_gives_more",
        "battery_takes_more",
        "chem_tank",
        "chem_vat",
    ];

    struct FixtureContent;
    #[mod_api::__subscriptions]
    impl Hooks for FixtureContent {
        fn register(&self, r: &mut dyn Registrar, _: &crate::RegistrationContext) {
            r.declare_fluid("potion");
            for id in CAPABILITY_TYPES {
                r.block_entity_type(id)
                    .blocks(&[&format!("test:{id}")])
                    .build();
            }
        }
        fn blocks(&self, _context: &mod_api::RegistrationContext, event: &mut BlocksEvent<'_>) {
            event.blocks.extend(
                ["gauge", "connector", "press", "hoard"]
                    .into_iter()
                    .chain(CAPABILITY_TYPES.iter().copied())
                    .map(|id| horizon_mods::content::Block {
                        id: format!("test:{id}"),
                        template: "minecraft:stone".into(),
                        ..Default::default()
                    }),
            );
        }
    }

    /// The test mods, their parts installed, on a thread that runs a game.
    pub fn game_thread() -> super::super::Mods {
        let mods: super::super::Mods = vec![
            ModEntry::new("doubler", Box::new(Doubler)),
            ModEntry::new("chemist", Box::new(Chemist)),
            ModEntry::new("test", Box::new(FixtureContent)),
        ]
        .into();
        super::super::install(mods.clone());
        install_parts(&mods).unwrap();
        crate::registry_keys::resolve_fluids(&[
            "minecraft:water",
            "minecraft:lava",
            "minecraft:flowing_water",
            "minecraft:flowing_lava",
            "minecraft:empty",
        ])
        .unwrap();
        mark_game_thread();
        mods
    }

    /// Install a signal part on glass for a real-game query.
    pub fn signal_game_thread() {
        struct Signal;
        #[mod_api::horizon_part]
        impl BlockPart for Signal {
            fn block_signal(&self, level: &dyn LevelHost, event: &mut BlockSignalEvent) {
                assert_eq!(
                    level.block_state([event.pos.x, event.pos.y, event.pos.z]),
                    event.state
                );
                event.signal = if event.direct { -2 } else { 20 };
            }
        }
        struct SignalMod;
        #[mod_api::__subscriptions]
        impl Hooks for SignalMod {
            fn register(&self, r: &mut dyn Registrar, _: &crate::RegistrationContext) {
                let block_key = r.block_key("minecraft:glass");
                r.block_part(block_key, Signal);
            }
        }
        let mods: super::super::Mods =
            vec![ModEntry::new("signal_test", Box::new(SignalMod))].into();
        super::super::install(mods.clone());
        install_parts(&mods).unwrap();
        mark_game_thread();
    }

    /// Install a weak-power part on glass for a real-game query.
    pub fn weak_power_game_thread() {
        struct WeakPower;
        #[mod_api::horizon_part]
        impl BlockPart for WeakPower {
            fn should_check_weak_power(
                &self,
                _: &dyn LevelHost,
                event: &mut ShouldCheckWeakPowerEvent,
            ) {
                event.check = true;
            }
        }
        struct WeakPowerMod;
        #[mod_api::__subscriptions]
        impl Hooks for WeakPowerMod {
            fn register(&self, r: &mut dyn Registrar, _: &crate::RegistrationContext) {
                let block_key = r.block_key("minecraft:glass");
                r.block_part(block_key, WeakPower);
            }
        }
        let mods: super::super::Mods =
            vec![ModEntry::new("weak_power_test", Box::new(WeakPowerMod))].into();
        super::super::install(mods.clone());
        install_parts(&mods).unwrap();
        mark_game_thread();
    }

    /// Install parts that change what a path may cross: glass becomes land and air, air
    /// stops being land and becomes water.
    pub fn pathfindable_game_thread() {
        struct Answers(&'static [(PathComputationType, bool)]);
        #[mod_api::horizon_part]
        impl BlockPart for Answers {
            fn is_pathfindable(&self, _: &dyn Host, event: &mut IsPathfindableEvent) {
                if let Some((_, answer)) = self.0.iter().find(|(kind, _)| *kind == event.kind) {
                    event.pathfindable = *answer;
                }
            }
        }
        struct PathMod;
        #[mod_api::__subscriptions]
        impl Hooks for PathMod {
            fn register(&self, r: &mut dyn Registrar, _: &crate::RegistrationContext) {
                let block_key = r.block_key("minecraft:glass");
                r.block_part(
                    block_key,
                    Answers(&[
                        (PathComputationType::Land, true),
                        (PathComputationType::Air, true),
                    ]),
                );
                let block_key = r.block_key("minecraft:air");
                r.block_part(
                    block_key,
                    Answers(&[
                        (PathComputationType::Land, false),
                        (PathComputationType::Water, true),
                    ]),
                );
            }
        }
        let mods: super::super::Mods = vec![ModEntry::new("path_test", Box::new(PathMod))].into();
        super::super::install(mods.clone());
        install_parts(&mods).unwrap();
        mark_game_thread();
    }

    /// Install a part on glass that asks, when a glass block is removed, for every neighbour to
    /// be told, as an override of `affectNeighborsAfterRemoval` calling `updateNeighborsAt`.
    pub fn removal_game_thread() {
        struct Leaver;
        #[mod_api::horizon_part]
        impl BlockPart for Leaver {
            fn affect_neighbors_after_removal(
                &self,
                _: &dyn LevelHost,
                event: &mut AffectNeighborsAfterRemovalEvent<'_>,
            ) {
                event.updates.push(NeighbourUpdate::Neighbours);
            }
        }
        struct LeaverMod;
        #[mod_api::__subscriptions]
        impl Hooks for LeaverMod {
            fn register(&self, r: &mut dyn Registrar, _: &crate::RegistrationContext) {
                let block_key = r.block_key("minecraft:glass");
                r.block_part(block_key, Leaver);
            }
        }
        let mods: super::super::Mods =
            vec![ModEntry::new("leaver_test", Box::new(LeaverMod))].into();
        super::super::install(mods.clone());
        install_parts(&mods).unwrap();
        mark_game_thread();
    }

    /// Install a connection rule on oak fences, cobblestone walls and glass panes for real-game
    /// tests: it flips vanilla's answer.
    pub fn connection_game_thread() {
        struct ConnectionMod;
        #[mod_api::__subscriptions]
        impl Hooks for ConnectionMod {
            fn register(&self, r: &mut dyn Registrar, _: &crate::RegistrationContext) {
                for block in [
                    "minecraft:oak_fence",
                    "minecraft:cobblestone_wall",
                    "minecraft:glass_pane",
                ] {
                    let block_key = r.block_key(block);
                    r.block_part(block_key, ConnectionRule);
                }
            }
        }
        let mods: super::super::Mods =
            vec![ModEntry::new("connection_test", Box::new(ConnectionMod))].into();
        super::super::install(mods.clone());
        install_parts(&mods).unwrap();
        mark_game_thread();
    }

    std::thread_local! {
        static SIGNAL_READS: std::cell::RefCell<Vec<SignalRead>> = const { std::cell::RefCell::new(Vec::new()) };
    }

    /// What the signal-reading glass saw: in `neighbor_changed`, whether it is powered and
    /// how strongly, and whether its tick was scheduled; or that its tick ran.
    #[derive(Clone, Debug, PartialEq, Eq)]
    // exhaustive: a test fixture's record, matched whole by the game's test
    pub enum SignalRead {
        /// `neighbor_changed`: `has_neighbor_signal`, `best_neighbor_signal`, the scheduling's answer.
        Changed([i32; 3], bool, i32, bool),
        /// `tick_block` ran at this position, reading the signal from its south side.
        Ticked([i32; 3], i32),
    }

    /// The reads the signal-reading glass recorded on this thread, taken.
    pub fn signal_reads() -> Vec<SignalRead> {
        SIGNAL_READS.with(|r| std::mem::take(&mut *r.borrow_mut()))
    }

    /// Install a part on glass that, when a neighbour changes, reads its inputs and
    /// schedules its own tick 2 ticks later, as a diode does, for real-game tests.
    pub fn signal_reader_game_thread() {
        struct Reader(BlockKey);
        #[mod_api::horizon_part]
        impl BlockPart for Reader {
            fn neighbor_changed(
                &self,
                level: &mut dyn crate::LevelTicksHost,
                event: &mut crate::NeighborChangedEvent,
            ) {
                let pos = [event.pos.x, event.pos.y, event.pos.z];
                let read = SignalRead::Changed(
                    pos,
                    level.has_neighbor_signal(pos),
                    level.best_neighbor_signal(pos),
                    level.schedule_block_tick(pos, self.0, 2),
                );
                SIGNAL_READS.with(|r| r.borrow_mut().push(read));
            }
            fn tick_block(
                &self,
                level: &mut dyn crate::LevelWriteHost,
                event: &mut crate::TickBlockEvent,
            ) -> Flow {
                let pos = [event.pos.x, event.pos.y, event.pos.z];
                let south = [pos[0], pos[1], pos[2] + 1];
                let read = SignalRead::Ticked(pos, level.signal(south, Direction::South));
                SIGNAL_READS.with(|r| r.borrow_mut().push(read));
                Flow::Continue
            }
        }
        struct ReaderMod;
        #[mod_api::__subscriptions]
        impl Hooks for ReaderMod {
            fn register(&self, r: &mut dyn Registrar, _: &crate::RegistrationContext) {
                let block_key = r.block_key("minecraft:glass");
                r.block_part(block_key, Reader(block_key));
            }
        }
        let mods: super::super::Mods =
            vec![ModEntry::new("signal_reader_test", Box::new(ReaderMod))].into();
        super::super::install(mods.clone());
        install_parts(&mods).unwrap();
        mark_game_thread();
    }

    /// Install a redstone connector on glass for real-game tests.
    pub fn redstone_game_thread() {
        struct RedstoneMod;
        #[mod_api::__subscriptions]
        impl Hooks for RedstoneMod {
            fn register(&self, r: &mut dyn Registrar, _: &crate::RegistrationContext) {
                let block_key = r.block_key("minecraft:glass");
                r.block_part(block_key, Connector);
            }
        }
        let mods: super::super::Mods =
            vec![ModEntry::new("redstone_test", Box::new(RedstoneMod))].into();
        super::super::install(mods.clone());
        install_parts(&mods).unwrap();
        mark_game_thread();
    }

    #[cfg(test)]
    /// The comparator reading of state `state`, from 1.
    fn read(state: u32) -> i32 {
        let mut output = 1;
        dispatch_analog_output(
            &Level,
            LevelDimension::Overworld,
            BlockPos::new(0, 64, 0),
            BlockStateId(state),
            Direction::North,
            &mut output,
        );
        output
    }

    #[cfg(test)]
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
                state_count: 5,
                state_block: &|state| match state {
                    1 => "test:gauge",
                    2 => "test:other",
                    3 => "test:connector",
                    4 => "test:press",
                    _ => "minecraft:air",
                },
            },
        )
    }

    #[test]
    fn a_block_part_answers_for_its_block_only() {
        game_thread();
        // No world is open, so no state has its parts yet: nothing answers.
        assert_eq!(read(1), 1);
        resolve(&[]).unwrap();
        // 5 more from the part: 1 + 5.
        assert_eq!(read(1), 6);
        assert_eq!(read(2), 1);
    }

    #[test]
    fn scoped_dispatch_looks_up_only_subscribed_hooks_and_parts() {
        game_thread();
        resolve(&[]).unwrap();
        let parts = super::parts("test");
        let analog = parts.for_hook(HookId::AnalogOutput).unwrap();
        let connect = parts.for_hook(HookId::ShouldRedstoneWireConnectTo).unwrap();
        assert_eq!(analog.block_state(BlockStateId(1)).len(), 1);
        assert!(connect.block_state(BlockStateId(1)).is_empty());
        assert_eq!(connect.block_state(BlockStateId(3)).len(), 1);
        assert!(analog.block_state(BlockStateId(3)).is_empty());
        // The block entity part's BlockPart override is indexed independently.
        assert_eq!(read(4), 41);
        assert!(!super::has_part_hook(HookId::ShouldCheckWeakPower));
        super::TEST_PART_LOOKUPS.set(0);
        let mut check = false;
        dispatch_should_check_weak_power(
            &Level,
            LevelDimension::Overworld,
            BlockPos::new(0, 64, 0),
            BlockStateId(1),
            Direction::East,
            &mut check,
        );
        assert_eq!(super::TEST_PART_LOOKUPS.get(), 0);
        assert!(!check);
    }

    #[test]
    fn redstone_connection_is_scoped_to_the_attached_block() {
        game_thread();
        resolve(&[]).unwrap();
        for (state, expected) in [(3, true), (2, false)] {
            let mut connected = false;
            dispatch_should_redstone_wire_connect_to(
                &Level,
                LevelDimension::Overworld,
                BlockPos::new(0, 64, 0),
                BlockStateId(state),
                Some(Direction::East),
                &mut connected,
            );
            assert_eq!(connected, expected);
        }
    }

    #[test]
    fn a_connection_is_answered_only_by_the_blocks_own_part_with_its_kind() {
        game_thread();
        resolve(&[]).unwrap();
        for kind in [
            ConnectionKind::Fence,
            ConnectionKind::Wall,
            ConnectionKind::Bars,
        ] {
            // `test:connector` answers whether the kind is a wall, from the
            // opposite answer; `test:other` has no part and keeps it.
            let answer = kind == ConnectionKind::Wall;
            for (state, expected) in [(3, answer), (2, !answer)] {
                let mut connected = !answer;
                dispatch_connects_to(
                    &Level,
                    crate::__private::ConnectsToCall {
                        dimension: LevelDimension::Overworld,
                        pos: BlockPos::new(0, 64, 0),
                        state: BlockStateId(state),
                        kind,
                        neighbour: BlockStateId(1),
                        direction: Direction::West,
                        face_solid: true,
                        connected: &mut connected,
                    },
                );
                assert_eq!(connected, expected, "{kind:?} on state {state}");
            }
        }
    }

    #[test]
    fn a_state_without_parts_is_answered_by_nothing() {
        game_thread();
        resolve(&[]).unwrap();
        // Out of range, and in range but a block no part is attached to.
        assert!(
            super::parts("analog_output")
                .for_hook(HookId::AnalogOutput)
                .unwrap()
                .block_state(BlockStateId(99))
                .is_empty()
        );
        assert!(
            super::parts("analog_output")
                .for_hook(HookId::AnalogOutput)
                .unwrap()
                .block_state(BlockStateId(0))
                .is_empty()
        );
        // A scoped hook has no global method, so both keep vanilla's answer.
        assert_eq!(read(0), 1);
        assert_eq!(read(99), 1);
    }

    #[test]
    fn each_game_answers_from_its_own_tags_and_reload_drops_what_left() {
        let mods = game_thread();
        // Both games resolve before either reads, so one shared table would fail one of them.
        let resolved = std::sync::Barrier::new(2);
        std::thread::scope(|scope| {
            let client = scope.spawn(|| {
                super::super::install(mods.clone());
                install_parts(&mods).unwrap();
                mark_game_thread();
                resolve(&["test:other"]).unwrap();
                resolved.wait();
                (read(1), read(2))
            });
            let server = scope.spawn(|| {
                super::super::install(mods.clone());
                install_parts(&mods).unwrap();
                mark_game_thread();
                resolve(&["test:gauge"]).unwrap();
                resolved.wait();
                let before = (read(1), read(2));
                // `/reload` with the tag emptied.
                resolve(&[]).unwrap();
                (before, read(1))
            });
            // The tag part comes after the key part of the same mod: 1 + 5 + 100.
            assert_eq!(client.join().unwrap(), (6, 101));
            assert_eq!(server.join().unwrap(), ((106, 1), 6));
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
                state_count: 0,
                state_block: &|_| unreachable!(),
            },
        )
        .unwrap_err();
        assert_eq!(refused.len(), 1, "{refused:?}");
        assert!(refused[0].starts_with("doubler attached "));
        assert!(refused[0].contains("Gauge to block test:gauge, which this game does not have"));
    }

    #[test]
    fn a_tag_giving_a_part_twice_keeps_the_earlier_mod() {
        let part = |mod_index| Attached::<dyn BlockPart> {
            mod_id: ["earlier", "later"][mod_index],
            mod_index,
            order: 0,
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
    fn a_level_not_in_place_asks_no_provider() {
        game_thread();
        let mut world = PipeWorld::default();
        let host: &mut dyn ServerHost = &mut world;
        // The pipe's type has a provider, which answers in the level in place; parked while
        // the server ticks another, the same block entity's level gives none.
        let parked = LevelRef::new(NativeWorldEpoch::new(1), "minecraft:the_nether");
        assert!(here().capability(host, &ITEMS, PIPE, None).is_some());
        assert!(parked.capability(host, &ITEMS, PIPE, None).is_none());
    }

    #[test]
    fn a_mod_reads_a_vanilla_chests_items_from_the_game_with_no_provider_for_it() {
        game_thread();
        let mut world = PipeWorld {
            chest: Some(stick()),
            ..PipeWorld::default()
        };
        let host: &mut dyn ServerHost = &mut world;
        // No mod registered an items provider for the chest's type: the game's hoppers have no
        // mod's handler to look for there, yet a mod reading it gets the game's own slots.
        assert!(block_entity_providers(&ITEMS, host, on(CHEST, None)).is_none());
        let chest = here().capability(host, &ITEMS, CHEST, None).unwrap();
        assert_eq!(chest.owner(), "the game");
        assert_eq!(chest.size(host), 1);
        assert_eq!(chest.resource(host, 0), Some(ItemResource::of(&stick())));
    }

    #[test]
    fn a_capability_part_answers_by_side_and_forwards_to_its_neighbours_capability() {
        game_thread();
        let mut world = PipeWorld::default();
        let host: &mut dyn ServerHost = &mut world;
        let up = here()
            .capability(host, &ITEMS, PIPE, Some(Direction::Up))
            .unwrap();
        assert_eq!(up.size(host), 0);
        let pipe = here()
            .capability(host, &ITEMS, PIPE, Some(Direction::North))
            .expect("the pipe's part");
        assert_eq!(pipe.size(host), 1);
        let stick_ = ItemResource::of(&stick());
        // The pipe calls the chest's capability, the game's own, with the host it was given:
        // in an aborted transaction the stick goes in, and comes back out.
        let aborted: Result<(), u32> =
            host.transaction(|host, tx| Err(pipe.insert(host, tx, 0, &stick_, 1)));
        assert_eq!(aborted, Err(1));
        assert!(
            backend_of(&*host)
                .and_then(|backend| backend.container_stack(on(CHEST, None), 0))
                .is_none()
        );
        let went = host.transaction(|host, tx| Ok::<_, ()>(pipe.insert(host, tx, 0, &stick_, 1)));
        assert_eq!(went, Ok(1));
        assert_eq!(
            backend_of(&*host).and_then(|backend| backend.container_stack(on(CHEST, None), 0)),
            Some(stick())
        );
        let went = host.transaction(|host, tx| Ok::<_, ()>(pipe.insert(host, tx, 0, &stick_, 1)));
        assert_eq!(went, Ok(0));
        // No block entity with slots: no answer. A mod's own capability has no game answer.
        assert!(here().capability(host, &ITEMS, [5, 64, 0], None).is_none());
        const CHEMICALS: CapabilityKey<dyn ResourceHandler<ItemResource>> =
            CapabilityKey::new("test:chemicals");
        assert!(here().capability(host, &CHEMICALS, CHEST, None).is_none());
    }

    #[cfg(test)]
    /// Puts `amount` of `item` into slot 0 of [`ITEM_ITEMS`]' handler for the item at `place`, in a
    /// transaction committed when all went in: how much did.
    fn fill(host: &mut dyn ServerHost, place: &ItemPlace, item: ItemStack, amount: u32) -> u32 {
        let handler = place
            .capability(host, &ITEM_ITEMS)
            .expect("the item's part");
        let resource = ItemResource::of(&item);
        let went =
            host.transaction(
                |host, tx| match handler.insert(host, tx, 0, &resource, amount) {
                    went if went == amount => Ok(went),
                    went => Err(went),
                },
            );
        went.unwrap_or_else(|went| went)
    }

    #[test]
    fn an_item_handler_writes_its_stack_back_where_it_lives_splitting_one_off() {
        game_thread();
        let mut player = Player {
            main_hand: Some(stack("test:pouch", 2, 0)),
            ..Player::default()
        };
        // Aborted, nothing changes.
        let host: &mut dyn ServerHost = &mut player;
        let pouch = HAND.capability(host, &ITEM_ITEMS).unwrap();
        let sticks = ItemResource::of(&stick());
        let aborted: Result<(), u32> =
            host.transaction(|host, tx| Err(pouch.insert(host, tx, 0, &sticks, 3)));
        assert_eq!(aborted, Err(3));
        assert_eq!(player.main_hand, Some(stack("test:pouch", 2, 0)));
        assert!(player.given.is_empty());
        // One of the two pouches is filled: it leaves the hand for the player, the other stays.
        let host: &mut dyn ServerHost = &mut player;
        assert_eq!(fill(host, &HAND, stick(), 3), 3);
        assert_eq!(player.main_hand, Some(stack("test:pouch", 1, 0)));
        assert_eq!(player.given, [stack("test:pouch", 1, 3)]);
        assert_eq!(player.items(), 2 + 3);
        // An item with no part and no game slots has no answer.
        player.main_hand = Some(stack("minecraft:stone", 1, 0));
        let host: &mut dyn ServerHost = &mut player;
        assert!(HAND.capability(host, &ITEM_ITEMS).is_none());
    }

    /// A mod's energy cell at `.0`: up to 1000, kept in the cell's own stack (its damage, for the
    /// test), so the handler holds nothing.
    struct Cell(ItemPlace);
    impl Cell {
        /// The cell here rewritten as holding `energy`: `moved` when that goes through, else 0.
        fn rewrite(
            &self,
            host: &mut dyn ServerHost,
            tx: &Transaction,
            energy: u32,
            moved: u32,
        ) -> u32 {
            let charged = ItemResource::of(&stack("test:cell", 1, energy));
            match self.0.exchange(host, tx, &charged, 1) {
                1 => moved,
                _ => 0,
            }
        }
    }
    impl EnergyHandler for Cell {
        fn amount(&self, host: &mut dyn ServerHost) -> u32 {
            self.0.stack(host).map_or(0, |cell| cell.damage)
        }
        fn capacity(&self, _: &mut dyn ServerHost) -> u32 {
            1000
        }
        fn insert(&self, host: &mut dyn ServerHost, tx: &Transaction, amount: u32) -> u32 {
            let held = self.amount(host);
            match amount.min(1000 - held) {
                0 => 0,
                went => self.rewrite(host, tx, held + went, went),
            }
        }
        fn extract(&self, host: &mut dyn ServerHost, tx: &Transaction, amount: u32) -> u32 {
            let held = self.amount(host);
            match amount.min(held) {
                0 => 0,
                came => self.rewrite(host, tx, held - came, came),
            }
        }
    }

    #[test]
    fn an_items_energy_moves_through_its_place_and_a_charged_one_splits_off_its_stack() {
        game_thread();
        let slot = ItemPlace::PlayerSlot { player: 1, slot: 9 };
        let mut player = Player {
            main_hand: Some(stack("test:cell", 2, 600)),
            ..Player::default()
        };
        player.slots.insert(9, stack("test:cell", 1, 900));
        let host: &mut dyn ServerHost = &mut player;
        let hand = HAND.capability(host, &ITEM_ENERGY).unwrap();
        let other = slot.capability(host, &ITEM_ENERGY).unwrap();
        // The other cell has room for 100: one of the two in the hand gives that and splits off.
        let moved = host.transaction(|host, tx| move_energy(host, &hand, &other, 400, tx));
        assert_eq!(moved, Ok(100));
        assert_eq!(player.slots[&9], stack("test:cell", 1, 1000));
        assert_eq!(player.main_hand, Some(stack("test:cell", 1, 600)));
        assert_eq!(player.given, [stack("test:cell", 1, 500)]);
        // Aborted, a move leaves both as they were.
        let host: &mut dyn ServerHost = &mut player;
        let aborted = host.transaction(|host, tx| {
            let moved = move_energy(host, &other, &hand, 400, tx);
            Err::<(), _>(moved)
        });
        assert_eq!(aborted, Err(Ok(400)));
        assert_eq!(player.slots[&9], stack("test:cell", 1, 1000));
        assert_eq!(player.main_hand, Some(stack("test:cell", 1, 600)));
        // An item no mod answers for has no energy.
        player.main_hand = Some(stick());
        let host: &mut dyn ServerHost = &mut player;
        assert!(HAND.capability(host, &ITEM_ENERGY).is_none());
    }

    #[test]
    fn a_stack_inside_an_item_and_a_mods_place_are_reached_through_their_handlers() {
        game_thread();
        let mut player = Player {
            main_hand: Some(stack("test:bag", 1, 1)),
            ..Player::default()
        };
        player.slots.insert(20 + 4, stack("test:pouch", 1, 2));
        let host: &mut dyn ServerHost = &mut player;
        // The empty pouch in the bag in the hand takes 3 sticks: the pouch is rewritten in the
        // bag, and the bag in the hand.
        let in_bag = ItemPlace::Nested {
            parent: Box::new(HAND),
            slot: 0,
        };
        assert_eq!(fill(host, &in_bag, stick(), 3), 3);
        assert_eq!(in_bag.stack(host), Some(stack("test:pouch", 1, 3)));
        assert_eq!(HAND.stack(host), Some(stack("test:bag", 1, 4)));
        // A charm slot is a mod's place, read and written by its provider.
        let charm = ItemPlace::Mod {
            provider: "test:charms",
            key: 4,
        };
        assert_eq!(fill(host, &charm, stick(), 1), 1);
        assert_eq!(charm.stack(host), Some(stack("test:pouch", 1, 3)));
        let unknown = ItemPlace::Mod {
            provider: "test:none",
            key: 4,
        };
        assert!(unknown.stack(host).is_none());
        assert_eq!(insert(host, &unknown, stick()), 0);
    }

    #[cfg(test)]
    /// `place.insert` of `stack` in a transaction of its own: how much went.
    fn insert(host: &mut dyn ServerHost, place: &ItemPlace, stack: ItemStack) -> u32 {
        let resource = ItemResource::of(&stack);
        let went = host
            .transaction(|host, tx| Ok::<_, ()>(place.insert(host, tx, &resource, stack.count)));
        went.unwrap_or(0)
    }

    #[cfg(test)]
    /// `place.exchange` of `stack.count` of what is there for `stack`, in a transaction of its
    /// own: how many.
    fn exchange(host: &mut dyn ServerHost, place: &ItemPlace, stack: ItemStack) -> u32 {
        let resource = ItemResource::of(&stack);
        let swapped = host
            .transaction(|host, tx| Ok::<_, ()>(place.exchange(host, tx, &resource, stack.count)));
        swapped.unwrap_or(0)
    }

    #[cfg(test)]
    /// Two three-slot game chests, `FROM` and `INTO`; `INTO`'s slots hold up to `limit` each.
    #[derive(Clone)]
    struct TwoChests {
        from: [Option<ItemStack>; 3],
        into: [Option<ItemStack>; 3],
        limit: u32,
        kept: Vec<Self>,
    }
    #[cfg(test)]
    const FROM: [i32; 3] = [3, 64, 0];
    #[cfg(test)]
    const INTO: [i32; 3] = [4, 64, 0];
    #[cfg(test)]
    impl TwoChests {
        #[cfg(test)]
        fn slots(&mut self, pos: [i32; 3]) -> Option<(&mut [Option<ItemStack>; 3], u32)> {
            match pos {
                FROM => Some((&mut self.from, 64)),
                INTO => Some((&mut self.into, self.limit)),
                _ => None,
            }
        }
        #[cfg(test)]
        fn count(slots: &[Option<ItemStack>; 3], item: &str) -> u32 {
            slots
                .iter()
                .flatten()
                .filter(|s| &*s.item == item)
                .map(|s| s.count)
                .sum()
        }
    }
    #[cfg(test)]
    impl Journal for TwoChests {
        fn kept(&mut self) -> &mut Vec<Self> {
            &mut self.kept
        }
    }
    #[cfg(test)]
    impl Backend for TwoChests {
        fn level_in_place(&self, level: LevelRef) -> bool {
            level == here()
        }
        fn container_size(&self, BlockSide { pos, .. }: BlockSide) -> Option<usize> {
            [FROM, INTO].contains(&pos).then_some(3)
        }
        fn container_stack(
            &self,
            BlockSide { pos, .. }: BlockSide,
            slot: usize,
        ) -> Option<ItemStack> {
            match pos {
                FROM => self.from.get(slot)?.clone(),
                INTO => self.into.get(slot)?.clone(),
                _ => None,
            }
        }
        fn container_capacity(&self, _: BlockSide, _: usize, _: &ItemStack) -> u32 {
            self.limit
        }
    }
    #[cfg(test)]
    impl BackendMut for TwoChests {
        fn container_insert(
            &mut self,
            BlockSide { pos, .. }: BlockSide,
            slot: usize,
            stack: ItemStack,
            _: &Transaction,
        ) -> Option<ItemStack> {
            let Some((slots, limit)) = self.slots(pos) else {
                return Some(stack);
            };
            let held = slots[slot].clone();
            let room = match &held {
                None => limit,
                Some(held) if held.same_item_same_components(&stack) => limit - held.count,
                Some(_) => 0,
            };
            let put = room.min(stack.count);
            if put > 0 {
                slots[slot] = Some(ItemStack {
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
            BlockSide { pos, .. }: BlockSide,
            slot: usize,
            max: u32,
            _: &Transaction,
        ) -> Option<ItemStack> {
            let (slots, _) = self.slots(pos)?;
            let held = slots[slot].as_mut()?;
            let out = ItemStack {
                count: max.min(held.count),
                ..held.clone()
            };
            held.count -= out.count;
            if held.count == 0 {
                slots[slot] = None;
            }
            Some(out)
        }
    }
    #[cfg(test)]
    impl Host for TwoChests {
        fn backend(&self) -> Option<BackendRef<'_>> {
            Some(BackendRef::of(self))
        }
        fn block_entity(&self, _: LevelRef, _: [i32; 3]) -> Option<BlockEntityRef> {
            None
        }
    }
    #[cfg(test)]
    impl ServerHost for TwoChests {
        fn transaction_open(&mut self, _: TransactionSeal) {
            open(self);
        }
        fn transaction_close(&mut self, _: TransactionSeal, commit: bool) {
            close(self, commit);
        }
        fn backend_mut(&mut self) -> Option<BackendMutRef<'_>> {
            Some(BackendMutRef::of(self))
        }
    }

    #[cfg(test)]
    /// A target that answers one more than it was given, and keeps nothing.
    struct TakesMore;
    #[cfg(test)]
    impl ResourceHandler<ItemResource> for TakesMore {
        fn size(&self, _: &mut dyn ServerHost) -> usize {
            1
        }
        fn resource(&self, _: &mut dyn ServerHost, _: usize) -> Option<ItemResource> {
            None
        }
        fn amount(&self, _: &mut dyn ServerHost, _: usize) -> u32 {
            0
        }
        fn capacity(&self, _: &mut dyn ServerHost, _: usize, _: &ItemResource) -> u32 {
            64
        }
        fn insert(
            &self,
            _: &mut dyn ServerHost,
            _: &Transaction,
            _: usize,
            _: &ItemResource,
            n: u32,
        ) -> u32 {
            n + 1
        }
        fn extract(
            &self,
            _: &mut dyn ServerHost,
            _: &Transaction,
            _: usize,
            _: &ItemResource,
            _: u32,
        ) -> u32 {
            0
        }
    }

    #[test]
    fn a_move_takes_what_the_filter_accepts_up_to_max_and_leaves_what_the_target_refuses() {
        game_thread();
        let sticks = |n| Some(stack("minecraft:stick", n, 0));
        let apples = Some(stack("minecraft:apple", 3, 0));
        let mut world = TwoChests {
            from: [sticks(5), apples, sticks(4)],
            into: [None, None, None],
            limit: 64,
            kept: Vec::new(),
        };
        let only_sticks = |r: &ItemResource| r.item() == "minecraft:stick";
        let moved = |world: &mut TwoChests, max| {
            let host: &mut dyn ServerHost = world;
            let from = here().capability(host, &ITEMS, FROM, None).unwrap();
            let to = here().capability(host, &ITEMS, INTO, None).unwrap();
            host.transaction(|host, tx| move_resources(host, &from, &to, only_sticks, max, tx))
                .unwrap()
        };
        // Up to 7 sticks: all of the first slot's, two of the third's; the apples stay.
        assert_eq!(moved(&mut world, 7), 7);
        assert_eq!(TwoChests::count(&world.from, "minecraft:stick"), 2);
        assert_eq!(TwoChests::count(&world.into, "minecraft:stick"), 7);
        assert_eq!(TwoChests::count(&world.from, "minecraft:apple"), 3);
        // A target with room for one more of the two (its other slots hold apples): that one
        // moves, the other stays put.
        world.limit = 8;
        world.into[1] = Some(stack("minecraft:apple", 1, 0));
        world.into[2] = Some(stack("minecraft:apple", 1, 0));
        assert_eq!(moved(&mut world, 64), 1);
        assert_eq!(TwoChests::count(&world.from, "minecraft:stick"), 1);
        assert_eq!(TwoChests::count(&world.into, "minecraft:stick"), 8);
        assert_eq!(moved(&mut world, 64), 0);
        assert_eq!(TwoChests::count(&world.from, "minecraft:stick"), 1);
    }

    #[test]
    fn a_move_into_a_handler_answering_more_than_it_was_given_is_undone() {
        game_thread();
        let mut world = TwoChests {
            from: [Some(stack("minecraft:stick", 5, 0)), None, None],
            into: [None, None, None],
            limit: 64,
            kept: Vec::new(),
        };
        let host: &mut dyn ServerHost = &mut world;
        let from = here().capability(host, &ITEMS, FROM, None).unwrap();
        let liar: Capability<dyn ResourceHandler<ItemResource>> = Capability {
            handler: Box::new(TakesMore),
            owner: Some("doubler"),
        };
        let moved =
            host.transaction(|host, tx| move_resources(host, &from, &liar, |_| true, 64, tx));
        let owner = "doubler".to_owned();
        assert_eq!(moved, Err(Overanswered { owner }));
        assert_eq!(world.from[0], Some(stack("minecraft:stick", 5, 0)));
        assert!(super::super::item_place::warned_about("doubler"));
    }

    #[test]
    fn a_changed_component_is_an_exchange_of_one_that_splits_it_off_and_an_abort_puts_it_back() {
        game_thread();
        let sword = |count, damage| stack("minecraft:iron_sword", count, damage);
        let mut shelf_ = shelf([Some(sword(2, 0)), None, None]);
        let host: &mut dyn ServerHost = &mut shelf_;
        let place = shelf_slot(0);
        let plain = place.resource(host).unwrap();
        let damage = ComponentId::new("minecraft:damage").unwrap();
        assert_eq!(plain.component(&damage), None);
        let worn = plain.with_component(host, &damage, Tag::Int(5));
        assert_eq!(worn.component(&damage), Some(Tag::Int(5)));
        // Aborted, nothing changed; committed, one sword of the two is worn and splits off.
        let aborted = host.transaction(|host, tx| {
            assert_eq!(place.exchange(host, tx, &worn, 1), 1);
            Err::<(), ()>(())
        });
        assert!(aborted.is_err());
        assert_eq!(shelf_.slots, [Some(sword(2, 0)), None, None]);
        let host: &mut dyn ServerHost = &mut shelf_;
        assert_eq!(exchange(host, &place, sword(1, 5)), 1);
        assert_eq!(shelf_.slots, [Some(sword(1, 0)), Some(sword(1, 5)), None]);
    }

    #[test]
    fn a_removed_default_component_is_recorded_in_the_patch_and_a_default_set_one_is_dropped() {
        game_thread();
        let shelf_ = shelf([None, None, None]);
        let host: &dyn Host = &shelf_;
        let sword = ItemResource::of(&stack("minecraft:iron_sword", 1, 0));
        let patch = |resource: &ItemResource| {
            let Some(Tag::Compound(patch)) =
                horizon_items::item_data::encode_components(&resource.to_stack(1))
            else {
                return Vec::new();
            };
            patch
                .keys()
                .map(|key| key.to_string_lossy())
                .collect::<Vec<_>>()
        };
        // An iron sword has `max_damage` by default: removing it is a removal in its patch,
        // so the sword is no longer the plain one.
        let max_damage = ComponentId::new("minecraft:max_damage").unwrap();
        let unbreakable = sword.without_component(host, &max_damage);
        assert_eq!(patch(&unbreakable), ["!minecraft:max_damage"]);
        assert_ne!(unbreakable, sword);
        assert_eq!(unbreakable.component(&max_damage), None);
        // Set again, the removal goes and the value is the patch's own.
        let set = unbreakable.with_component(host, &max_damage, Tag::Int(10));
        assert_eq!(patch(&set), ["minecraft:max_damage"]);
        assert_eq!(set.component(&max_damage), Some(Tag::Int(10)));
        // A component the item hasn't by default only loses the stack's own value.
        let data = ComponentId::new("minecraft:custom_data").unwrap();
        let tagged = sword.with_component(host, &data, Tag::Compound(Compound::new()));
        assert_ne!(tagged, sword);
        assert_eq!(tagged.without_component(host, &data), sword);
        // Set to the item's default, a component keeps no value of the stack's own, as vanilla's
        // `set` drops it: the stack is the plain item, whatever NBT type the mod wrote.
        assert_eq!(set.with_component(host, &max_damage, Tag::Int(250)), sword);
        assert_eq!(
            unbreakable.with_component(host, &max_damage, Tag::Short(250)),
            sword
        );
        let enchantable = ComponentId::new("minecraft:enchantable").unwrap();
        let value = |v| Tag::Compound([("value".into(), v)].into_iter().collect());
        assert_eq!(
            sword.with_component(host, &enchantable, value(Tag::Byte(14))),
            sword
        );
        assert_ne!(
            sword.with_component(host, &enchantable, value(Tag::Int(15))),
            sword
        );
    }

    #[test]
    fn a_component_set_keeps_a_change_its_codec_reads_and_raw_nbt_keeps_its_types() {
        /// A stick's report: `custom_data` `{x: 1}`, a huge `max_damage` and a mod's `test:long`.
        struct Report;
        impl Host for Report {
            fn item_default_component(&self, _: &str, id: &str) -> Option<Tag> {
                let x = || Tag::Compound([("x".into(), Tag::Int(1))].into_iter().collect());
                match id {
                    "minecraft:custom_data" => Some(x()),
                    "minecraft:max_damage" | "test:long" => Some(Tag::Long(1 << 53)),
                    _ => None,
                }
            }
        }
        let stick = ItemResource::of(&stick());
        let set = |id, value: Tag| {
            let id = ComponentId::new(id).unwrap();
            stick.with_component(&Report, &id, value).component(&id)
        };
        // `custom_data` is kept as the NBT given (`CustomData.equals`): a byte where the report
        // has an int is a change, while the same int is the default.
        let x = |v| Tag::Compound([("x".into(), v)].into_iter().collect());
        assert_eq!(
            set("minecraft:custom_data", x(Tag::Byte(1))),
            Some(x(Tag::Byte(1)))
        );
        assert_eq!(set("minecraft:custom_data", x(Tag::Int(1))), None);
        // Integers compare exactly, past where an `f64` keeps them apart.
        let past = Tag::Long((1 << 53) + 1);
        assert_eq!(
            set("minecraft:max_damage", past.clone()),
            Some(past.clone())
        );
        assert_eq!(set("test:long", past.clone()), Some(past));
        assert_eq!(set("minecraft:max_damage", Tag::Int(0)), Some(Tag::Int(0)));
        // A vanilla component's codec reads any integer width: the default's value is it.
        assert_eq!(set("minecraft:max_damage", Tag::Long(1 << 53)), None);
        // A mod's component, whose codec the game doesn't know, is equal type for type.
        let double = Tag::Double(((1u64 << 53) as f64).to_bits());
        assert_eq!(set("test:long", double.clone()), Some(double));
    }

    #[test]
    fn a_component_id_is_a_namespaced_identifier() {
        for id in ["minecraft:damage", "mymod:charge/level"] {
            assert_eq!(
                ComponentId::new(id).map(|id| id.as_str().to_owned()),
                Some(id.into())
            );
        }
        for id in [
            "damage",
            "!minecraft:food",
            "not a namespaced id!",
            "Mymod:charge",
            ":path",
            "mymod:",
        ] {
            assert!(ComponentId::new(id).is_none(), "{id}");
        }
    }

    #[test]
    fn a_write_that_cant_go_through_changes_nothing_and_no_item_is_made_or_lost() {
        game_thread();
        let bread = |count| stack("minecraft:bread", count, 0);
        let stone = || Some(stack("minecraft:stone", 64, 0));
        // An output slot, as a furnace's, beside full ones: its iron comes out, but the bread
        // can go nowhere, so the iron goes back.
        let iron = || Some(stack("minecraft:iron_ingot", 1, 0));
        let mut shelf_ = shelf([iron(), stone(), stone()]);
        shelf_.output = Some(0);
        assert_eq!(exchange(&mut shelf_, &shelf_slot(0), bread(1)), 0);
        assert_eq!(shelf_.slots, [iron(), stone(), stone()]);
        // One of ten apples becomes the bread, which goes into the next slot.
        let mut shelf_ = shelf([Some(stack("minecraft:apple", 10, 0)), None, None]);
        assert_eq!(exchange(&mut shelf_, &shelf_slot(0), bread(1)), 1);
        assert_eq!(
            shelf_.slots,
            [Some(stack("minecraft:apple", 9, 0)), Some(bread(1)), None]
        );
        assert_eq!(shelf_.items(), 10);

        // Two empty pouches on a full shelf: a filled one has nowhere to go, so neither changes
        // and the sticks stay out.
        let mut shelf = shelf([Some(stack("test:pouch", 2, 0)), stone(), stone()]);
        let before = shelf.items();
        assert_eq!(fill(&mut shelf, &shelf_slot(0), stick(), 3), 0);
        assert_eq!(shelf.items(), before);
        assert_eq!(shelf.slots[0], Some(stack("test:pouch", 2, 0)));
        // With a slot free, one pouch is filled there.
        shelf.slots[2] = None;
        let before = shelf.items();
        assert_eq!(fill(&mut shelf, &shelf_slot(0), stick(), 3), 3);
        assert_eq!(
            shelf.slots,
            [
                Some(stack("test:pouch", 1, 0)),
                stone(),
                Some(stack("test:pouch", 1, 3))
            ]
        );
        assert_eq!(shelf.items(), before + 3);

        // Two pouches in a charm slot, which can't split: nothing changes. So with a bag in
        // that slot holding the pouch filled.
        let mut player = Player::default();
        player.slots.insert(20 + 1, stack("test:pouch", 2, 0));
        player.slots.insert(20 + 2, stack("test:bag", 2, 1));
        let before = player.items();
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
            assert_eq!(fill(&mut player, &charm, stick(), 3), 0);
        }
        assert_eq!(player.items(), before);
        assert_eq!(player.slots[&21], stack("test:pouch", 2, 0));
        assert_eq!(player.slots[&22], stack("test:bag", 2, 1));
    }

    #[test]
    fn a_truthful_handler_moving_one_per_call_is_moved_in_passes_and_never_named() {
        game_thread();
        // A slot that gives one item per extract and takes one per insert: the 3 apples come
        // out, and the 3 bread go in, one pass each.
        let mut shelf_ = shelf([Some(stack("minecraft:apple", 3, 0)), None, None]);
        shelf_.one_per_insert = true;
        let bread = stack("minecraft:bread", 3, 0);
        assert_eq!(exchange(&mut shelf_, &shelf_slot(0), bread.clone()), 3);
        assert_eq!(shelf_.slots, [Some(bread), None, None]);
        assert!(!super::super::item_place::warned_about("the game"));
    }

    #[test]
    fn an_item_handler_answering_more_than_it_was_asked_or_offered_moves_nothing_and_is_named() {
        game_thread();
        let apples = |count| stack("minecraft:apple", count, 0);
        let bread = stack("minecraft:bread", 1, 0);
        let mut shelf_ = shelf([Some(apples(10)), None, None]);
        shelf_.whole_extract = true;
        shelf_.overstates = true;
        // Asked for one apple to swap for the bread, it takes and answers 6: the move is undone,
        // and the shelf keeps its 10.
        assert_eq!(exchange(&mut shelf_, &shelf_slot(0), bread), 0);
        assert_eq!(shelf_.slots, [Some(apples(10)), None, None]);
        // Offered 2 apples, it keeps none and answers 7 taken: undone, and the slot stays empty.
        assert_eq!(insert(&mut shelf_, &shelf_slot(1), apples(2)), 0);
        assert_eq!(shelf_.slots, [Some(apples(10)), None, None]);
        assert!(super::super::item_place::warned_about("doubler"));
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
        let in_bag = ItemPlace::Nested {
            parent: Box::new(HAND),
            slot: 0,
        };
        assert_eq!(fill(&mut player, &in_bag, stick(), 3), 3);
        // One bag leaves the hand holding the filled pouch; the other stays as it was, and no
        // pouch is loose.
        assert_eq!(player.main_hand, Some(stack("test:bag", 1, 1)));
        assert_eq!(player.given, [stack("test:bag", 1, 4)]);
        assert_eq!(player.items(), before + 3);
    }

    #[test]
    fn an_aborted_write_inside_a_nested_item_puts_every_level_back() {
        game_thread();
        // The pouch in the bag in the hand is filled in a transaction that then aborts: the
        // bag and its pouch are as they were, and nothing was given.
        let mut player = Player {
            main_hand: Some(stack("test:bag", 2, 1)),
            ..Player::default()
        };
        let in_bag = ItemPlace::Nested {
            parent: Box::new(HAND),
            slot: 0,
        };
        let host: &mut dyn ServerHost = &mut player;
        let pouch = in_bag.capability(host, &ITEM_ITEMS).unwrap();
        let sticks = ItemResource::of(&stick());
        let aborted: Result<(), u32> =
            host.transaction(|host, tx| Err(pouch.insert(host, tx, 0, &sticks, 3)));
        assert_eq!(aborted, Err(3));
        assert_eq!(player.main_hand, Some(stack("test:bag", 2, 1)));
        assert!(player.given.is_empty());
    }

    #[test]
    fn an_aborted_inner_write_of_a_handler_inside_an_item_changes_nothing() {
        game_thread();
        // A case of one diamond and one stick in the hand: its stick is taken and two put in;
        // on the way it tries emptying its diamond slot in a transaction it aborts. The
        // diamond stays, and no item is made or lost.
        let mut player = Player {
            main_hand: Some(stack("test:case", 1, 101)),
            ..Player::default()
        };
        let sticks = ItemPlace::Nested {
            parent: Box::new(HAND),
            slot: 1,
        };
        let host: &mut dyn ServerHost = &mut player;
        let resource = ItemResource::of(&stick());
        host.transaction(|host, tx| {
            assert_eq!(sticks.extract(host, tx, &resource, 1), 1);
            assert_eq!(sticks.insert(host, tx, &resource, 2), 2);
            Ok::<_, ()>(())
        })
        .unwrap();
        assert_eq!(player.main_hand, Some(stack("test:case", 1, 102)));
        assert!(player.given.is_empty());
    }

    mod deep_places {
        use super::*;

        /// An outer bag at `.0` holding one `test:bag`, kept in its damage as `Bag` keeps its
        /// pouch: 0 when empty.
        pub(super) struct Wrap(pub ItemPlace);
        impl ResourceHandler<ItemResource> for Wrap {
            fn size(&self, _: &mut dyn ServerHost) -> usize {
                1
            }
            fn resource(&self, host: &mut dyn ServerHost, _: usize) -> Option<ItemResource> {
                let n = self.0.stack(host)?.damage;
                (n > 0).then(|| ItemResource::of(&stack("test:bag", 1, n - 1)))
            }
            fn amount(&self, host: &mut dyn ServerHost, i: usize) -> u32 {
                u32::from(self.resource(host, i).is_some())
            }
            fn capacity(&self, _: &mut dyn ServerHost, _: usize, r: &ItemResource) -> u32 {
                u32::from(r.item() == "test:bag")
            }
            fn insert(
                &self,
                host: &mut dyn ServerHost,
                tx: &Transaction,
                _: usize,
                r: &ItemResource,
                n: u32,
            ) -> u32 {
                if n == 0 || r.item() != "test:bag" || self.0.stack(host).unwrap().damage != 0 {
                    return 0;
                }
                let new = ItemResource::of(&stack("test:outer", 1, r.to_stack(1).damage + 1));
                self.0.exchange(host, tx, &new, 1)
            }
            fn extract(
                &self,
                host: &mut dyn ServerHost,
                tx: &Transaction,
                i: usize,
                r: &ItemResource,
                n: u32,
            ) -> u32 {
                if n == 0 || self.resource(host, i).as_ref() != Some(r) {
                    return 0;
                }
                let new = ItemResource::of(&stack("test:outer", 1, 0));
                self.0.exchange(host, tx, &new, 1)
            }
        }

        /// A mod place that is the player's hand, and answers one more than it moved.
        pub(super) struct LyingPlace;
        impl PlaceProvider for LyingPlace {
            fn stack(&self, host: &mut dyn ServerHost, _: u64) -> Option<ItemStack> {
                HAND.stack(host)
            }
            fn insert(
                &self,
                host: &mut dyn ServerHost,
                tx: &Transaction,
                _: u64,
                r: &ItemResource,
                n: u32,
            ) -> u32 {
                HAND.insert(host, tx, r, n) + 1
            }
            fn extract(
                &self,
                host: &mut dyn ServerHost,
                tx: &Transaction,
                _: u64,
                r: &ItemResource,
                n: u32,
            ) -> u32 {
                HAND.extract(host, tx, r, n) + 1
            }
        }

        #[test]
        fn a_pouch_in_a_bag_in_a_bag_is_put_back_by_an_abort_at_any_depth() {
            game_thread();
            let in_outer = ItemPlace::Nested {
                parent: Box::new(HAND),
                slot: 0,
            };
            let in_bag = ItemPlace::Nested {
                parent: Box::new(in_outer.clone()),
                slot: 0,
            };
            for abort_depth in 0..3 {
                let initial = stack("test:outer", 1, 2);
                let mut player = Player {
                    main_hand: Some(initial.clone()),
                    ..Player::default()
                };
                let host: &mut dyn ServerHost = &mut player;
                let r = ItemResource::of(&stick());
                let result = host.transaction(|host, _| {
                    let middle = host.transaction(|host, _| {
                        let inner = host.transaction(|host, tx| {
                            let handler = in_bag.capability(host, &ITEM_ITEMS).unwrap();
                            assert_eq!(
                                handler.insert(host, tx, 0, &r, 3),
                                3,
                                "depth {abort_depth}"
                            );
                            if abort_depth == 2 { Err(()) } else { Ok(()) }
                        });
                        if abort_depth == 1 {
                            Err(())
                        } else {
                            inner.or(Ok(()))
                        }
                    });
                    if abort_depth == 0 {
                        Err(())
                    } else {
                        middle.or(Ok(()))
                    }
                });
                assert_eq!(result.is_err(), abort_depth == 0);
                assert_eq!(player.main_hand, Some(initial), "depth {abort_depth}");
                assert!(player.given.is_empty());
                // Committed, the outer bag is rewritten whole.
                let host: &mut dyn ServerHost = &mut player;
                assert_eq!(fill(host, &in_bag, stick(), 3), 3);
                assert_eq!(player.main_hand, Some(stack("test:outer", 1, 5)));
                assert!(player.given.is_empty());
            }
        }

        #[test]
        fn a_write_three_levels_down_in_one_of_two_stacked_bags_splits_that_one_off() {
            game_thread();
            let in_outer = ItemPlace::Nested {
                parent: Box::new(HAND),
                slot: 0,
            };
            let in_bag = ItemPlace::Nested {
                parent: Box::new(in_outer),
                slot: 0,
            };
            let mut player = Player {
                main_hand: Some(stack("test:outer", 2, 2)),
                ..Player::default()
            };
            assert_eq!(fill(&mut player, &in_bag, stick(), 3), 3);
            assert_eq!(player.main_hand, Some(stack("test:outer", 1, 2)));
            assert_eq!(player.given, [stack("test:outer", 1, 5)]);
        }

        #[test]
        fn an_index_less_move_goes_on_to_the_next_index() {
            game_thread();
            let mut s = shelf([
                Some(stack("minecraft:stick", 63, 0)),
                Some(stack("minecraft:stick", 63, 0)),
                None,
            ]);
            s.whole_extract = true;
            let host: &mut dyn ServerHost = &mut s;
            let h = here().capability(host, &ITEMS, SHELF, None).unwrap();
            let r = ItemResource::of(&stick());
            host.transaction(|host, tx| {
                assert_eq!(h.insert_any(host, tx, &r, 3), 3);
                assert_eq!(h.extract_any(host, tx, &r, 129), 129);
                Ok::<_, ()>(())
            })
            .unwrap();
            assert_eq!(s.slots, [None, None, None]);
        }

        #[test]
        fn a_mod_place_answering_more_than_it_was_asked_has_its_write_undone() {
            game_thread();
            let mut p = Player {
                main_hand: Some(stack("minecraft:stick", 2, 0)),
                ..Player::default()
            };
            let at = ItemPlace::Mod {
                provider: "test:lying-place",
                key: 0,
            };
            let host: &mut dyn ServerHost = &mut p;
            let r = ItemResource::of(&stick());
            host.transaction(|host, tx| {
                assert_eq!(at.insert(host, tx, &r, 1), 0);
                assert_eq!(at.extract(host, tx, &r, 1), 0);
                assert_eq!(HAND.stack(host), Some(stack("minecraft:stick", 2, 0)));
                Ok::<_, ()>(())
            })
            .unwrap();
            assert!(p.given.is_empty());
            assert!(super::super::super::item_place::warned_about("doubler"));
        }
    }

    #[test]
    fn ticket_controllers_get_the_callers_namespace_and_refuse_anothers() {
        let mut r = super::PartRegistrar::new("first");
        r.register_ticket_controller("quarry", true);
        r.register_ticket_controller("first:drill", false);
        assert!(r.refused.is_empty(), "{:?}", r.refused);
        let ids: Vec<_> = r
            .controllers
            .iter()
            .map(|c| (&*c.id, c.validates))
            .collect();
        assert_eq!(ids, [("first:quarry", true), ("first:drill", false)]);
        // Another mod's namespace, and no name at all.
        r.register_ticket_controller("second:quarry", true);
        r.register_ticket_controller("first:", true);
        r.register_ticket_controller("", true);
        assert_eq!(r.refused.len(), 3, "{:?}", r.refused);
        assert!(r.refused[0].contains("first registered ticket controller \"second:quarry\""));
        assert!(r.refused[0].contains("not its own"));
        assert!(r.refused[1].contains("no name") && r.refused[2].contains("no name"));
        assert_eq!(r.controllers.len(), 2);
    }

    #[test]
    fn a_ticket_controller_registered_twice_refuses_the_mods_naming_mod_and_id() {
        let mut r = super::PartRegistrar::new("first");
        r.register_ticket_controller("quarry", true);
        r.register_ticket_controller("first:quarry", false);
        r.mod_id = "second";
        // The same path under another mod is another id.
        r.register_ticket_controller("quarry", true);
        assert_eq!(r.refused.len(), 1, "{:?}", r.refused);
        assert_eq!(
            r.refused[0],
            "first registered ticket controller first:quarry twice"
        );
        assert_eq!(r.controllers.len(), 2);
        assert_eq!(&*r.controllers[1].id, "second:quarry");
    }

    #[test]
    fn a_place_provider_given_twice_is_refused() {
        let mut r = super::PartRegistrar::new("first");
        (&mut r as &mut dyn Registrar).place_provider("test:charms", Arc::new(Charms));
        r.mod_id = "second";
        r.mod_index = 1;
        (&mut r as &mut dyn Registrar).place_provider("test:charms", Arc::new(Charms));
        (&mut r as &mut dyn Registrar).place_provider("charms", Arc::new(Charms));
        let invalid = r.block_entity_type_key("minecraft:");
        (&mut r as &mut dyn Registrar)
            .block_entity_capability(&ITEMS, &invalid, |_, at| Some(Box::new(Pipe(at))));
        assert_eq!(r.refused.len(), 3, "{:?}", r.refused);
        assert!(
            r.refused[0]
                .contains("item places test:charms are provided twice, by first and second")
        );
        assert!(r.refused[1].contains("not a namespaced name"));
        assert!(r.refused[2].contains("invalid registry key \"minecraft:\""));
    }

    /// A mod declaring its own key under a name another mod's providers answer with another
    /// handler type is refused at load, naming both mods and both types, as is a key whose name
    /// has no namespace; the same name with the same type, from another mod, is not.
    #[test]
    fn a_capability_name_answered_with_two_handler_types_is_refused() {
        const NOT_CHEMICALS: CapabilityKey<dyn ResourceHandler<FluidResource>> =
            CapabilityKey::new("test:chemical");
        const UNNAMESPACED: CapabilityKey<dyn ResourceHandler<Chemical>> =
            CapabilityKey::new("chemical");
        let mut r = super::PartRegistrar::new("first");
        let chem_tank = r.block_entity_type_key("test:chem_tank");
        (&mut r as &mut dyn Registrar)
            .block_entity_capability(&CHEMICALS, &chem_tank, |_, at| Some(Box::new(ChemTank(at))));
        r.mod_id = "second";
        r.mod_index = 1;
        let chem_vat = r.block_entity_type_key("test:chem_vat");
        (&mut r as &mut dyn Registrar).block_entity_capability(
            &NOT_CHEMICALS,
            &chem_vat,
            |_, at| Some(Box::new(ModTank { at, lie: None })),
        );
        (&mut r as &mut dyn Registrar)
            .block_entity_capability(&CHEMICALS, &chem_vat, |_, at| Some(Box::new(ChemTank(at))));
        (&mut r as &mut dyn Registrar).block_entity_capability(
            &UNNAMESPACED,
            &chem_vat,
            |_, at| Some(Box::new(ChemTank(at))),
        );
        assert_eq!(r.refused.len(), 2, "{:?}", r.refused);
        let two_types = &r.refused[0];
        assert!(
            two_types.starts_with("capability test:chemical is answered with two handler types")
        );
        let chemicals = type_name::<dyn ResourceHandler<Chemical>>();
        let fluids = type_name::<dyn ResourceHandler<FluidResource>>();
        assert!(
            two_types.contains(&format!("{chemicals} by first")),
            "{two_types}"
        );
        assert!(
            two_types.contains(&format!("{fluids} by second")),
            "{two_types}"
        );
        assert!(r.refused[1].contains("capability \"chemical\", which is not a namespaced name"));
    }

    /// A handler that only tells which provider made it, by its slot count.
    struct Marked(usize);
    impl ResourceHandler<ItemResource> for Marked {
        fn size(&self, _: &mut dyn ServerHost) -> usize {
            self.0
        }
        fn resource(&self, _: &mut dyn ServerHost, _: usize) -> Option<ItemResource> {
            None
        }
        fn amount(&self, _: &mut dyn ServerHost, _: usize) -> u32 {
            0
        }
        fn capacity(&self, _: &mut dyn ServerHost, _: usize, _: &ItemResource) -> u32 {
            0
        }
        fn insert(
            &self,
            _: &mut dyn ServerHost,
            _: &Transaction,
            _: usize,
            _: &ItemResource,
            _: u32,
        ) -> u32 {
            0
        }
        fn extract(
            &self,
            _: &mut dyn ServerHost,
            _: &Transaction,
            _: usize,
            _: &ItemResource,
            _: u32,
        ) -> u32 {
            0
        }
    }

    #[cfg(test)]
    /// `test:ordered` block entities, where the game has slots of its own from x = 8 on.
    struct Ordered;
    #[cfg(test)]
    impl ServerHost for Ordered {}
    #[cfg(test)]
    impl Backend for Ordered {
        fn level_in_place(&self, level: LevelRef) -> bool {
            level == here()
        }
        fn block_entity_kind(&self, _: BlockEntityRef) -> Option<BlockEntityTypeKey> {
            Some(type_key("test:ordered"))
        }
        fn container_size(&self, BlockSide { pos, .. }: BlockSide) -> Option<usize> {
            (pos[0] >= 8).then_some(27)
        }
    }
    #[cfg(test)]
    impl Host for Ordered {
        fn backend(&self) -> Option<BackendRef<'_>> {
            Some(BackendRef::of(self))
        }
        fn block_entity(&self, level: LevelRef, position: [i32; 3]) -> Option<BlockEntityRef> {
            Some(BlockEntityRef { level, position })
        }
    }

    #[test]
    fn disabled_capability_providers_are_skipped_and_resume() {
        struct Later;
        #[mod_api::__subscriptions]
        impl Hooks for Later {
            fn register(&self, r: &mut dyn Registrar, _: &crate::RegistrationContext) {
                let ordered = r.block_entity_type_key("test:ordered");
                r.block_entity_capability(&ITEMS, &ordered, |_, _| Some(Box::new(Marked(99))));
                r.item_capability(&ITEM_ITEMS, "test:ordered_item", |_, _| {
                    Some(Box::new(Marked(99)))
                });
                r.entity_capability(&ENTITY_ITEMS, "test:robot", |_, _, ()| {
                    Some(Box::new(Marked(99)))
                });
            }
        }
        let mods: crate::Mods = vec![
            ModEntry::new("doubler", Box::new(Doubler)),
            ModEntry::new("later", Box::new(Later)),
        ]
        .into();
        crate::install(mods.clone());
        install_parts(&mods).unwrap();
        mark_game_thread();

        // These lookup objects keep their indexed registrations, not a snapshot of enabled mods.
        let at = BlockSide {
            level: here(),
            pos: [7, 64, 0],
            side: None,
        };
        let direct_block = || block_entity_providers(&ITEMS, &Ordered, at).unwrap();
        let robot = EntityRef::new(NativeWorldEpoch::new(1), 3);
        let mut player = Player::default();
        let direct_entity =
            || entity_providers(&ENTITY_ITEMS, &Player::default(), robot, ()).unwrap();
        let block_before_disable = direct_block();
        let entity_before_disable = direct_entity();
        let check = |without_game: Option<usize>, with_game: usize, robot_size: Option<usize>| {
            let host: &mut dyn ServerHost = &mut Ordered;
            let block = here().capability(host, &ITEMS, [7, 64, 0], None);
            assert_eq!(block.map(|found| found.size(host)), without_game);
            let block = here().capability(host, &ITEMS, [9, 64, 0], None).unwrap();
            assert_eq!(block.size(host), with_game);
            assert_eq!(
                direct_block()
                    .ask(host, false)
                    .map(|found| found.size(host)),
                without_game
            );
            assert_eq!(
                direct_block().ask(host, true).map(|found| found.size(host)),
                None
            );
            for (damage, expected) in [(0, without_game), (1, Some(with_game))] {
                let host: &mut dyn ServerHost =
                    &mut HeldItem(stack("test:ordered_item", 1, damage));
                let item = HAND.capability(host, &ITEM_ITEMS);
                assert_eq!(item.map(|found| found.size(host)), expected);
            }
            let host: &mut dyn ServerHost = &mut Player::default();
            let entity = robot.capability(host, &ENTITY_ITEMS, ());
            assert_eq!(entity.map(|found| found.size(host)), robot_size);
            assert_eq!(
                direct_entity()
                    .ask(host, false)
                    .map(|found| found.size(host)),
                robot_size
            );
        };
        // At x = 9 Doubler replaces the game's block provider, while the item with damage 1
        // uses the game's handler (27), so check that special block answer separately.
        assert_eq!(
            here()
                .capability(&mut Ordered, &ITEMS, [9, 64, 0], None)
                .unwrap()
                .size(&mut Ordered),
            3
        );
        assert!(crate::set_mod_enabled("doubler", false));
        check(Some(99), 27, Some(99));
        assert_eq!(
            block_before_disable
                .ask(&mut Ordered, false)
                .unwrap()
                .size(&mut Ordered),
            99
        );
        let entity = entity_before_disable.ask(&mut player, false).unwrap();
        assert_eq!(entity.size(&mut player), 99);
        assert!(crate::set_mod_enabled("later", false));
        check(None, 27, None);
        assert!(crate::set_mod_enabled("later", true));
        check(Some(99), 27, Some(99));
        assert!(crate::set_mod_enabled("doubler", true));
        assert_eq!(
            here()
                .capability(&mut Ordered, &ITEMS, [7, 64, 0], None)
                .unwrap()
                .size(&mut Ordered),
            1
        );
        assert_eq!(
            robot
                .capability(&mut player, &ENTITY_ITEMS, ())
                .unwrap()
                .size(&mut player),
            3
        );
        let host: &mut dyn ServerHost = &mut HeldItem(stack("test:ordered_item", 1, 0));
        assert_eq!(HAND.capability(host, &ITEM_ITEMS).unwrap().size(host), 1);
    }

    #[test]
    fn disabling_a_provider_during_lookup_does_not_skip_the_next_mod() {
        struct SwitchOff;
        #[mod_api::__subscriptions]
        impl Hooks for SwitchOff {
            fn register(&self, r: &mut dyn Registrar, _: &crate::RegistrationContext) {
                let ordered = r.block_entity_type_key("test:ordered");
                r.block_entity_capability(&ITEMS, &ordered, |_, _| {
                    assert!(crate::set_mod_enabled("switch", false));
                    None
                });
                r.block_entity_capability(&ITEMS, &ordered, |_, _| {
                    panic!("a disabled mod's later provider must not run")
                });
            }
        }
        struct Later;
        #[mod_api::__subscriptions]
        impl Hooks for Later {
            fn register(&self, r: &mut dyn Registrar, _: &crate::RegistrationContext) {
                let ordered = r.block_entity_type_key("test:ordered");
                r.block_entity_capability(&ITEMS, &ordered, |_, _| Some(Box::new(Marked(99))));
            }
        }
        let mods: crate::Mods = vec![
            ModEntry::new("switch", Box::new(SwitchOff)),
            ModEntry::new("later", Box::new(Later)),
        ]
        .into();
        crate::install(mods.clone());
        install_parts(&mods).unwrap();
        mark_game_thread();
        let found = here()
            .capability(&mut Ordered, &ITEMS, [7, 64, 0], None)
            .unwrap();
        assert_eq!(
            (found.owner(), found.size(&mut Ordered)),
            ("later".into(), 99)
        );
    }

    #[test]
    fn disabled_place_providers_refuse_reads_and_writes_and_resume() {
        game_thread();
        let mut player = Player::default();
        let charm = ItemPlace::Mod {
            provider: "test:charms",
            key: 4,
        };
        assert_eq!(
            insert(&mut player, &charm, stack("minecraft:stick", 3, 0)),
            3
        );
        assert!(crate::set_mod_enabled("chemist", false));
        assert_eq!(
            charm.amount(&mut player),
            3,
            "another mod's flag is independent"
        );
        assert!(crate::set_mod_enabled("doubler", false));
        assert!(charm.stack(&mut player).is_none());
        assert_eq!(insert(&mut player, &charm, stick()), 0);
        let resource = ItemResource::of(&stick());
        assert_eq!(
            player.transaction(|host, tx| Ok::<_, ()>(charm.extract(host, tx, &resource, 1))),
            Ok(0)
        );
        assert_eq!(
            player.slots[&24].count, 3,
            "disabled writes leave the place unchanged"
        );
        assert!(crate::set_mod_enabled("doubler", true));
        assert_eq!(charm.amount(&mut player), 3);
        assert_eq!(insert(&mut player, &charm, stick()), 1);
        assert_eq!(
            player.transaction(|host, tx| Ok::<_, ()>(charm.extract(host, tx, &resource, 1))),
            Ok(1)
        );
        assert_eq!(charm.amount(&mut player), 3);
    }

    #[test]
    fn providers_are_asked_ahead_of_the_game_then_the_game_then_the_mods_in_order() {
        game_thread();
        let host: &mut dyn ServerHost = &mut Ordered;
        let answer = |host: &mut dyn ServerHost, x| {
            let found = here().capability(host, &ITEMS, [x, 64, 0], None).unwrap();
            (found.owner.is_some(), found.size(host))
        };
        // No game slots: the first mod provider that answers; the one before it declined.
        assert_eq!(answer(host, 7), (true, 1));
        // The game's slots come before the mods' providers.
        assert_eq!(answer(host, 8), (false, 27));
        // Providers registered before the game come first where they answer, in registration
        // order: at x = 9 both answer, at x = 10 only the second.
        assert_eq!(answer(host, 9), (true, 3));
        assert_eq!(answer(host, 10), (true, 4));
        // The game's own lookup takes a mod's handler only where the game has none or a mod
        // comes first.
        let mod_only = |host: &mut dyn ServerHost, x| {
            let at = BlockSide {
                level: here(),
                pos: [x, 64, 0],
                side: None,
            };
            let game_slots = backend_of(&*host)
                .and_then(|backend| backend.container_size(at))
                .is_some();
            let providers = block_entity_providers(&ITEMS, host, at)?;
            providers
                .ask(host, game_slots)
                .map(|found| found.size(host))
        };
        assert_eq!(mod_only(host, 7), Some(1));
        assert_eq!(mod_only(host, 8), None);
        assert_eq!(mod_only(host, 9), Some(3));
        // With no provider for the block entity's type, or no type (a vanilla one), there are
        // no providers to ask, so the game needn't work out its own answer: a hopper's lookup
        // costs one table check.
        let at = BlockSide {
            level: here(),
            pos: [8, 64, 0],
            side: None,
        };
        assert!(block_entity_providers(&ITEMS, &TypeOf(None), at).is_none());
        assert!(block_entity_providers(&ITEMS, &TypeOf(Some("test:tank")), at).is_none());

        // Item providers in the same order, for the item in a player's hand: with no game slots
        // (damage 0), the first mod's that answers; with them (damage 1), the game's; for two
        // items, the one ahead of the game.
        let item = |damage, count| {
            let mut host = HeldItem(stack("test:ordered_item", count, damage));
            let host: &mut dyn ServerHost = &mut host;
            let found = HAND.capability(host, &ITEM_ITEMS).unwrap();
            (found.owner.is_some(), found.size(host))
        };
        assert_eq!(item(0, 1), (true, 1));
        assert_eq!(item(1, 1), (false, 27));
        assert_eq!(item(1, 2), (true, 3));
    }

    #[cfg(test)]
    /// Block entities of one type, or vanilla ones (none).
    struct TypeOf(Option<&'static str>);
    #[cfg(test)]
    impl Backend for TypeOf {
        fn block_entity_kind(&self, _: BlockEntityRef) -> Option<BlockEntityTypeKey> {
            self.0.map(type_key)
        }
    }
    #[cfg(test)]
    impl Host for TypeOf {
        fn backend(&self) -> Option<BackendRef<'_>> {
            Some(BackendRef::of(self))
        }
        fn block_entity(&self, level: LevelRef, position: [i32; 3]) -> Option<BlockEntityRef> {
            Some(BlockEntityRef { level, position })
        }
    }

    #[cfg(test)]
    /// Player 1 holding one stack in their hand, which has game slots (`item_container`) when
    /// its damage is 1.
    struct HeldItem(ItemStack);
    #[cfg(test)]
    impl Backend for HeldItem {
        fn item_container_slots(&self, stack: &ItemStack) -> Option<Vec<Option<ItemStack>>> {
            (stack.damage == 1).then(|| vec![None; 27])
        }
    }
    #[cfg(test)]
    impl BackendMut for HeldItem {
        fn game_stack(&mut self, place: &ItemPlace) -> Option<ItemStack> {
            (*place == HAND).then(|| self.0.clone())
        }
    }
    #[cfg(test)]
    impl Host for HeldItem {
        fn backend(&self) -> Option<BackendRef<'_>> {
            Some(BackendRef::of(self))
        }
    }
    #[cfg(test)]
    impl ServerHost for HeldItem {
        fn backend_mut(&mut self) -> Option<BackendMutRef<'_>> {
            Some(BackendMutRef::of(self))
        }
    }

    /// How a mod tank misanswers: an extract takes and answers 100 mB more than asked
    /// (`GivesMore`), or an insert answers 100 mB more than went in (`TakesMore`).
    #[derive(Clone, Copy, PartialEq)]
    enum Lie {
        GivesMore,
        TakesMore,
    }
    /// A mod's tank: the game's tank at its position, reached from every side but its top, and
    /// misanswering as `lie` says.
    struct ModTank {
        at: BlockSide,
        lie: Option<Lie>,
    }
    impl ResourceHandler<FluidResource> for ModTank {
        fn size(&self, host: &mut dyn ServerHost) -> usize {
            if self.at.side == Some(Direction::Up) {
                return 0;
            }
            crate::backend_of(host)
                .and_then(|backend| backend.fluid_tank_count(self.at))
                .unwrap_or(0)
        }
        fn resource(&self, host: &mut dyn ServerHost, index: usize) -> Option<FluidResource> {
            Some(FluidResource::of(
                &crate::backend_of(host).and_then(|backend| backend.fluid_tank(self.at, index))?,
            ))
        }
        fn amount(&self, host: &mut dyn ServerHost, index: usize) -> u32 {
            crate::backend_of(host)
                .and_then(|backend| backend.fluid_tank(self.at, index))
                .map_or(0, |held| held.amount)
        }
        fn capacity(
            &self,
            host: &mut dyn ServerHost,
            index: usize,
            resource: &FluidResource,
        ) -> u32 {
            crate::backend_of(host).map_or(0, |backend| {
                backend.fluid_tank_capacity(self.at, index, &resource.to_stack(1))
            })
        }
        fn insert(
            &self,
            host: &mut dyn ServerHost,
            tx: &Transaction,
            index: usize,
            resource: &FluidResource,
            amount: u32,
        ) -> u32 {
            let Some(backend) = crate::backend_mut_of(host) else {
                return 0;
            };
            let rest = backend.fluid_tank_insert(self.at, index, resource.to_stack(amount), tx);
            let went = amount - rest.map_or(0, |rest| rest.amount);
            match self.lie {
                Some(Lie::TakesMore) => went + 100,
                _ => went,
            }
        }
        fn extract(
            &self,
            host: &mut dyn ServerHost,
            tx: &Transaction,
            index: usize,
            resource: &FluidResource,
            amount: u32,
        ) -> u32 {
            let held =
                crate::backend_of(host).and_then(|backend| backend.fluid_tank(self.at, index));
            if !held.is_some_and(|held| resource.matches(&held)) {
                return 0;
            }
            let more = if self.lie == Some(Lie::GivesMore) {
                100
            } else {
                0
            };
            crate::backend_mut_of(host)
                .and_then(|backend| backend.fluid_tank_extract(self.at, index, amount + more, tx))
                .map_or(0, |out| out.amount)
        }
    }

    #[cfg(test)]
    /// One tank, as the game answers it: what it holds, its capacity, and whether it fills and
    /// drains. Truthfully: a `halving` tank takes half of each offer, a `half_room` one at most
    /// half its room, a `short` one gives one less than asked, and a `quarters` one gives and
    /// takes only whole quarter buckets, as a cauldron its thirds.
    #[derive(Clone)]
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
    #[cfg(test)]
    /// Game tanks by position, and the mod block entity types standing at some.
    #[derive(Clone, Default)]
    struct TankWorld {
        tanks: HashMap<[i32; 3], GameTank>,
        types: HashMap<[i32; 3], &'static str>,
        kept: Vec<Self>,
    }
    #[cfg(test)]
    const SOURCE: [i32; 3] = [0, 64, 0];
    #[cfg(test)]
    const TANK: [i32; 3] = [1, 64, 0];
    #[cfg(test)]
    const OTHER: [i32; 3] = [2, 64, 0];
    #[cfg(test)]
    impl TankWorld {
        #[cfg(test)]
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
        #[cfg(test)]
        fn held(&self, pos: [i32; 3]) -> Option<FluidStack> {
            self.tanks[&pos].held.clone()
        }
        #[cfg(test)]
        fn amount(&self, pos: [i32; 3]) -> u32 {
            self.tanks[&pos].held.as_ref().map_or(0, |held| held.amount)
        }
        #[cfg(test)]
        /// Moves up to `max` from `from`'s handler through `from_side` into `to`'s through
        /// `to_side`, in a transaction of its own.
        fn moved(
            &mut self,
            (from, from_side): ([i32; 3], Option<Direction>),
            (to, to_side): ([i32; 3], Option<Direction>),
            max: u32,
        ) -> Result<u32, Overanswered> {
            let host: &mut dyn ServerHost = self;
            let source = here().capability(host, &FLUIDS, from, from_side).unwrap();
            let target = here().capability(host, &FLUIDS, to, to_side).unwrap();
            host.transaction(|host, tx| move_resources(host, &source, &target, |_| true, max, tx))
        }
    }
    #[cfg(test)]
    impl Journal for TankWorld {
        fn kept(&mut self) -> &mut Vec<Self> {
            &mut self.kept
        }
    }
    #[cfg(test)]
    impl Backend for TankWorld {
        fn level_in_place(&self, level: LevelRef) -> bool {
            level == here()
        }
        fn block_entity_kind(
            &self,
            BlockEntityRef { position: pos, .. }: BlockEntityRef,
        ) -> Option<BlockEntityTypeKey> {
            self.types.get(&pos).map(|id| type_key(id))
        }
        fn fluid_tank_count(&self, BlockSide { pos, .. }: BlockSide) -> Option<usize> {
            self.tanks.contains_key(&pos).then_some(1)
        }
        fn fluid_tank(&self, BlockSide { pos, .. }: BlockSide, _: usize) -> Option<FluidStack> {
            self.tanks.get(&pos)?.held.clone()
        }
        fn fluid_tank_capacity(
            &self,
            BlockSide { pos, .. }: BlockSide,
            _: usize,
            _: &FluidStack,
        ) -> u32 {
            self.tanks
                .get(&pos)
                .filter(|tank| tank.fills)
                .map_or(0, |tank| tank.capacity)
        }
    }
    #[cfg(test)]
    impl BackendMut for TankWorld {
        fn fluid_tank_insert(
            &mut self,
            BlockSide { pos, .. }: BlockSide,
            _: usize,
            fluid: FluidStack,
            _: &Transaction,
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
            if add > 0 {
                tank.held = Some(fluid.with_amount(held + add));
            }
            (add < fluid.amount).then(|| fluid.with_amount(fluid.amount - add))
        }
        fn fluid_tank_extract(
            &mut self,
            BlockSide { pos, .. }: BlockSide,
            _: usize,
            max: u32,
            _: &Transaction,
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
            tank.held = (take < held.amount).then(|| held.with_amount(held.amount - take));
            Some(held.with_amount(take))
        }
    }
    #[cfg(test)]
    impl Host for TankWorld {
        fn block_entity(&self, level: LevelRef, position: [i32; 3]) -> Option<BlockEntityRef> {
            Some(BlockEntityRef { level, position })
        }
        fn backend(&self) -> Option<BackendRef<'_>> {
            Some(BackendRef::of(self))
        }
    }
    #[cfg(test)]
    impl ServerHost for TankWorld {
        fn transaction_open(&mut self, _: TransactionSeal) {
            open(self);
        }
        fn transaction_close(&mut self, _: TransactionSeal, commit: bool) {
            close(self, commit);
        }
        fn backend_mut(&mut self) -> Option<BackendMutRef<'_>> {
            Some(BackendMutRef::of(self))
        }
    }

    #[cfg(test)]
    fn potion(amount: u32, potion: &str) -> FluidStack {
        let components = horizon_items::item_data::StackData {
            potion: Some(Arc::from(potion)),
            ..Default::default()
        };
        FluidStack {
            components: Some(Box::new(components)),
            ..FluidStack::new(
                crate::registry_keys::fluid_key("test:potion").unwrap(),
                amount,
            )
        }
    }

    #[test]
    fn a_fluid_move_fills_only_with_the_same_fluid_and_components() {
        game_thread();
        let mut world = TankWorld::default();
        world.tank(SOURCE, Some(potion(1500, "minecraft:healing")), true);
        world.tank(TANK, None, true).types.insert(TANK, "test:tank");
        // The mod's tank takes nothing from above.
        let up = (TANK, Some(Direction::Up));
        let west = (TANK, Some(Direction::West));
        assert_eq!(world.moved((SOURCE, None), up, 4000), Ok(0));
        // What the tank has room for moves, and nothing more leaves the source.
        assert_eq!(world.moved((SOURCE, None), west, 4000), Ok(1000));
        assert_eq!(world.moved(west, (SOURCE, None), 400), Ok(400));
        assert_eq!(world.held(SOURCE), Some(potion(900, "minecraft:healing")));
        assert_eq!(world.held(TANK), Some(potion(600, "minecraft:healing")));
        // The same fluid with other components, or another fluid, doesn't go in.
        for other in [
            potion(500, "minecraft:poison"),
            FluidStack::new(crate::registry_keys::fluid_key("test:potion").unwrap(), 500),
        ] {
            world.tank(SOURCE, Some(other.clone()), true);
            assert_eq!(world.moved((SOURCE, None), (TANK, None), 4000), Ok(0));
            assert_eq!(world.held(SOURCE), Some(other));
            assert_eq!(world.held(TANK), Some(potion(600, "minecraft:healing")));
        }
        // A drain-only tank has no capacity, and nothing moves.
        let water = FluidStack::new(
            crate::registry_keys::fluid_key("minecraft:water").unwrap(),
            1000,
        );
        world.tank(SOURCE, Some(water.clone()), true);
        world.tank(TANK, None, false);
        let host: &mut dyn ServerHost = &mut world;
        let target = here().capability(host, &FLUIDS, TANK, None).unwrap();
        assert_eq!(target.capacity(host, 0, &FluidResource::of(&water)), 0);
        assert_eq!(world.moved((SOURCE, None), (TANK, None), 1000), Ok(0));
        assert_eq!(world.held(SOURCE), Some(water));
    }

    #[test]
    fn a_fluid_move_tries_again_with_what_the_target_took_and_loses_nothing() {
        game_thread();
        let water = |amount| {
            FluidStack::new(
                crate::registry_keys::fluid_key("minecraft:water").unwrap(),
                amount,
            )
        };
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
            // Half its room: moves of 500, 250, then 125, which quarters can't give.
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
            // One less than asked: the 999 it gives all go in.
            ("a source giving one less", |s, _| s.short = true, 999),
        ];
        for (case, setup, moves) in cases {
            let mut world = TankWorld::default();
            world.tank(SOURCE, Some(water(1000)), true);
            world.tank(TANK, None, true);
            let [source, target] = world.tanks.get_disjoint_mut([&SOURCE, &TANK]);
            setup(source.unwrap(), target.unwrap());
            let moved = world.moved((SOURCE, None), (TANK, None), 1000);
            assert_eq!(moved, Ok(moves), "{case}");
            assert_eq!(
                (world.amount(SOURCE), world.amount(TANK)),
                (1000 - moves, moves),
                "{case}"
            );
        }
    }

    #[test]
    fn a_handler_answering_more_than_it_was_asked_aborts_the_whole_move_and_loses_no_fluid() {
        game_thread();
        let water = |amount| {
            Some(FluidStack::new(
                crate::registry_keys::fluid_key("minecraft:water").unwrap(),
                amount,
            ))
        };
        for (liar, at) in [("test:gives_more", SOURCE), ("test:takes_more", TANK)] {
            let mut world = TankWorld::default();
            world.tank(SOURCE, water(1000), true);
            world.tank(OTHER, water(1000), true);
            world.tank(TANK, None, true).types.insert(at, liar);
            let host: &mut dyn ServerHost = &mut world;
            let ends = |host: &mut dyn ServerHost, pos| here().capability(host, &FLUIDS, pos, None);
            let (source, other, tank) = (
                ends(host, SOURCE).unwrap(),
                ends(host, OTHER).unwrap(),
                ends(host, TANK).unwrap(),
            );
            // Two moves into the tank in one transaction, the second from the source: the
            // first move through the liar fails, and its error aborts the transaction, so
            // every tank holds what it held.
            let moved = host.transaction(|host, tx| {
                let first = move_resources(host, &other, &tank, |_| true, 300, tx)?;
                let second = move_resources(host, &source, &tank, |_| true, 300, tx)?;
                Ok(first + second)
            });
            let owner = "doubler".to_owned();
            assert_eq!(moved, Err(Overanswered { owner }), "{liar}");
            let held = [SOURCE, OTHER, TANK].map(|pos| world.held(pos));
            assert_eq!(held, [water(1000), water(1000), None], "{liar}");
        }
    }

    /// A mod's battery: the game's at its position, misanswering by 100 as `lie` says.
    struct ModBattery {
        at: BlockSide,
        lie: Lie,
    }
    impl EnergyHandler for ModBattery {
        fn amount(&self, host: &mut dyn ServerHost) -> u32 {
            crate::backend_of(host)
                .and_then(|backend| backend.energy_amount(self.at))
                .unwrap_or(0)
        }
        fn capacity(&self, host: &mut dyn ServerHost) -> u32 {
            crate::backend_of(host).map_or(0, |backend| backend.energy_capacity(self.at))
        }
        fn insert(&self, host: &mut dyn ServerHost, tx: &Transaction, amount: u32) -> u32 {
            let went = crate::backend_mut_of(host)
                .map_or(0, |backend| backend.energy_insert(self.at, amount, tx));
            match self.lie {
                Lie::TakesMore => went + 100,
                Lie::GivesMore => went,
            }
        }
        fn extract(&self, host: &mut dyn ServerHost, tx: &Transaction, amount: u32) -> u32 {
            let more = match self.lie {
                Lie::GivesMore => 100,
                Lie::TakesMore => 0,
            };
            crate::backend_mut_of(host).map_or(0, |backend| {
                backend.energy_extract(self.at, amount + more, tx)
            })
        }
    }

    #[cfg(test)]
    /// Batteries as the game answers them, by position: what each holds, and the most one
    /// insert moves, of a capacity of 1000; and the mod block entity types standing at some.
    #[derive(Clone, Default)]
    struct EnergyWorld {
        batteries: HashMap<[i32; 3], (u32, u32)>,
        types: HashMap<[i32; 3], &'static str>,
        kept: Vec<Self>,
    }
    #[cfg(test)]
    impl EnergyWorld {
        #[cfg(test)]
        fn amounts(&self, at: &[[i32; 3]]) -> Vec<u32> {
            at.iter().map(|pos| self.batteries[pos].0).collect()
        }
    }
    #[cfg(test)]
    impl Journal for EnergyWorld {
        fn kept(&mut self) -> &mut Vec<Self> {
            &mut self.kept
        }
    }
    #[cfg(test)]
    impl Host for EnergyWorld {
        fn block_entity(&self, level: LevelRef, position: [i32; 3]) -> Option<BlockEntityRef> {
            Some(BlockEntityRef { level, position })
        }
        fn backend(&self) -> Option<BackendRef<'_>> {
            Some(BackendRef::of(self))
        }
    }
    #[cfg(test)]
    impl ServerHost for EnergyWorld {
        fn transaction_open(&mut self, _: TransactionSeal) {
            open(self);
        }
        fn transaction_close(&mut self, _: TransactionSeal, commit: bool) {
            close(self, commit);
        }
        fn backend_mut(&mut self) -> Option<BackendMutRef<'_>> {
            Some(BackendMutRef::of(self))
        }
    }
    #[cfg(test)]
    impl Backend for EnergyWorld {
        fn level_in_place(&self, level: LevelRef) -> bool {
            level == here()
        }
        fn block_entity_kind(
            &self,
            BlockEntityRef { position: pos, .. }: BlockEntityRef,
        ) -> Option<BlockEntityTypeKey> {
            self.types.get(&pos).map(|id| type_key(id))
        }
        fn energy_amount(&self, BlockSide { pos, .. }: BlockSide) -> Option<u32> {
            Some(self.batteries.get(&pos)?.0)
        }
        fn energy_capacity(&self, _: BlockSide) -> u32 {
            1000
        }
    }
    #[cfg(test)]
    impl BackendMut for EnergyWorld {
        fn energy_insert(&mut self, at: BlockSide, amount: u32, _: &Transaction) -> u32 {
            let Some((held, limit)) = self.batteries.get_mut(&at.pos) else {
                return 0;
            };
            let went = amount.min(*limit).min(1000 - *held);
            *held += went;
            went
        }
        fn energy_extract(&mut self, at: BlockSide, amount: u32, _: &Transaction) -> u32 {
            let Some((held, _)) = self.batteries.get_mut(&at.pos) else {
                return 0;
            };
            let came = amount.min(*held);
            *held -= came;
            came
        }
    }

    #[test]
    fn an_energy_move_takes_what_the_targets_room_and_limit_let_in_and_loses_none() {
        game_thread();
        let mut world = EnergyWorld::default();
        world.batteries.insert(SOURCE, (900, 1000));
        world.batteries.insert(TANK, (700, 1000));
        world.batteries.insert(OTHER, (0, 250));
        let mut moved = |from, to, max| {
            let host: &mut dyn ServerHost = &mut world;
            let source = here().capability(host, &ENERGY, from, None).unwrap();
            let target = here().capability(host, &ENERGY, to, None).unwrap();
            host.transaction(|host, tx| move_energy(host, &source, &target, max, tx))
        };
        // The target's room, then its limit per insert, each by a move tried again with what
        // it took; and no more than the source holds.
        assert_eq!(moved(SOURCE, TANK, 1000), Ok(300));
        assert_eq!(moved(SOURCE, OTHER, 1000), Ok(250));
        assert_eq!(moved(OTHER, SOURCE, 1000), Ok(250));
        assert_eq!(world.amounts(&[SOURCE, TANK, OTHER]), [600, 1000, 0]);
    }

    #[test]
    fn a_battery_answering_more_than_it_was_asked_aborts_the_whole_move_and_loses_no_energy() {
        game_thread();
        for (liar, at) in [
            ("test:battery_gives_more", SOURCE),
            ("test:battery_takes_more", TANK),
        ] {
            let mut world = EnergyWorld::default();
            world.batteries.insert(SOURCE, (500, 1000));
            world.batteries.insert(OTHER, (500, 1000));
            world.batteries.insert(TANK, (0, 1000));
            world.types.insert(at, liar);
            let host: &mut dyn ServerHost = &mut world;
            let ends = |host: &mut dyn ServerHost, pos| here().capability(host, &ENERGY, pos, None);
            let (source, other, tank) = (
                ends(host, SOURCE).unwrap(),
                ends(host, OTHER).unwrap(),
                ends(host, TANK).unwrap(),
            );
            // Two moves into the battery at `TANK` in one transaction, the second from the
            // source: a move through the liar fails, and its error aborts the transaction.
            let moved = host.transaction(|host, tx| {
                let first = move_energy(host, &other, &tank, 200, tx)?;
                let second = move_energy(host, &source, &tank, 200, tx)?;
                Ok(first + second)
            });
            let owner = "doubler".to_owned();
            assert_eq!(moved, Err(Overanswered { owner }), "{liar}");
            assert_eq!(
                world.amounts(&[SOURCE, OTHER, TANK]),
                [500, 500, 0],
                "{liar}"
            );
        }
    }

    #[test]
    fn a_mods_tag_part_registered_first_runs_before_its_key_part() {
        let mut r = super::PartRegistrar::new("registrar-test");
        (&mut r as &mut dyn Registrar).block_part_tag("test:gauged", TagGauge);
        let block_key = (&mut r as &mut dyn Registrar).block_key("test:gauge");
        (&mut r as &mut dyn Registrar).block_part(block_key, Gauge);
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
    fn a_vanilla_type_refuses_a_native_payload_part_at_registration() {
        let mut r = super::PartRegistrar::new("test");
        let kind = r.block_entity_type_key("minecraft:chest");
        assert_eq!(
            (&mut r as &mut dyn Registrar).block_entity_part(&kind, Press),
            Err(BlockEntityPartError::VanillaType { kind }),
        );
        assert!(r.parts.block_entity_kinds.is_empty());
        assert!(r.parts.blocks.is_empty());
        assert!(r.parts.block_entities.is_empty());
    }

    #[test]
    fn a_duplicate_native_part_returns_its_typed_error_and_keeps_the_first() {
        let mut r = super::PartRegistrar::new("test");
        let kind = (&mut r as &mut dyn Registrar)
            .block_entity_type("press")
            .blocks(&["test:press"])
            .build();
        (&mut r as &mut dyn Registrar)
            .block_entity_part(&kind, Press)
            .unwrap();
        assert_eq!(
            (&mut r as &mut dyn Registrar).block_entity_part(&kind, Press),
            Err(BlockEntityPartError::Duplicate {
                kind,
                part: type_name::<Press>()
            }),
        );
        assert_eq!(r.parts.block_entity_kinds["test:press"].len(), 1);
    }

    #[test]
    fn a_block_entity_part_answers_by_block_and_by_entity_type() {
        game_thread();
        let ticks = |block| {
            let mut ticks = false;
            dispatch_block_entity_ticks(&NoHost, block, &mut ticks);
            ticks
        };
        // The ticker is asked by block: the type id is not a block.
        assert!(ticks("test:press"));
        assert!(!ticks("doubler:press_entity"));
        let tick = |kind: &str| {
            let mut entity = ModBlockEntity::new(kind.into(), Default::default());
            dispatch_block_entity_tick(&mut NoHost, [0, 64, 0], &mut entity);
            entity.data.len()
        };
        // The tick is asked by the entity's type, which differs from its block's id.
        assert_eq!(tick("doubler:press_entity"), 1);
        assert_eq!(tick("test:press"), 0);
        // The view distance is asked by block, as the ticker is.
        let distance = |block| {
            let mut distance = 64.0;
            dispatch_block_entity_view_distance(&NoHost, block, &mut distance);
            distance
        };
        assert_eq!(distance("test:press"), 128.0);
        assert_eq!(distance("doubler:press_entity"), 64.0);
        assert_eq!(distance("test:other"), 64.0);
    }

    #[test]
    fn with_no_parts_the_block_entity_is_never_named() {
        let parts = Parts::default();
        let named = std::cell::Cell::new(false);
        let id = || {
            named.set(true);
            Some("test:press")
        };
        assert!(parts.block_entity(id).is_empty());
        assert!(!named.get(), "an empty table resolved the block's name");
    }

    #[test]
    fn the_same_part_twice_on_one_block_is_refused() {
        struct Other;
        #[mod_api::horizon_part]
        impl BlockPart for Other {}
        let mut r = super::PartRegistrar::new("first");
        let block_key = (&mut r as &mut dyn Registrar).block_key("test:gauge");
        (&mut r as &mut dyn Registrar).block_part(block_key, Gauge);
        r.mod_id = "second";
        r.mod_index = 1;
        let block_key = (&mut r as &mut dyn Registrar).block_key("test:gauge");
        (&mut r as &mut dyn Registrar).block_part(block_key, Other);
        let block_key = (&mut r as &mut dyn Registrar).block_key("test:gauge");
        (&mut r as &mut dyn Registrar).block_part(block_key, Gauge);
        (&mut r as &mut dyn Registrar).block_key("test:bad name");
        (&mut r as &mut dyn Registrar).block_part_tag("#test:gauged", Other);
        assert_eq!(r.parts.blocks["test:gauge"].len(), 2);
        assert_eq!(r.refused.len(), 3, "{:?}", r.refused);
        assert!(
            r.refused[0].contains("Gauge is attached to test:gauge twice, by first and second")
        );
        assert!(r.refused[1].contains("mod second: invalid registry key"));
        assert!(r.refused[2].contains("write it without #"));
    }
}
