//! Restricted views for registration and data loading.
pub use horizon_world::native_load_context::{DataLoadOrigin, LoopSide};
use std::sync::{Arc, OnceLock};

/// The permission boundary of the current owning thread. Only the game advances it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ModPhase {
    /// Package discovery, before declarations.
    Packages,
    /// Once-only declarations, without a Host.
    Registration,
    /// One side's startup, before its configs and world.
    Setup,
    /// Config loading: settings reads and scoped config files.
    Config,
    /// The owning game's normal runtime.
    World,
    /// Restricted data or asset loading, including published resource queries,
    /// returning to its entering phase.
    Reloading,
}

#[derive(Clone, Copy)]
struct PhaseState {
    phase: ModPhase,
    resume: Option<ModPhase>,
}

thread_local! {
    static PHASE: std::cell::Cell<PhaseState> = const { std::cell::Cell::new(PhaseState {
        phase: if cfg!(test) { ModPhase::World } else { ModPhase::Packages }, resume: None,
    }) };
}

/// A refused engine transition never changes the phase or dispatches a hook.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PhaseError {
    /// The token belongs to another thread.
    WrongThread,
    /// The requested edge is not part of the phase machine.
    Transition {
        /// Unchanged current phase.
        from: ModPhase,
        /// Refused destination.
        to: ModPhase,
    },
}
impl std::fmt::Display for PhaseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "mod phase transition refused: {self:?}")
    }
}
impl std::error::Error for PhaseError {}

/// Engine-only authority for one thread; moving it does not transfer authority.
#[derive(Clone)]
#[non_exhaustive]
pub struct PhaseOwner {
    thread: std::thread::ThreadId,
}
impl PhaseOwner {
    /// Bind a new owning thread (or a loader) to its initial phase. Registration
    /// is process-owned; a new side starts at Setup after those tables are frozen.
    pub fn new(initial: ModPhase) -> Self {
        PHASE.set(PhaseState {
            phase: initial,
            resume: None,
        });
        Self {
            thread: std::thread::current().id(),
        }
    }
    /// A loader's temporary thread-local phase; the calling thread's state is restored.
    pub fn scope(initial: ModPhase) -> PhaseScope {
        let previous = PHASE.with(std::cell::Cell::get);
        PhaseScope {
            owner: Self::new(initial),
            previous,
            _thread: std::marker::PhantomData,
        }
    }
    /// Advance a legal edge on the bound thread, without dispatching anything.
    pub fn advance(&self, to: ModPhase) -> Result<(), PhaseError> {
        if self.thread != std::thread::current().id() {
            return Err(PhaseError::WrongThread);
        }
        PHASE.with(|cell| {
            let mut state = cell.get();
            let from = state.phase;
            match (from, to) {
                (ModPhase::Packages, ModPhase::Registration)
                | (ModPhase::Registration, ModPhase::Setup)
                | (ModPhase::Setup, ModPhase::Config)
                | (ModPhase::Config, ModPhase::World)
                | (ModPhase::World, ModPhase::Config) => {}
                (ModPhase::Setup | ModPhase::World, ModPhase::Reloading) => {
                    state.resume = Some(from);
                }
                (ModPhase::Reloading, _) if state.resume == Some(to) => {
                    state.resume = None;
                }
                _ => return Err(PhaseError::Transition { from, to }),
            }
            state.phase = to;
            cell.set(state);
            Ok(())
        })
    }
    /// Load configs before returning to normal runtime, including on failure.
    pub fn config(&self) -> Result<PhaseGuard<'_>, PhaseError> {
        self.advance(ModPhase::Config)?;
        Ok(PhaseGuard {
            owner: self,
            resume: ModPhase::World,
            _thread: std::marker::PhantomData,
        })
    }
    /// Nest a reload in Setup or World; errors and panics restore the caller's phase.
    pub fn reloading(&self) -> Result<PhaseGuard<'_>, PhaseError> {
        let resume = current_phase();
        self.advance(ModPhase::Reloading)?;
        Ok(PhaseGuard {
            owner: self,
            resume,
            _thread: std::marker::PhantomData,
        })
    }
}

