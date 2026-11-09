//! The runtime for wasm mods. Packages declaring `[wasm]` are grouped by their `[dependencies]`
//! (the same model as native mods: required and optional, semver on `horizon-mod.toml`
//! versions, one library copy shared by its dependents). Every refusal names its reason for the
//! Mods page.
pub mod link;
mod points;
pub use points::InternalsCatalog;
mod runtime;
#[allow(
    dead_code,
    unused_variables,
    unused_mut,
    unsafe_op_in_unsafe_fn,
    clippy::all
)]
pub(crate) mod boundary {
    include!(concat!(env!("OUT_DIR"), "/wire_host_generated.rs"));
}
pub(crate) use runtime::WeakWasmMod;
pub use runtime::{CallClass, WasmHandler, WasmMod};

pub(crate) trait NoAnswer {
    fn no_answer() -> Self;
}
impl<T: Default> NoAnswer for T {
    fn no_answer() -> Self {
        T::default()
    }
}
pub(crate) fn call_refused<E: From<(mod_api::Identifier, String)>>() -> E {
    E::from((
        mod_api::Identifier::parse("horizon:wasm_mod").expect("valid constant identifier"),
        "the mod's call didn't run".into(),
    ))
}

use horizon_mod_package::dependencies;
use horizon_mod_package::manifest::Manifest;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// One wasm mod as installed: its manifest and its checked `mod.wasm`.
pub struct WasmPackage {
    pub manifest: Manifest,
    pub module: Vec<u8>,
}

/// Why a wasm mod doesn't load, for the Mods page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// A required dependency isn't installed, or none installed matches its version.
    MissingDependency { dependency: String, version: String },
    /// The shared native/wasm resolver refused the dependency graph.
    DependencyOrder { reason: String },
    /// A dependency's installed `mod.wasm` isn't the one this mod was linked against.
    LinkedAgainst { dependency: String },
    /// Members of one group were built with different toolchains.
    Toolchain { found: String, group: String },
    /// A dependency of this mod was refused, so its group can't link.
    DependencyRefused { dependency: String },
    /// The mod's declared game internals do not resolve against this game's catalog.
    Internals(mod_points::endpoint_policy::InternalsRefusal),
    /// The package's `mod.wasm` failed its checks.
    Package(String),
    /// The group could not instantiate, initialize or run.
    Runtime(String),
    /// The mod was compiled against another generated boundary.
    Layout { built: u64, game: u64 },
    /// The mod's generated point records differ from this game's.
    PointLayout { built: u64, game: u64 },
    /// The game has no shared Rust standard library beside its executable.
    NoStd,
    /// A running group trapped or exhausted a resource limit.
    Stopped,
    /// Another thread owns a group whose import is running native code.
    Busy,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingDependency {
                dependency,
                version,
            } => {
                write!(f, "needs {dependency} {version}, which isn't installed")
            }
            Self::DependencyOrder { reason } => write!(f, "{reason}"),
            Self::LinkedAgainst { dependency } => {
                write!(f, "built against another build of {dependency}")
            }
            Self::Toolchain { found, group } => {
                write!(f, "built with {found}, its dependencies with {group}")
            }
            Self::DependencyRefused { dependency } => {
                write!(f, "its dependency {dependency} didn't load")
            }
            Self::Internals(why) => write!(f, "{why}"),
            Self::Package(why) => write!(f, "{why}"),
            Self::Runtime(why) => write!(f, "{why}"),
            Self::Layout { .. } => write!(f, "built for another version of the mod API"),
            Self::PointLayout { .. } => {
                write!(f, "built for another version of the mod points API")
            }
            Self::NoStd => write!(f, "this game has no wasm standard library member"),
            Self::Stopped => write!(f, "stopped after a trap or resource limit"),
            Self::Busy => write!(f, "its wasm group is busy"),
        }
    }
}

impl std::error::Error for Refusal {}

