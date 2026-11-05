//! The one runtime for wasm mods. Packages declaring `[wasm]` are grouped by their
//! `[dependencies]` (the same model as native mods: required and optional, semver on
//! `horizon-mod.toml` versions, one library copy shared by its dependents), each group is
//! linked into one component by `wit_component::Linker` (one memory, one copy of each
//! member), compiled once and cached by its members' hashes, and instantiated with the
//! mod API as its only imports. A group runs sandboxed from the others and from the game; a
//! trap stops its group. Every refusal names its reason for the Mods page.
use horizon_mods::manifest::{Dependency, Manifest};
use mod_api::__private::wasm_host::{Budgets, EPOCH, LoadError, Member};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

mod points;

pub use points::Listening;

/// The game's shared Rust standard library member, shipped beside the executable and linked
/// first into every group (members are built against the same pinned toolchain).
pub const STD_MEMBER: &str = "horizon-wasm-std.wasm";

/// The runtime's settings.
#[derive(Clone, Debug, Default)]
pub struct RuntimeConfig {
    /// Run mods on Pulley, wasmtime's interpreter, instead of compiling them: for hosts that
    /// can't map executable memory. About 30–40× slower on compute; off by default.
    pub pulley: bool,
    /// Where compiled groups are kept between runs; none compiles every start.
    pub cache: Option<PathBuf>,
    pub budgets: Budgets,
}

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
    /// A dependency's installed `mod.wasm` isn't the one this mod was linked against.
    LinkedAgainst { dependency: String },
    /// Members of one group were built with different toolchains.
    Toolchain { found: String, group: String },
    /// A dependency of this mod was refused, so its group can't link.
    DependencyRefused { dependency: String },
    /// The group didn't link (the first problem the linker named).
    Link(String),
    /// The group's component didn't compile.
    Compile(String),
    /// This game has no wasm standard library member beside it, so no wasm mod can link.
    NoStd,
    /// The package's `mod.wasm` failed its checks.
    Package(String),
    /// The group didn't instantiate or its exports aren't this game's.
    Load(LoadError),
    /// An injection, or a place the mod listens at, names no point of this game.
    NoSuchPoint { point: String },
    /// The mod listens at a place its `horizon-mod.toml` doesn't list.
    UndeclaredPoint { point: String },
    /// Its `horizon-mod.toml` lists a place the mod doesn't listen at.
    UnfilledPoint { point: String },
    /// The mod was built for another shape of a point's record than this game's.
    ChangedPoint {
        point: String,
        built: String,
        game: String,
    },
    /// Its listeners couldn't be set up.
    Points(String),
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
            Self::LinkedAgainst { dependency } => {
                write!(f, "built against another build of {dependency}")
            }
            Self::Toolchain { found, group } => {
                write!(f, "built with {found}, its dependencies with {group}")
            }
            Self::DependencyRefused { dependency } => {
                write!(f, "its dependency {dependency} didn't load")
            }
            Self::Link(why) => write!(f, "didn't link: {why}"),
            Self::Compile(why) => write!(f, "didn't compile: {why}"),
            Self::NoStd => write!(
                f,
                "this game was built without wasm mod support ({STD_MEMBER})"
            ),
            Self::Package(why) => write!(f, "{why}"),
            Self::Load(why) => write!(f, "{why}"),
            Self::NoSuchPoint { point } => {
                write!(f, "listens at {point}, which this game has no point at")
            }
            Self::UndeclaredPoint { point } => write!(
                f,
                "listens at {point}, which its horizon-mod.toml doesn't list"
            ),
            Self::UnfilledPoint { point } => write!(
                f,
                "lists {point} in its horizon-mod.toml but doesn't listen there"
            ),
            Self::ChangedPoint { point, built, game } => write!(
                f,
                "was built for another {point}: its record is ({built}), this game's ({game}); rebuild it"
            ),
            Self::Points(why) => write!(f, "{why}"),
        }
    }
}

impl std::error::Error for Refusal {}

/// What loading the installed wasm mods gave: the mods to install, in dependency order, and
/// each refused mod with its reason.
pub struct Loaded {
    pub mods: Vec<mod_api::ModEntry>,
    pub refused: Vec<(String, Refusal)>,
    /// The mods' listeners at points, in the order of `mods`: registered after the native mods'.
    pub listening: Vec<Listening>,
}