/// A temporary loader context, not authority transferred to a worker.
// exhaustive: engine-only constructor and private state
pub struct PhaseScope {
    owner: PhaseOwner,
    previous: PhaseState,
    _thread: std::marker::PhantomData<std::rc::Rc<()>>,
}
impl std::ops::Deref for PhaseScope {
    type Target = PhaseOwner;
    fn deref(&self) -> &PhaseOwner {
        &self.owner
    }
}
impl Drop for PhaseScope {
    fn drop(&mut self) {
        PHASE.set(self.previous);
    }
}

/// Engine-only phase scope, restored during unwinding too.
// exhaustive: only the engine constructs it
pub struct PhaseGuard<'a> {
    owner: &'a PhaseOwner,
    resume: ModPhase,
    _thread: std::marker::PhantomData<std::rc::Rc<()>>,
}
impl Drop for PhaseGuard<'_> {
    fn drop(&mut self) {
        self.owner
            .advance(self.resume)
            .expect("scoped phase returns to its declared destination");
    }
}

/// The phase of this thread, used in refusal diagnostics.
pub fn current_phase() -> ModPhase {
    PHASE.with(|cell| cell.get().phase)
}

/// The catalog-owned access class of a generated game operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum LoadAccess {
    /// No extra load access beyond the phase's metadata/settings exceptions.
    WorldOnly,
    /// The existing client setup reads, not general reload permissions.
    ClientSetup,
    /// Published asset/data facts, including resource-cache queries.
    PublishedResources,
}

/// One cell read at the generated API boundary, not a tick scan or registry lookup.
/// A refused mod call panics so a listener cannot ignore a Result and continue.
#[inline]
pub fn check_access(operation: &str, access: LoadAccess) {
    let phase = current_phase();
    if phase == ModPhase::World {
        return;
    }
    let metadata = matches!(
        operation,
        "on" | "is_loaded"
            | "block_key_id"
            | "item_key_id"
            | "effect_key_id"
            | "block_states_of"
            | "block_item"
            | "block_in_tag"
            | "max_stack_size"
            | "item_stack_in_tag"
            | "item_crafting_remainder"
            | "item_default_component"
            | "item_burn_time"
            | "item_compost_chance"
            | "item_cooldown_group"
            | "entity_type_in_tag"
            | "tag_members"
            | "state_outline"
            | "state_fluid_state"
            | "fluid_in_tag"
            | "fluid_is_same"
            | "fluid_bucket"
            | "fluid_pickup_sound"
    ) || operation.starts_with("block_state_");
    let settings = operation == "setting";
    let allowed = match phase {
        ModPhase::World => true,
        ModPhase::Setup => {
            settings
                || metadata
                || matches!(
                    access,
                    LoadAccess::ClientSetup | LoadAccess::PublishedResources
                )
        }
        ModPhase::Config => settings || operation == "config_files",
        ModPhase::Reloading => settings || metadata || access == LoadAccess::PublishedResources,
        ModPhase::Packages | ModPhase::Registration => false,
    };
    assert!(
        allowed,
        "mod API {operation} is unavailable during {phase:?}"
    );
}

/// Whether a package contains compiled code or content alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PackageKind {
    /// The package carries compiled code.
    Code,
    /// The package carries content alone: data and assets.
    Content,
}

/// Read-only package metadata available during registration.
#[non_exhaustive]
pub struct PackageInfo {
    pub(crate) id: String,
    pub(crate) version: String,
    pub(crate) kind: PackageKind,
    pub(crate) side: horizon_mods::Side,
    /// Its own namespace, then the others it declares.
    pub(crate) namespaces: Vec<String>,
    /// The ids it declares dependencies on.
    pub(crate) dependencies: Vec<String>,
}