/// Splits the packages into groups: each mod with everything it depends on, transitively;
/// two mods sharing a dependency share its group, so a library has one copy. A mod whose
/// required dependency is missing, or whose linked hashes or toolchain disagree, is refused
/// with every mod depending on it.
fn groups(packages: &[WasmPackage]) -> (Vec<Vec<usize>>, Vec<(String, Refusal)>) {
    let by_id: BTreeMap<&str, usize> = packages
        .iter()
        .enumerate()
        .map(|(i, p)| (p.manifest.id.as_str(), i))
        .collect();
    let mut refused: BTreeMap<usize, Refusal> = BTreeMap::new();
    let declared: Vec<_> = packages
        .iter()
        .map(|p| dependencies::from_manifest(&p.manifest.dependencies))
        .collect();
    // Refuse exactly the resolver's owners, then their dependents, preserving
    // unrelated libraries. There is no second semver or topological rule here.
    let order = loop {
        let active: Vec<_> = (0..packages.len())
            .filter(|i| !refused.contains_key(i))
            .collect();
        let inputs: Vec<_> = active
            .iter()
            .map(|&i| dependencies::Mod {
                id: &packages[i].manifest.id,
                version: &packages[i].manifest.version,
                has_api: true,
                dependencies: &declared[i],
            })
            .collect();
        match dependencies::resolve(&inputs) {
            Ok(resolution) => {
                break resolution
                    .order
                    .into_iter()
                    .map(|i| active[i])
                    .collect::<Vec<_>>();
            }
            Err(error) => {
                let reason = error.to_string();
                let owners: Vec<&str> = match &error {
                    dependencies::Error::Cycle { mods } => {
                        mods.iter().map(String::as_str).collect()
                    }
                    dependencies::Error::Duplicate(id) => vec![id],
                    dependencies::Error::Manifest { owner, .. }
                    | dependencies::Error::Missing { owner, .. }
                    | dependencies::Error::NoApi { owner, .. }
                    | dependencies::Error::CargoConflict { owner, .. }
                    | dependencies::Error::Version { owner, .. } => vec![owner],
                };
                for &i in &active {
                    if owners.contains(&packages[i].manifest.id.as_str()) {
                        let refusal = match &error {
                            dependencies::Error::Missing { target, .. }
                            | dependencies::Error::Version { target, .. } => {
                                Refusal::MissingDependency {
                                    dependency: target.clone(),
                                    version: declared[i]
                                        .iter()
                                        .find(|d| d.id == *target)
                                        .unwrap()
                                        .requirement
                                        .to_string(),
                                }
                            }
                            _ => Refusal::DependencyOrder {
                                reason: reason.clone(),
                            },
                        };
                        refused.insert(i, refusal);
                    }
                }
                propagate_refusals(packages, &by_id, &mut refused);
            }
        }
    };

    for (i, package) in packages.iter().enumerate() {
        let wasm = package
            .manifest
            .wasm
            .as_ref()
            .expect("a wasm package declares [wasm]");
        for id in package.manifest.dependencies.keys() {
            let Some(&d) = by_id.get(id.as_str()) else {
                continue;
            };
            let dep = packages[d]
                .manifest
                .wasm
                .as_ref()
                .expect("a wasm package declares [wasm]");
            if wasm
                .linked()
                .get(id)
                .is_some_and(|hash| hash != dep.sha512())
            {
                refused.entry(i).or_insert(Refusal::LinkedAgainst {
                    dependency: id.clone(),
                });
            }
            if dep.toolchain() != wasm.toolchain() {
                refused.entry(i).or_insert(Refusal::Toolchain {
                    found: wasm.toolchain().into(),
                    group: dep.toolchain().into(),
                });
            }
        }
    }
    propagate_refusals(packages, &by_id, &mut refused);
    // Union-find over the dependency edges of the mods that stay.
    let mut parent: Vec<usize> = (0..packages.len()).collect();
    fn root(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    for (i, package) in packages
        .iter()
        .enumerate()
        .filter(|(i, _)| !refused.contains_key(i))
    {
        for id in package.manifest.dependencies.keys() {
            if let Some(&d) = by_id.get(id.as_str()).filter(|d| !refused.contains_key(d)) {
                let (a, b) = (root(&mut parent, i), root(&mut parent, d));
                parent[a] = b;
            }
        }
    }
    let mut by_root: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for i in (0..packages.len()).filter(|i| !refused.contains_key(i)) {
        by_root.entry(root(&mut parent, i)).or_default().push(i);
    }
    let groups = by_root
        .into_values()
        .map(|members| {
            order
                .iter()
                .copied()
                .filter(|i| members.contains(i))
                .collect()
        })
        .collect();
    let refused = refused
        .into_iter()
        .map(|(i, r)| (packages[i].manifest.id.clone(), r))
        .collect();
    (groups, refused)
}

/// A refused mod takes its dependents with it, until nothing changes.
fn propagate_refusals(
    packages: &[WasmPackage],
    by_id: &BTreeMap<&str, usize>,
    refused: &mut BTreeMap<usize, Refusal>,
) {
    loop {
        let before = refused.len();
        for (i, package) in packages.iter().enumerate() {
            if refused.contains_key(&i) {
                continue;
            }
            if let Some(id) = package.manifest.dependencies.keys().find(|id| {
                by_id
                    .get(id.as_str())
                    .is_some_and(|d| refused.contains_key(d))
            }) {
                refused.insert(
                    i,
                    Refusal::DependencyRefused {
                        dependency: id.clone(),
                    },
                );
            }
        }
        if refused.len() == before {
            break;
        }
    }
}

static REFUSED: OnceLock<Vec<(String, Refusal)>> = OnceLock::new();
static INITIALIZED_REFUSALS: OnceLock<Vec<(String, Refusal)>> = OnceLock::new();

/// The wasm mods refused at startup, each with its reason, for the Mods page.
pub fn refused() -> &'static [(String, Refusal)] {
    INITIALIZED_REFUSALS
        .get()
        .or_else(|| REFUSED.get())
        .map_or(&[], Vec::as_slice)
}