/// The engine every group runs on: calls limited by time, its epoch advanced by a thread of
/// its own for as long as the engine lives, or by instructions with `budgets.fuel`.
pub fn engine(config: &RuntimeConfig) -> wasmtime::Result<wasmtime::Engine> {
    let mut engine = wasmtime::Config::new();
    let fuel = config.budgets.fuel.is_some();
    engine
        .wasm_component_model(true)
        .consume_fuel(fuel)
        .epoch_interruption(!fuel);
    if config.pulley {
        engine.target("pulley64")?;
    }
    let engine = wasmtime::Engine::new(&engine)?;
    if !fuel {
        let weak = engine.weak();
        std::thread::Builder::new()
            .name("wasm epoch".into())
            .spawn(move || {
                while let Some(engine) = weak.upgrade() {
                    engine.increment_epoch();
                    drop(engine);
                    std::thread::sleep(EPOCH);
                }
            })?;
    }
    Ok(engine)
}

fn dependency_version(dependency: &Dependency) -> (&semver::VersionReq, bool) {
    match dependency {
        Dependency::Version(version) => (version, false),
        Dependency::Detailed(details) => (&details.version, details.optional),
    }
}

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
    for (i, package) in packages.iter().enumerate() {
        let wasm = package
            .manifest
            .wasm
            .as_ref()
            .expect("a wasm package declares [wasm]");
        for (id, dependency) in &package.manifest.dependencies {
            let (version, optional) = dependency_version(dependency);
            let found = by_id.get(id.as_str()).filter(|&&d| {
                semver::Version::parse(&packages[d].manifest.version)
                    .is_ok_and(|v| version.matches(&v))
            });
            let Some(&d) = found else {
                if !optional {
                    refused.entry(i).or_insert(Refusal::MissingDependency {
                        dependency: id.clone(),
                        version: version.to_string(),
                    });
                }
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
    // A refused mod takes its dependents with it, until nothing changes.
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
        .map(|members| dependency_order(packages, members))
        .collect();
    let refused = refused
        .into_iter()
        .map(|(i, r)| (packages[i].manifest.id.clone(), r))
        .collect();
    (groups, refused)
}

/// A group's members, each after the members it depends on (ties by id).
fn dependency_order(packages: &[WasmPackage], members: Vec<usize>) -> Vec<usize> {
    let mut placed: Vec<usize> = Vec::new();
    let mut left: BTreeSet<(&str, usize)> = members
        .iter()
        .map(|&i| (packages[i].manifest.id.as_str(), i))
        .collect();
    while !left.is_empty() {
        let ready = left
            .iter()
            .copied()
            .find(|&(_, i)| {
                packages[i]
                    .manifest
                    .dependencies
                    .keys()
                    .all(|id| !left.iter().any(|&(other, _)| other == id))
            })
            // A cycle: link the rest as they are; the linker resolves symbols both ways.
            .unwrap_or_else(|| *left.iter().next().unwrap());
        left.remove(&ready);
        placed.push(ready.1);
    }
    placed
}

/// The linked component of one group, from the cache when its members are unchanged.
fn component(
    engine: &wasmtime::Engine,
    config: &RuntimeConfig,
    std_member: &[u8],
    packages: &[WasmPackage],
    members: &[usize],
) -> Result<wasmtime::component::Component, Refusal> {
    let key = {
        let mut key = String::new();
        for &i in members {
            let wasm = packages[i]
                .manifest
                .wasm
                .as_ref()
                .expect("a wasm package declares [wasm]");
            key.push_str(wasm.sha512());
        }
        key.push_str(&horizon_mods::manifest::sha512_of(std_member));
        key.push_str(if config.pulley { "pulley" } else { "native" });
        // Code compiled to count fuel differs from code checking the epoch.
        key.push_str(if config.budgets.fuel.is_some() {
            "fuel"
        } else {
            "epoch"
        });
        horizon_mods::manifest::sha512_of(key.as_bytes())
    };
    let cached = config
        .cache
        .as_ref()
        .map(|dir| dir.join(format!("{}.cwasm", &key[..32])));
    if let Some(path) = &cached
        && let Ok(bytes) = std::fs::read(path)
        // SAFETY: the file is the game's own cache, written by `serialize` below for this
        // engine configuration; wasmtime checks its version and settings when reading it.
        && let Ok(component) = unsafe { wasmtime::component::Component::deserialize(engine, &bytes) }
    {
        return Ok(component);
    }
    let mut linker = wit_component::Linker::default();
    linker.stub_missing_functions(true);
    linker
        .library("libstd.so", std_member, false)
        .map_err(|e| Refusal::Link(format!("{e:#}")))?;
    for &i in members {
        let id = &packages[i].manifest.id;
        linker
            .library(&format!("lib{id}.so"), &packages[i].module, false)
            .map_err(|e| Refusal::Link(format!("{id}: {e:#}")))?;
    }
    let linked = linker
        .encode()
        .map_err(|e| Refusal::Link(format!("{e:#}")))?;
    let component = wasmtime::component::Component::new(engine, &linked)
        .map_err(|e| Refusal::Compile(format!("{e:#}")))?;
    if let Some(path) = cached {
        let written = component
            .serialize()
            .map_err(|e| e.to_string())
            .and_then(|bytes| {
                std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
                std::fs::write(&path, bytes).map_err(|e| e.to_string())
            });
        if let Err(why) = written {
            log::warn!("wasm mods: couldn't cache {}: {why}", path.display());
        }
    }
    Ok(component)
}

/// Loads every installed wasm mod. `std_member` is the game's shared Rust standard library
/// module, the first member of every group.
pub fn load(config: &RuntimeConfig, std_member: &[u8], packages: Vec<WasmPackage>) -> Loaded {
    let engine = match engine(config) {
        Ok(engine) => engine,
        Err(e) => {
            let why = Refusal::Compile(format!("{e:#}"));
            let refused = packages
                .iter()
                .map(|p| (p.manifest.id.clone(), why.clone()))
                .collect();
            return Loaded {
                mods: Vec::new(),
                refused,
                listening: Vec::new(),
            };
        }
    };
    let (groups, mut refused) = groups(&packages);
    let mut mods = Vec::new();
    let mut listening = Vec::new();
    for members in groups {
        let members_of_group = members.clone();
        let ids: Vec<&'static str> = members
            .iter()
            .map(|&i| &*Box::leak(packages[i].manifest.id.clone().into_boxed_str()))
            .collect();
        let group: Vec<Member> = ids
            .iter()
            .map(|&id| Member {
                id,
                exports: horizon_mods::manifest::export_prefix(id),
            })
            .collect();
        let loaded =
            component(&engine, config, std_member, &packages, &members).and_then(|component| {
                mod_api::__private::wasm_host::instantiate(
                    &engine,
                    &component,
                    &group,
                    config.budgets,
                )
                .map_err(Refusal::Load)
            });
        match loaded {
            Ok(members) => {
                // Every member's points are checked before any is installed: a member refused
                // over its points takes the members depending on it with it, as a dependency
                // refused earlier does.
                let mut checked: Vec<_> = members
                    .into_iter()
                    .zip(&members_of_group)
                    .map(|(member, &i)| {
                        let listening = points::listen(&packages[i].manifest, &member.exports);
                        (member, i, listening)
                    })
                    .collect();
                loop {
                    let failed: BTreeSet<&str> = checked
                        .iter()
                        .filter(|(_, _, listening)| listening.is_err())
                        .map(|(_, i, _)| packages[*i].manifest.id.as_str())
                        .collect();
                    let mut changed = false;
                    for (_, i, listening) in &mut checked {
                        if listening.is_ok()
                            && let Some(id) = packages[*i]
                                .manifest
                                .dependencies
                                .keys()
                                .find(|id| failed.contains(id.as_str()))
                        {
                            *listening = Err(Refusal::DependencyRefused {
                                dependency: id.clone(),
                            });
                            changed = true;
                        }
                    }
                    if !changed {
                        break;
                    }
                }
                for (member, _, checked) in checked {
                    match checked {
                        Ok(points) => {
                            mods.push(member.entry);
                            listening.extend(points);
                        }
                        Err(why) => refused.push((member.exports.id().to_string(), why)),
                    }
                }
            }
            Err(why) => refused.extend(ids.iter().map(|id| (id.to_string(), why.clone()))),
        }
    }
    Loaded {
        mods,
        refused,
        listening,
    }
}

static REFUSED: OnceLock<Vec<(String, Refusal)>> = OnceLock::new();

/// The wasm mods refused at startup, each with its reason, for the Mods page.
pub fn refused() -> &'static [(String, Refusal)] {
    REFUSED.get().map_or(&[], Vec::as_slice)
}

/// Loads the wasm mods the player turned on in `game_dir` (and every `dev` package), once at
/// startup: the same selection as content mods (`horizon_mods::selection`), so a wasm mod's
/// `assets/` and `data/` load as content beside its code. Refusals are kept for [`refused`]; the
/// mods, and their listeners at points for the game to register after the native mods'.
pub fn load_installed(
    executable: &Path,
    game_dir: &Path,
    dev: &[PathBuf],
    config: &RuntimeConfig,
) -> (Vec<mod_api::ModEntry>, Vec<Listening>) {
    let enabled = horizon_mods::mod_list::ModList::load(game_dir).enabled;
    let mut refused = Vec::new();
    let packages: Vec<WasmPackage> =
        horizon_mods::selection::content_packages(game_dir, &enabled, dev)
            .filter(|selected| selected.package.manifest.wasm.is_some())
            .filter_map(
                |selected| match horizon_mods::Package::read_wasm(&selected.path) {
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
    let loaded = if packages.is_empty() {
        (Vec::new(), Vec::new())
    } else {
        match std::fs::read(executable.with_file_name(STD_MEMBER)) {
            Ok(std_member) => {
                let loaded = load(config, &std_member, packages);
                refused.extend(loaded.refused);
                (loaded.mods, loaded.listening)
            }
            Err(_) => {
                refused.extend(
                    packages
                        .into_iter()
                        .map(|p| (p.manifest.id, Refusal::NoStd)),
                );
                (Vec::new(), Vec::new())
            }
        }
    };
    for (id, why) in &refused {
        log::warn!("wasm mod {id} refused: {why}");
    }
    let _ = REFUSED.set(refused);
    loaded
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A component importing `name` from the game's host interface, with nothing else.
    fn importing(name: &str) -> Vec<u8> {
        let mut resolve = wit_parser::Resolve::default();
        let package = resolve
            .push_str(
                "mod.wit",
                &format!(
                    "package horizon:mod-api@0.1.0;\ninterface host {{ {name}: func(); }}\nworld m {{ import host; }}"
                ),
            )
            .unwrap();
        let world = resolve.select_world(&[package], Some("m")).unwrap();
        let mut module = wasm_encoder::Module::new();
        let mut types = wasm_encoder::TypeSection::new();
        types.ty().function([], []);
        module.section(&types);
        let mut imports = wasm_encoder::ImportSection::new();
        imports.import(
            "horizon:mod-api/host@0.1.0",
            name,
            wasm_encoder::EntityType::Function(0),
        );
        module.section(&imports);
        let mut module = module.finish();
        wit_component::embed_component_metadata(
            &mut module,
            &resolve,
            world,
            wit_component::StringEncoding::UTF8,
            false,
        )
        .unwrap();
        wit_component::ComponentEncoder::default()
            .module(&module)
            .unwrap()
            .encode()
            .unwrap()
    }

    #[test]
    fn a_mod_importing_what_the_game_lacks_is_refused_naming_it() {
        let engine = engine(&RuntimeConfig::default()).unwrap();
        let component =
            wasmtime::component::Component::new(&engine, importing("not-a-host-call")).unwrap();
        let refused = mod_api::__private::wasm_host::instantiate(
            &engine,
            &component,
            &[],
            Budgets::default(),
        )
        .err()
        .unwrap();
        let LoadError::Import { import, .. } = &refused else {
            panic!("refused for another reason: {refused}");
        };
        assert_eq!(import, "horizon:mod-api/host@0.1.0#not-a-host-call");
    }

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