impl PackageInfo {
    fn new(kind: PackageKind, manifest: &horizon_mods::Manifest) -> Self {
        Self {
            id: manifest.id.clone(),
            version: manifest.version.clone(),
            kind,
            side: manifest.side,
            namespaces: std::iter::once(&manifest.id)
                .chain(&manifest.namespaces)
                .cloned()
                .collect(),
            dependencies: manifest.dependencies.keys().cloned().collect(),
        }
    }
    /// Package identifier.
    pub fn id(&self) -> &str {
        &self.id
    }
    /// The package's version.
    pub fn version(&self) -> &str {
        &self.version
    }
    /// Whether this package contains code.
    pub fn kind(&self) -> PackageKind {
        self.kind
    }
    /// The package's declared side.
    pub fn side(&self) -> horizon_mods::Side {
        self.side
    }
    /// The package's own namespace followed by any additional namespaces.
    pub fn namespaces(&self) -> impl Iterator<Item = &str> {
        self.namespaces.iter().map(String::as_str)
    }
    /// Whether this package declares a dependency on `id`.
    pub fn depends_on(&self, id: &str) -> bool {
        self.dependencies.iter().any(|d| d == id)
    }
}

static PACKAGES: OnceLock<Arc<[PackageInfo]>> = OnceLock::new();

#[cfg(any(test, feature = "testing"))]
thread_local! {
    static TEST_PACKAGES: std::cell::RefCell<Option<Arc<[PackageInfo]>>> = const { std::cell::RefCell::new(None) };
}
/// Runs `f` with only the packages `ids` installed, for tests.
#[cfg(any(test, feature = "testing"))]
pub fn with_test_packages<R>(ids: &[&str], f: impl FnOnce() -> R) -> R {
    let packages: Vec<_> = ids
        .iter()
        .map(|id| {
            let manifest = horizon_mods::Manifest::parse(&format!(
                "id='{id}'\nname='{id}'\nversion='1'\napi='0.2'\ncapabilities=['registries']\n"
            ))
            .unwrap();
            PackageInfo::new(PackageKind::Code, &manifest)
        })
        .collect();
    TEST_PACKAGES.set(Some(packages.into()));
    let result = f();
    TEST_PACKAGES.set(None);
    result
}

fn inventory(
    native: &[horizon_mods::packages::AdjacentManifest],
    content: &[horizon_mods::selection::SelectedPackage],
) -> Vec<PackageInfo> {
    let mut packages: Vec<_> = native
        .iter()
        .map(|p| PackageInfo::new(PackageKind::Code, &p.manifest))
        .collect();
    for selected in content {
        let manifest = &selected.package.manifest;
        // A wasm package is code, selected with the content packages.
        let kind = match manifest.wasm {
            Some(_) => PackageKind::Code,
            None => PackageKind::Content,
        };
        if !packages.iter().any(|p| p.id() == manifest.id) {
            packages.push(PackageInfo::new(kind, manifest));
        }
    }
    // The code inventory follows the one installed native/wasm dispatch slice;
    // content-only packages remain after it in their selected layer order.
    packages.sort_by_key(|p| {
        crate::mods()
            .iter()
            .position(|entry| entry.id == p.id)
            .unwrap_or(usize::MAX)
    });
    packages
}

/// Records the package inventory once at startup; a second call panics.
pub fn install_packages(
    native: &[horizon_mods::packages::AdjacentManifest],
    content: &[horizon_mods::selection::SelectedPackage],
) {
    assert!(
        PACKAGES.set(inventory(native, content).into()).is_ok(),
        "package inventory already installed"
    );
}

/// Read-only view of the startup package inventory, shared: cloning it copies no package.
#[derive(Clone)]
// exhaustive: its fields are private, so mods cannot build or destructure it
pub struct LoadedMods {
    pub(crate) entries: Arc<[PackageInfo]>,
}

impl LoadedMods {
    pub(crate) fn new() -> Self {
        #[cfg(any(test, feature = "testing"))]
        if let Some(entries) = TEST_PACKAGES.with_borrow(Clone::clone) {
            return Self { entries };
        }
        Self {
            entries: PACKAGES.get().cloned().unwrap_or_else(|| Arc::new([])),
        }
    }

    /// Package IDs in dispatch order.
    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(PackageInfo::id)
    }

    /// Packages in dispatch order.
    pub fn iter(&self) -> impl Iterator<Item = &PackageInfo> {
        self.entries.iter()
    }

    /// Find a package by ID. This linear lookup is intended for registration, not hot paths.
    pub fn get(&self, id: &str) -> Option<&PackageInfo> {
        self.entries.iter().find(|entry| entry.id() == id)
    }
}