/// The current startup refusal or live trap/limit status, for a Mods-page entry.
pub fn refusal(id: &str) -> Option<Refusal> {
    refused()
        .iter()
        .find(|(owner, _)| owner == id)
        .map(|(_, why)| why.clone())
        .or_else(|| {
            MODULES
                .get()?
                .iter()
                .find(|(manifest, _)| manifest.id == id)
                .and_then(|(_, module)| module.is_stopped().then_some(Refusal::Stopped))
        })
}

/// Loads the wasm mods the player turned on in `game_dir` (and every `dev` package), once at
/// startup: the same selection as content mods (`horizon_mod_package::selection`), so a wasm mod's
/// `assets/` and `data/` load as content beside its code. Refusals are kept for [`refused`].
pub fn load_installed(game_dir: &Path, dev: &[PathBuf]) -> Loaded {
    let enabled = horizon_mod_package::mod_list::ModList::load(game_dir).enabled;
    let mut refused = Vec::new();
    let packages: Vec<WasmPackage> =
        horizon_mod_package::selection::content_packages(game_dir, &enabled, dev)
            .filter(|selected| selected.package.manifest.wasm.is_some())
            .filter_map(
                |selected| match horizon_mod_package::Package::read_wasm(&selected.path) {
                    Ok(module) => Some(WasmPackage {
                        manifest: selected.package.manifest,
                        module,
                    }),
                    Err(why) => {
                        refused.push((selected.package.manifest.id, Refusal::Package(why)));
                        None
                    }
                },
            )
            .collect();
    let mut loaded = match std::env::current_exe()
        .ok()
        .and_then(|exe| std::fs::read(exe.with_file_name("horizon-wasm-std.wasm")).ok())
    {
        Some(std) => load(&std, packages),
        None => Loaded {
            modules: Vec::new(),
            mods: Vec::new(),
            manifests: Vec::new(),
            refused: packages
                .into_iter()
                .map(|p| (p.manifest.id, Refusal::NoStd))
                .collect(),
            catalog: None,
        },
    };
    refused.append(&mut loaded.refused);
    for (id, why) in &refused {
        log::warn!("wasm mod {id} refused: {why}");
    }
    let _ = REFUSED.set(refused.clone());
    loaded.refused = refused;
    let _ = MODULES.set(
        loaded
            .manifests
            .iter()
            .cloned()
            .zip(loaded.modules.iter().cloned())
            .collect(),
    );
    loaded
}

