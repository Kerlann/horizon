//! Shared table state, cross-kind registration and common test fixtures only.
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
    // hidden: the game's goal registration reads these (#1077)
    #[doc(hidden)]
    pub fn mod_id(&self) -> &'static str {
        self.mod_id
    }
    // hidden: the game's goal registration reads these (#1077)
    #[doc(hidden)]
    pub fn type_name(&self) -> &'static str {
        self.type_name
    }
    // hidden: the game's goal registration reads these (#1077)
    #[doc(hidden)]
    pub fn part(&self) -> &Arc<P> {
        &self.part
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
    key_mappings: Vec<(&'static str, super::NativeKeyRegistration)>,
    /// Block parts, and block entity parts as the block parts they also are.
    blocks: HashMap<Box<str>, Vec<Attached<dyn BlockPart>>>,
    /// Block entity parts by block, for the hooks asked by block (`block_entity`, `screen`).
    block_entities: HashMap<Box<str>, Vec<Attached<dyn BlockEntityPart>>>,
    /// Block entity parts by block entity type, for the hooks asked by the entity
    /// (`block_entity_tick`, `menu_button`): its type id need not be its block's id.
    block_entity_kinds: HashMap<Box<str>, Vec<Attached<dyn BlockEntityPart>>>,
    items: HashMap<Box<str>, Vec<Attached<dyn ItemPart>>>,
    menus: HashMap<Box<str>, Vec<Attached<dyn MenuPart>>>,
    /// Entity parts by entity type id (`minecraft:zombie`), for the goal
    /// registration at every `Mob` construction (#1077). They carry no scoped
    /// hook, so they never enter `hooks`.
    entities: HashMap<Box<str>, Vec<Attached<dyn EntityPart>>>,
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
    /// The tooltip appenders, in registration order.
    tooltip_appenders: Vec<TooltipAppenderPart>,
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
    game_rule_categories: Vec<String>,
    game_rules: Vec<horizon_client_core::gamerules::Rule>,
    game_rule_slots: Vec<u32>,
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

/// Refusals at the Parts load boundary.
#[derive(Debug)]
#[non_exhaustive]
pub enum ResolvePartsError {
    /// An item key names no loaded item.
    ItemKeys(Vec<ItemKeyError>),
    /// Legacy block or item Part registration refusals.
    Parts(Vec<String>),
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
) -> Result<(), ResolvePartsError> {
    let Some(installed) = installed() else {
        return Ok(());
    };
    if let Err(errors) = installed.registry_keys.items.resolve(
        |id| (known.item)(id).then_some(()),
        |mod_id, id| ItemKeyError {
            mod_id,
            id: id.to_owned(),
        },
    ) {
        return Err(ResolvePartsError::ItemKeys(errors));
    }
    let mut unknown: Vec<String> = unknown_keys(&installed.keys.blocks, known.block, "block")
        .chain(unknown_keys(&installed.keys.items, known.item, "item"))
        .collect();
    if !unknown.is_empty() {
        unknown.sort();
        return Err(ResolvePartsError::Parts(unknown));
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

/// The game's [`Registrar`]: every mod's registrations, checked as they come.
// exhaustive: its fields are private, so mods cannot build or destructure it
pub struct PartRegistrar {
    game_rule_categories: Vec<String>,
    game_rules: Vec<horizon_client_core::gamerules::Rule>,
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
            game_rule_categories: Vec::new(),
            game_rules: Vec::new(),
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

/// Builds the parts from every mod's [`Hooks::register`](super::Hooks::register), in dispatch order. Once, at startup;
/// the error lists every part refused, naming its mods.
pub fn install_parts(mods: &[ModEntry]) -> Result<(), Vec<String>> {
    install_registrar(register_all(mods))
}

fn install_registrar(mut registrar: PartRegistrar) -> Result<(), Vec<String>> {
    let registry = horizon_client_core::gamerules::RuleRegistry::with_bool_rules(
        &registrar.game_rule_categories,
        &registrar.game_rules,
    )
    .map_err(|errors| {
        registrar.refused.extend(
            errors
                .into_iter()
                .map(|error| {
                    let owner = match &error {
                        horizon_client_core::gamerules::RuleRegistryError::UnknownCategory {
                            rule,
                            ..
                        }
                        | horizon_client_core::gamerules::RuleRegistryError::Duplicate(rule) => {
                            rule.split_once(':').map_or("unknown", |(owner, _)| owner)
                        }
                    };
                    format!("mod {owner}: {error:?}")
                })
                .collect::<Vec<_>>(),
        );
        registrar.refused.clone()
    })?;
    if !registrar.refused.is_empty() {
        return Err(registrar.refused);
    }
    let game_rule_slots = registrar
        .game_rules
        .iter()
        .map(|rule| registry.position(&rule.name).unwrap())
        .collect();
    let mut installed = Installed {
        game_rule_slots,
        game_rules: registrar.game_rules,
        game_rule_categories: registrar.game_rule_categories,
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
            Some(crate::__private::game::block_entity_ref(level, position, 1))
        }
    }

    /// How a mod tank misanswers: an extract takes and answers 100 mB more than asked
    /// (`GivesMore`), or an insert answers 100 mB more than went in (`TakesMore`).
    #[derive(Clone, Copy, PartialEq)]
    enum Lie {
        GivesMore,
        TakesMore,
    }
}