/// A registration phase's view. Declarations go on the hook's event; there is
/// no world, handle constructor, transaction or game Host at this point.
///
/// ```compile_fail
/// use mod_api::{Host, RegistrationContext};
/// fn game(_: &dyn Host) {}
/// fn registration(context: &RegistrationContext) { game(context); }
/// ```
///
/// ```compile_fail
/// use mod_api::{Host, RegistrationContext};
/// fn registration(context: &RegistrationContext) { let _ = Host::entity(context, 1); }
/// ```
///
/// ```compile_fail
/// use mod_api::{HostTransaction, RegistrationContext};
/// fn registration(context: &mut RegistrationContext) {
///     let _: Result<(), ()> = context.transaction(|_, _| Ok(()));
/// }
/// ```
// exhaustive: its fields are private, so mods cannot build or destructure it
pub struct RegistrationContext {
    pub(crate) mods: LoadedMods,
}

impl RegistrationContext {
    pub(crate) fn new() -> Self {
        Self {
            mods: LoadedMods::new(),
        }
    }

    /// The loaded-mod view, independent of this phase's event or registrar.
    pub fn mods(&self) -> &LoadedMods {
        &self.mods
    }
}

/// A data load's read-only view. The owning loader supplies its origin and
/// metadata; reading already-loaded data never creates this context.
///
/// ```compile_fail
/// use mod_api::{DataLoadContext, Host};
/// fn game(_: &dyn Host) {}
/// fn load(context: &dyn DataLoadContext) { game(context); }
/// ```
///
/// ```compile_fail
/// use mod_api::{DataLoadContext, Host};
/// fn load(context: &dyn DataLoadContext) { let _ = Host::entity(context, 1); }
/// ```
///
/// ```compile_fail
/// use mod_api::{DataLoadContext, HostTransaction};
/// fn load(context: &mut dyn DataLoadContext) {
///     let _: Result<(), ()> = context.transaction(|_, _| Ok(()));
/// }
/// ```
pub trait DataLoadContext {
    /// The phase selected at the owning load boundary.
    fn origin(&self) -> DataLoadOrigin;

    /// Registry names in ID order, from this resource generation's snapshot.
    fn registry(&self, name: &str) -> Option<Arc<[String]>>;

    /// The fluid tags this load's resources hold (`data/<ns>/tags/fluid`):
    /// each tag id to the fluids it names, in the sources' order — what a
    /// recipe's fluid ingredient resolves its `#tag`s with. `None` where the
    /// load reads no recipe resources.
    fn fluid_tags(&self) -> Option<Arc<[(String, Arc<[String]>)]>> {
        None
    }

    /// A loaded mod's setting captured at the owning load boundary, without file I/O.
    fn setting(&self, mod_id: &str, key: &str) -> Option<String>;

    /// The same loaded-mod view available during registration.
    fn mods(&self) -> LoadedMods;
}

/// The game's owning load, as a [`DataLoadContext`].
// hidden: the game's side of the context, not part of the mod surface
#[doc(hidden)]
// exhaustive: its fields are private; only the game builds it
pub struct OwningLoad<'a> {
    source: &'a horizon_world::native_load_context::LoadContext,
    mods: LoadedMods,
}

impl<'a> OwningLoad<'a> {
    pub fn new(source: &'a horizon_world::native_load_context::LoadContext) -> Self {
        Self {
            source,
            mods: LoadedMods::new(),
        }
    }
}

impl DataLoadContext for OwningLoad<'_> {
    fn origin(&self) -> DataLoadOrigin {
        self.source.origin()
    }
    fn registry(&self, name: &str) -> Option<Arc<[String]>> {
        self.source.registry(name)
    }
    fn fluid_tags(&self) -> Option<Arc<[(String, Arc<[String]>)]>> {
        Some(self.source.fluid_tags().clone())
    }
    fn setting(&self, mod_id: &str, key: &str) -> Option<String> {
        self.source.setting(mod_id, key)
    }
    fn mods(&self) -> LoadedMods {
        self.mods.clone()
    }
}