/// Loaded wasm entries are installed in the same dispatch list as native entries.
#[derive(Default)]
pub struct Loaded {
    pub mods: Vec<mod_api::ModEntry>,
    pub manifests: Vec<Manifest>,
    pub refused: Vec<(String, Refusal)>,
    modules: Vec<WasmMod>,
    catalog: Option<InternalsCatalog>,
}

impl Loaded {
    /// Supplies the game's internals catalog for `initialize`, which resolves each mod's
    /// declared internals against it. Without one, a mod declaring internals is refused.
    pub fn with_catalog(mut self, catalog: InternalsCatalog) -> Self {
        self.catalog = Some(catalog);
        self
    }

    /// Initializes programmatically loaded mods with the final inventory, then installs
    /// their point listeners. Call once, before sealing the point registry.
    pub fn initialize(
        &self,
        inventory: &[mod_api::PackageInfo],
    ) -> Result<(), Vec<(String, Refusal)>> {
        let refused = initialize_selected(
            self.manifests.iter().zip(&self.modules),
            inventory,
            self.catalog.as_ref(),
        );
        if refused.is_empty() {
            Ok(())
        } else {
            Err(refused)
        }
    }
}

static CATALOG: OnceLock<InternalsCatalog> = OnceLock::new();

/// Installs the game's internals catalog for the process-wide `initialize`, once, before it.
pub fn install_catalog(catalog: InternalsCatalog) {
    let _ = CATALOG.set(catalog);
}

static MODULES: OnceLock<Vec<(Manifest, WasmMod)>> = OnceLock::new();

/// Supplies the complete native/content inventory after the game's dispatch order is installed,
/// before registration. Constructors and subscription queries run before this, as on native.
pub fn initialize(inventory: &[mod_api::PackageInfo]) {
    let mut refused = REFUSED.get().cloned().unwrap_or_default();
    let modules = MODULES.get().map_or(&[][..], Vec::as_slice);
    refused.extend(initialize_selected(
        modules.iter().map(|(manifest, module)| (manifest, module)),
        inventory,
        CATALOG.get(),
    ));
    let _ = INITIALIZED_REFUSALS.set(refused);
}
fn initialize_selected<'a>(
    modules: impl Iterator<Item = (&'a Manifest, &'a WasmMod)>,
    inventory: &[mod_api::PackageInfo],
    catalog: Option<&InternalsCatalog>,
) -> Vec<(String, Refusal)> {
    let mut refused = Vec::new();
    for (manifest, module) in modules {
        let result = if initialize_modules(std::slice::from_ref(module), inventory) {
            points::listen(manifest, module, catalog)
        } else {
            Err(Refusal::Runtime("initialization failed".into()))
        };
        if let Err(why) = result {
            module.disable();
            log::error!("wasm mod {} refused: {why}", manifest.id);
            refused.push((manifest.id.clone(), why));
        }
    }
    refused
}
fn initialize_modules(modules: &[WasmMod], inventory: &[mod_api::PackageInfo]) -> bool {
    let mut succeeded = true;
    for module in modules {
        succeeded &= module
            .call(
                CallClass::Load,
                boundary::Frame::default(),
                mod_api::wire::INIT,
                None,
                |w| w.put(inventory),
                |_| Ok(()),
            )
            .is_some();
    }
    succeeded
}