/// The view of a hook fired inside a menu, recipe or player-list loop, where no
/// game is at hand: the side, the registries, the settings and the mod list.
/// The side that owns the loop builds it once per resource generation; a hook
/// never builds one, and its answers never change while it is held.
///
/// ```compile_fail
/// use mod_api::{Host, LoopContext};
/// fn game(_: &dyn Host) {}
/// fn each(context: &dyn LoopContext) { game(context); }
/// ```
///
/// ```compile_fail
/// use mod_api::{HostTransaction, LoopContext};
/// fn each(context: &mut dyn LoopContext) {
///     let _: Result<(), ()> = context.transaction(|_, _| Ok(()));
/// }
/// ```
pub trait LoopContext {
    /// The side whose loop fires the hook.
    fn side(&self) -> LoopSide;

    /// Registry names in ID order, from the resource generation's snapshot.
    fn registry(&self, name: &str) -> Option<Arc<[String]>>;

    /// A loaded mod's setting captured when this context was built, without file I/O.
    fn setting(&self, mod_id: &str, key: &str) -> Option<String>;

    /// The same loaded-mod view available during registration.
    fn mods(&self) -> LoadedMods;
}

/// The game's loop, as a [`LoopContext`].
// hidden: the game's side of the context, not part of the mod surface
#[doc(hidden)]
// exhaustive: its fields are private; only the game builds it
pub struct OwningLoop<'a> {
    source: &'a horizon_world::native_load_context::LoopContext,
    mods: LoadedMods,
}

impl<'a> OwningLoop<'a> {
    pub fn new(source: &'a horizon_world::native_load_context::LoopContext) -> Self {
        Self {
            source,
            mods: LoadedMods::new(),
        }
    }
}

impl LoopContext for OwningLoop<'_> {
    fn side(&self) -> LoopSide {
        self.source.side()
    }
    fn registry(&self, name: &str) -> Option<Arc<[String]>> {
        self.source.registry(name)
    }
    fn setting(&self, mod_id: &str, key: &str) -> Option<String> {
        self.source.setting(mod_id, key)
    }
    fn mods(&self) -> LoadedMods {
        self.mods.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolved_dispatch_slice_keeps_order_in_registration_config_and_reload() {
        use crate::*;
        use horizon_mods::dependencies;
        use std::sync::Mutex;
        struct Trace {
            id: &'static str,
            calls: Arc<Mutex<Vec<(&'static str, &'static str)>>>,
        }
        #[crate::__subscriptions]
        impl Hooks for Trace {
            fn register(&self, _: &mut dyn Registrar, _: &RegistrationContext) {
                assert_eq!(current_phase(), ModPhase::Registration);
                self.calls.lock().unwrap().push(("register", self.id));
            }
            fn config_loading(&self, _: HostSide<'_>, _: &mut ConfigLoadingEvent<'_>) {
                assert_eq!(current_phase(), ModPhase::Config);
                self.calls.lock().unwrap().push(("config", self.id));
            }
            fn add_server_reload_listeners(
                &self,
                _: &dyn Host,
                _: &mut AddServerReloadListenersEvent<'_>,
            ) {
                assert_eq!(current_phase(), ModPhase::Reloading);
                self.calls.lock().unwrap().push(("reload", self.id));
            }
        }
        struct Server;
        impl Host for Server {}
        impl ServerHost for Server {}
        let declarations: Vec<_> = ["z", "lib", "a"].into_iter().map(|id| {
            let deps = if id == "lib" { "" } else { "[dependencies]\nlib='^1'" };
            horizon_mods::Manifest::parse(&format!("id='{id}'\nname='{id}'\nversion='1'\napi='0.2'\ncapabilities=['registries']\n{deps}")).unwrap()
        }).collect();
        let declared: Vec<_> = declarations
            .iter()
            .map(|m| dependencies::from_manifest(&m.dependencies))
            .collect();
        let inputs: Vec<_> = declarations
            .iter()
            .zip(&declared)
            .map(|(m, dependencies)| dependencies::Mod {
                id: &m.id,
                version: &m.version,
                has_api: true,
                dependencies,
            })
            .collect();
        let order = dependencies::resolve(&inputs).unwrap().order;
        let calls = Arc::new(Mutex::new(Vec::new()));
        let ids = ["z", "lib", "a"];
        let mods: Mods = order
            .into_iter()
            .map(|i| {
                ModEntry::new(
                    ids[i],
                    Box::new(Trace {
                        id: ids[i],
                        calls: calls.clone(),
                    }),
                )
            })
            .collect::<Vec<_>>()
            .into();
        crate::install(mods.clone());
        let owner = PhaseOwner::new(ModPhase::Packages);
        owner.advance(ModPhase::Registration).unwrap();
        crate::install_parts(&mods).unwrap();
        owner.advance(ModPhase::Setup).unwrap();
        let mut host = Server;
        for _world in 0..2 {
            {
                let _config = owner.config().unwrap();
                dispatch_config_loading(
                    HostSide::Server(&mut host),
                    &NativeConfigFile {
                        mod_id: "lib".into(),
                        kind: NativeConfigKind::Server,
                        file: "lib-server.toml".into(),
                        text: String::new(),
                    },
                );
            }
            for _reload in 0..2 {
                let _reload = owner.reloading().unwrap();
                dispatch_add_server_reload_listeners(&host, &mut ServerListenerRegistry::new());
            }
        }
        let expected = [
            "register", "config", "reload", "reload", "config", "reload", "reload",
        ]
        .into_iter()
        .flat_map(|moment| ["lib", "z", "a"].map(|id| (moment, id)))
        .collect::<Vec<_>>();
        assert_eq!(*calls.lock().unwrap(), expected);
    }

    #[test]
    fn phase_edges_are_exhaustive_and_refusal_leaves_state_unchanged() {
        use ModPhase::*;
        let phases = [Packages, Registration, Setup, Config, World, Reloading];
        let edges = [
            (Packages, Registration),
            (Registration, Setup),
            (Setup, Config),
            (Config, World),
            (World, Config),
            (Setup, Reloading),
            (World, Reloading),
        ];
        for from in phases {
            for to in phases {
                let owner = PhaseOwner::new(from);
                let result = owner.advance(to);
                assert_eq!(
                    result.is_ok(),
                    edges.contains(&(from, to)),
                    "{from:?} -> {to:?}"
                );
                assert_eq!(current_phase(), if result.is_ok() { to } else { from });
            }
        }
        let owner = PhaseOwner::new(Packages);
        let other = owner.clone();
        assert_eq!(
            std::thread::spawn(move || other.advance(Registration))
                .join()
                .unwrap(),
            Err(PhaseError::WrongThread)
        );
        assert_eq!(current_phase(), Packages);
    }

    #[test]
    fn reloads_restore_setup_or_world_and_world_config_can_repeat() {
        use ModPhase::*;
        let owner = PhaseOwner::new(Packages);
        owner.advance(Registration).unwrap();
        owner.advance(Setup).unwrap();
        {
            let _reload = owner.reloading().unwrap();
            assert_eq!(current_phase(), Reloading);
            assert!(owner.reloading().is_err());
        }
        assert_eq!(current_phase(), Setup);
        assert!(owner.advance(Registration).is_err());
        for _world in 0..2 {
            {
                let _config = owner.config().unwrap();
                assert_eq!(current_phase(), Config);
                check_access("setting", LoadAccess::WorldOnly);
            }
            assert_eq!(current_phase(), World);
            for _reload in 0..2 {
                let _ = std::panic::catch_unwind(|| {
                    let _reload = owner.reloading().unwrap();
                    assert_eq!(current_phase(), Reloading);
                    check_access("block_state_default", LoadAccess::WorldOnly);
                    panic!("failed prepare");
                });
                assert_eq!(current_phase(), World);
            }
        }
        {
            let _loader = PhaseOwner::scope(Setup);
            assert_eq!(current_phase(), Setup);
        }
        assert_eq!(current_phase(), World);
    }

    #[test]
    fn early_world_handle_and_transaction_calls_panic_naming_the_phase() {
        use crate::{Host, HostTransaction, ServerHost};
        struct HostDouble;
        impl Host for HostDouble {}
        impl ServerHost for HostDouble {}
        for phase in [
            ModPhase::Registration,
            ModPhase::Config,
            ModPhase::Reloading,
            ModPhase::Setup,
        ] {
            let _owner = PhaseOwner::new(phase);
            for action in [0, 1, 2, 3] {
                let panic = std::panic::catch_unwind(|| {
                    let mut host = HostDouble;
                    match action {
                        0 => {
                            let _ = host.entity(1);
                        }
                        1 => {
                            let _ = host.level("minecraft:overworld");
                        }
                        2 => {
                            let _ = host.player(1);
                        }
                        _ => {
                            let _: Result<(), ()> = host.transaction(|_, _| Ok(()));
                        }
                    }
                })
                .expect_err("early access must panic, not return a refusal");
                let message = panic.downcast_ref::<String>().unwrap();
                assert!(message.contains(&format!("{phase:?}")), "{message}");
            }
        }
        let _owner = PhaseOwner::new(ModPhase::Registration);
        let refused = std::panic::catch_unwind(|| HostDouble.block_state_id("stone")).unwrap_err();
        assert!(
            refused
                .downcast_ref::<String>()
                .unwrap()
                .contains("Registration")
        );
    }

    #[test]
    fn inventory_keeps_first_content_package_after_code_mods() {
        let native = [horizon_mods::packages::AdjacentManifest {
            path: Default::default(),
            manifest: horizon_mods::Manifest::parse(
                "id='runtime'\nname='Runtime'\nversion='3'\napi='0.2'\ncapabilities=['registries']\n",
            )
            .unwrap(),
        }];
        let package = |version: &str| {
            let manifest = horizon_mods::Manifest::parse(&format!(
                "id='plain'\nname='Plain'\nversion='{version}'\napi='0.2'\ncapabilities=['registries']\n"
            )).unwrap();
            horizon_mods::selection::SelectedPackage {
                path: Default::default(),
                package: horizon_mods::Package {
                    manifest,
                    icon: None,
                },
            }
        };
        let entries = inventory(&native, &[package("1"), package("2")]);
        let view = LoadedMods {
            entries: entries.into(),
        };
        assert_eq!(view.ids().collect::<Vec<_>>(), ["runtime", "plain"]);
        let runtime = view.get("runtime").unwrap();
        assert_eq!(runtime.kind(), PackageKind::Code);
        assert_eq!(runtime.version(), "3");
        assert_eq!(runtime.side(), horizon_mods::Side::Client);
        let plain = view.get("plain").unwrap();
        assert_eq!(plain.kind(), PackageKind::Content);
        assert_eq!(plain.version(), "1");
        assert_eq!(plain.side(), horizon_mods::Side::Client);
        assert_eq!(plain.namespaces().collect::<Vec<_>>(), ["plain"]);
        assert!(!plain.depends_on("runtime"));
        assert!(view.get("missing").is_none());
    }

    #[test]
    fn a_loop_context_reads_its_side_registries_and_mods() {
        use horizon_world::native_load_context::{DataLoadOrigin, LoadContext, LoadInputs};
        let registries =
            std::collections::BTreeMap::from([("item".to_owned(), vec!["a:b".into()])]);
        let load = LoadContext::new(
            DataLoadOrigin::ClientPreload,
            std::sync::Arc::new(LoadInputs::new(registries, Default::default())),
            [],
        );
        let source = load.loop_context(LoopSide::Client, []);
        with_test_packages(&["one", "two"], || {
            let context = OwningLoop::new(&source);
            assert_eq!(context.side(), LoopSide::Client);
            assert_eq!(
                context.registry("item").as_deref(),
                Some(&["a:b".to_owned()][..])
            );
            assert_eq!(context.registry("block"), None);
            assert_eq!(context.setting("one", "mode"), None);
            assert_eq!(context.mods().ids().collect::<Vec<_>>(), ["one", "two"]);
        });
    }
}