pub fn load(std_member: &[u8], packages: Vec<WasmPackage>) -> Loaded {
    let (groups, refused) = groups(&packages);
    let mut loaded = Loaded {
        refused,
        ..Loaded::default()
    };
    let engine = match runtime::engine() {
        Ok(engine) => engine,
        Err(error) => {
            loaded.refused.extend(
                packages
                    .iter()
                    .map(|p| (p.manifest.id.clone(), Refusal::Runtime(error.to_string()))),
            );
            return loaded;
        }
    };
    for indices in groups {
        let mut members = vec![link::Member {
            name: "libstd.so",
            module: std_member,
        }];
        let names: Vec<_> = indices
            .iter()
            .map(|&i| format!("lib{}.so", packages[i].manifest.id))
            .collect();
        members.extend(indices.iter().zip(&names).map(|(&i, name)| link::Member {
            name,
            module: &packages[i].module,
        }));
        let result = WasmMod::instantiate(&engine, &members).and_then(|mods| {
            if mods.len() != indices.len() {
                return Err(Refusal::Runtime("missing mod boundary exports".into()));
            }
            let mut checked = Vec::new();
            for mut module in mods {
                let subscriptions: Vec<(String, i32, bool)> = module
                    .call(
                        CallClass::Load,
                        boundary::Frame::default(),
                        boundary::SUBSCRIBED,
                        None,
                        |_| {},
                        |r| r.take(),
                    )
                    .ok_or_else(|| Refusal::Runtime("subscriptions failed".into()))?;
                let names: Vec<String> = subscriptions
                    .iter()
                    .map(|(name, _, _)| name.clone())
                    .collect();
                module.listeners = subscriptions
                    .into_iter()
                    .filter_map(|(name, priority, receive_cancelled)| {
                        boundary::hook_id(&name).map(|hook| mod_api::Listener {
                            hook: Some(hook),
                            priority,
                            receive_cancelled,
                        })
                    })
                    .collect::<Vec<_>>()
                    .leak();
                // The ground provider runs on the worldgen workers, a contract wasm lacks yet.
                if names.iter().any(|name| name == "alter_ground") {
                    return Err(Refusal::Runtime(
                        "alter_ground: worldgen ground providers are not yet available to wasm mods"
                            .into(),
                    ));
                }
                module.subscribed = boundary::subscriptions_of("wasm mod", names);
                checked.push(module);
            }
            Ok(checked)
        });
        match result {
            Ok(mods) => {
                for (module, i) in mods.into_iter().zip(indices) {
                    let manifest = packages[i].manifest.clone();
                    let id = Box::leak(manifest.id.clone().into_boxed_str());
                    loaded.modules.push(module.clone());
                    loaded
                        .mods
                        .push(mod_api::ModEntry::new(id, Box::new(module)));
                    loaded.manifests.push(manifest);
                }
            }
            Err(why) => loaded.refused.extend(
                indices
                    .into_iter()
                    .map(|i| (packages[i].manifest.id.clone(), why.clone())),
            ),
        }
    }
    loaded
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package(
        id: &str,
        version: &str,
        deps: &str,
        sha: char,
        toolchain: &str,
        linked: &str,
    ) -> WasmPackage {
        let text = format!(
            "id = \"{id}\"\nname = \"{id}\"\nversion = \"{version}\"\napi = \"0.2\"\ncapabilities = [\"registries\"]\n\n[dependencies]\n{deps}\n[wasm]\nsha512 = \"{}\"\ntoolchain = \"{toolchain}\"\nbuilt_with = \"test\"\n{linked}",
            sha.to_string().repeat(128)
        );
        WasmPackage {
            manifest: Manifest::parse(&text).unwrap(),
            module: Vec::new(),
        }
    }

    fn ids(packages: &[WasmPackage], groups: &[Vec<usize>]) -> Vec<Vec<String>> {
        groups
            .iter()
            .map(|g| g.iter().map(|&i| packages[i].manifest.id.clone()).collect())
            .collect()
    }

    #[test]
    fn mods_sharing_a_library_share_one_group_in_dependency_order() {
        let packages = vec![
            package("b", "1.0.0", "lib = \"^1\"", 'b', "t", ""),
            package("lib", "1.2.0", "", 'a', "t", ""),
            package("c", "1.0.0", "lib = \"^1\"", 'c', "t", ""),
            package("alone", "1.0.0", "", 'd', "t", ""),
        ];
        let (groups, refused) = groups(&packages);
        assert!(refused.is_empty());
        assert_eq!(
            ids(&packages, &groups),
            [vec!["lib", "b", "c"], vec!["alone"]]
        );
    }

    #[test]
    fn bundle_ties_and_cycle_refusals_match_the_native_resolver() {
        let packages = vec![
            package("z", "1", "lib = '^1'", 'b', "t", ""),
            package("lib", "1.2", "", 'a', "t", ""),
            package("a", "1", "lib = '^1'", 'c', "t", ""),
        ];
        let (group, refused) = groups(&packages);
        assert!(refused.is_empty());
        assert_eq!(ids(&packages, &group), [vec!["lib", "z", "a"]]);
        let declared: Vec<_> = packages
            .iter()
            .map(|p| dependencies::from_manifest(&p.manifest.dependencies))
            .collect();
        let native: Vec<_> = packages
            .iter()
            .zip(&declared)
            .map(|(p, dependencies)| dependencies::Mod {
                id: &p.manifest.id,
                version: &p.manifest.version,
                has_api: true,
                dependencies,
            })
            .collect();
        assert_eq!(group[0], dependencies::resolve(&native).unwrap().order);
        let cyclic = vec![
            package("a", "1", "b = '^1'", 'a', "t", ""),
            package("b", "1", "a = '^1'", 'b', "t", ""),
            package("user", "1", "a = '^1'", 'c', "t", ""),
            package("free", "1", "", 'd', "t", ""),
        ];
        let (group, refused) = groups(&cyclic);
        assert_eq!(ids(&cyclic, &group), [vec!["free"]]);
        assert!(
            matches!(&refused[0].1, Refusal::DependencyOrder { reason } if reason.contains("a -> b -> a"))
        );
        assert!(matches!(&refused[1].1, Refusal::DependencyOrder { .. }));
        assert!(matches!(&refused[2].1, Refusal::DependencyRefused { .. }));
    }

    #[test]
    fn a_missing_required_dependency_refuses_the_mod_and_its_dependents() {
        let packages = vec![
            package("lib", "2.0.0", "", 'a', "t", ""),
            package("b", "1.0.0", "lib = \"^1\"", 'b', "t", ""),
            package(
                "c",
                "1.0.0",
                "b = \"^1\"\nextra = { version = \"^1\", optional = true }",
                'c',
                "t",
                "",
            ),
        ];
        let (groups, refused) = groups(&packages);
        assert_eq!(ids(&packages, &groups), [vec!["lib"]]);
        assert_eq!(
            refused,
            [
                (
                    "b".to_string(),
                    Refusal::MissingDependency {
                        dependency: "lib".into(),
                        version: "^1".into()
                    }
                ),
                (
                    "c".to_string(),
                    Refusal::DependencyRefused {
                        dependency: "b".into()
                    }
                ),
            ]
        );
    }

    #[test]
    fn another_build_of_a_dependency_or_toolchain_is_refused() {
        let linked = format!("[wasm.linked]\nlib = \"{}\"\n", "f".repeat(128));
        let packages = vec![
            package("lib", "1.0.0", "", 'a', "t", ""),
            package("b", "1.0.0", "lib = \"^1\"", 'b', "t", &linked),
            package("c", "1.0.0", "lib = \"^1\"", 'c', "other", ""),
        ];
        let (_, refused) = groups(&packages);
        assert_eq!(
            refused,
            [
                (
                    "b".to_string(),
                    Refusal::LinkedAgainst {
                        dependency: "lib".into()
                    }
                ),
                (
                    "c".to_string(),
                    Refusal::Toolchain {
                        found: "other".into(),
                        group: "t".into()
                    }
                ),
            ]
        );
    }
}
