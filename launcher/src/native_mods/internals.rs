//! Declared internals and typed raw listeners, shared by both native build routes.
use super::{api, inject};
use crate::i18n::trf;
use horizon_mods::internals::{self, ListedInternal};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

pub(super) const INTERNALS: &str = "internals.json";

#[derive(Default)]
pub(super) struct Catalog {
    pub(super) internals: Vec<ListedInternal>,
    pub(super) points: Vec<inject::ListedPoint>,
}

pub(super) struct Facade {
    pub(super) source: String,
    pub(super) crates: BTreeSet<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Error {
    Missing {
        mod_id: String,
        path: String,
    },
    Changed {
        mod_id: String,
        path: String,
        signature: String,
    },
    Gated {
        mod_id: String,
        path: String,
    },
    NotRaw {
        mod_id: String,
        target: String,
    },
    Path {
        path: String,
    },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::Missing { mod_id, path } => trf(
                "E_INTERNAL_MISSING: {mod} uses {path}, which this game doesn't have.",
                &[("mod", mod_id), ("path", path)],
            ),
            Self::Changed { mod_id, path, signature } => trf(
                "E_INTERNAL_CHANGED: {mod} uses {path}, whose signature is now {signature}; run cargo horizon-mod update to see what changed.",
                &[("mod", mod_id), ("path", path), ("signature", signature)],
            ),
            Self::Gated { mod_id, path } => trf(
                "E_INTERNAL_GATED: {mod} uses {path}, which exists only under a cfg; target-specific internals aren't supported.",
                &[("mod", mod_id), ("path", path)],
            ),
            Self::NotRaw { mod_id, target } => trf(
                "E_NOT_RAW: {mod}: {target} has no raw form: it is generic or takes impl Trait.",
                &[("mod", mod_id), ("target", target)],
            ),
            Self::Path { path } => trf(
                "E_INTERNAL_PATH: {path} cannot be re-exported as a declared internal; declare its types or free functions instead.",
                &[("path", path)],
            ),
        };
        f.write_str(&message)
    }
}
impl std::error::Error for Error {}

impl From<internals::Error> for Error {
    fn from(error: internals::Error) -> Self {
        match error {
            internals::Error::Missing { mod_id, path } => Self::Missing { mod_id, path },
            internals::Error::Changed {
                mod_id,
                path,
                signature,
            } => Self::Changed {
                mod_id,
                path,
                signature,
            },
            internals::Error::Gated { mod_id, path } => Self::Gated { mod_id, path },
        }
    }
}

impl Catalog {
    pub(super) fn read(dir: &Path) -> Result<Self, String> {
        let path = dir.join(INTERNALS);
        Ok(Self {
            internals: internals::read(&path)?,
            points: inject::listed(&dir.join(inject::POINTS))?,
        })
    }

    pub(super) fn check(&self, manifest: &horizon_mods::Manifest) -> Result<(), Error> {
        let Some(native) = &manifest.native else {
            return Ok(());
        };
        for declared in &native.internals {
            internals::check(&self.internals, &manifest.id, declared)?;
        }
        for injection in inject::raw_of(manifest) {
            if !self.points.iter().any(|point| {
                point.target == injection.target
                    && point.raw
                    && point.listener.is_some()
                    && point.return_listener.is_some()
            }) {
                return Err(Error::NotRaw {
                    mod_id: manifest.id.clone(),
                    target: injection.target,
                });
            }
        }
        Ok(())
    }

    pub(super) fn prepare<'a>(
        &self,
        manifests: impl IntoIterator<Item = &'a horizon_mods::Manifest>,
    ) -> Result<BTreeMap<&'a str, Facade>, Error> {
        manifests
            .into_iter()
            .map(|manifest| {
                self.facade(manifest)
                    .map(|facade| (manifest.id.as_str(), facade))
            })
            .collect()
    }

    pub(super) fn log_raw(&self, manifest: &horizon_mods::Manifest) -> Result<(), String> {
        for injection in inject::raw_of(manifest) {
            let point = self
                .points
                .iter()
                .find(|point| point.target == injection.target)
                .expect("raw target checked");
            let place = point.place(injection.at_return);
            super::append_log(&trf(
                "{mod} listens raw at {point}",
                &[("mod", &manifest.id), ("point", &place)],
            ))?;
        }
        Ok(())
    }

    /// Re-exports are the compiler's boundary: no module wildcard can widen the declaration.
    pub(super) fn facade(&self, manifest: &horizon_mods::Manifest) -> Result<Facade, Error> {
        self.check(manifest)?;
        let mut tree = Module::default();
        let mut crates = BTreeSet::from(["horizon_client".into()]);
        if let Some(native) = &manifest.native {
            for declared in &native.internals {
                let entry = self
                    .internals
                    .iter()
                    .find(|entry| {
                        entry.path == declared.path
                            && entry.hash == declared.hash
                            && entry.cfg.is_empty()
                            && !entry.shape_only
                    })
                    .expect("freshness checked");
                let path = self.export_path(entry)?;
                let parts: Vec<_> = path.split("::").collect();
                crates.insert(parts[0].to_owned());
                // `horizon_client` is the facade's root, other exposed crates retain their name.
                let parts = if parts[0] == "horizon_client" {
                    &parts[1..]
                } else {
                    &parts[..]
                };
                let mut module = &mut tree;
                for part in &parts[..parts.len() - 1] {
                    module = module.children.entry((*part).to_owned()).or_default();
                }
                module.items.insert(format!("pub use {path};"));
            }
        }
        let mut text = String::from(
            "//! This mod's declared game internals, generated from its pinned build.\n",
        );
        tree.write(&mut text, 0);
        text.push_str("pub mod raw {\n");
        let mut constructors = BTreeSet::new();
        let mut exports = BTreeSet::new();
        for injection in inject::raw_of(manifest) {
            let point = self
                .points
                .iter()
                .find(|point| point.target == injection.target)
                .expect("raw target checked");
            let listener = if injection.at_return {
                &point.return_listener
            } else {
                &point.listener
            }
            .as_ref()
            .expect("raw listener checked");
            crates.insert(
                listener
                    .split("::")
                    .next()
                    .expect("listener path")
                    .to_owned(),
            );
            let name = listener.rsplit("::").next().expect("listener path");
            exports.insert(format!("    pub use {listener};\n"));
            let head = point.listener.as_ref().expect("raw listener checked");
            let raw = head
                .strip_suffix("Listener")
                .expect("generated listener suffix")
                .to_owned()
                + "Raw";
            exports.insert(format!("    pub use {raw};\n"));
            let constructor = point.place(injection.at_return);
            let variant = name
                .strip_prefix("__")
                .and_then(|name| name.strip_suffix("Listener"))
                .expect("generated listener name");
            constructors.insert(format!(
                "    /// Registers a function of this place's exact listener type.\n    pub const fn {constructor}(id: &'static str, f: {name}) -> mod_points::__private::RawListener {{\n        // SAFETY: `f` has this place's listener type, checked by the signature.\n        unsafe {{ mod_points::__private::RawListener::new(id, mod_points::PointId::{variant}, f as *const ()) }}\n    }}\n"
            ));
        }
        for export in exports {
            text.push_str(&export);
        }
        for constructor in constructors {
            text.push_str(&constructor);
        }
        text.push_str("}\n");
        Ok(Facade {
            source: text,
            crates,
        })
    }

    fn export_path(&self, entry: &ListedInternal) -> Result<String, Error> {
        let invalid = || Error::Path {
            path: entry.path.clone(),
        };
        if entry.kind == "mod" {
            return Err(invalid());
        }
        let member = matches!(entry.kind.as_str(), "field" | "method")
            || entry.signature.starts_with("impl ");
        let path = if member {
            entry.path.rsplit_once("::").ok_or_else(invalid)?.0
        } else {
            &entry.path
        };
        // Generic arguments belong to a fingerprint, not to a `use` path.
        let mut depth = 0;
        let path: String = path
            .chars()
            .filter(|&c| match c {
                '<' => {
                    depth += 1;
                    false
                }
                '>' => {
                    depth -= 1;
                    false
                }
                _ => depth == 0,
            })
            .collect();
        syn::parse_str::<syn::Path>(&path).map_err(|_| invalid())?;
        if !member {
            return Ok(path);
        }
        let name = path.rsplit("::").next().ok_or_else(invalid)?;
        // An inherent impl in a child module names a type imported from its parent. Resolve to
        // the nearest enclosing declaration rather than re-exporting that private import.
        let mut scope = path.rsplit_once("::").ok_or_else(invalid)?.0;
        loop {
            if entry.kind == "field"
                && self
                    .internals
                    .iter()
                    .any(|entry| entry.path == scope && entry.kind == "enum" && !entry.shape_only)
            {
                return Ok(scope.to_owned());
            }
            let candidate = format!("{scope}::{name}");
            if self.internals.iter().any(|entry| {
                entry.path == candidate
                    && matches!(entry.kind.as_str(), "struct" | "enum" | "type" | "trait")
                    && !entry.shape_only
            }) {
                return Ok(candidate);
            }
            let Some((parent, _)) = scope.rsplit_once("::") else {
                break;
            };
            scope = parent;
        }
        Err(invalid())
    }
}

#[derive(Default)]
struct Module {
    items: BTreeSet<String>,
    children: BTreeMap<String, Module>,
}
impl Module {
    fn write(&self, text: &mut String, depth: usize) {
        let indent = "    ".repeat(depth);
        for item in &self.items {
            text.push_str(&format!("{indent}{item}\n"));
        }
        for (name, child) in &self.children {
            text.push_str(&format!("{indent}pub mod {name} {{\n"));
            child.write(text, depth + 1);
            text.push_str(&format!("{indent}}}\n"));
        }
    }
}

pub(super) fn needed(manifest: &horizon_mods::Manifest) -> bool {
    manifest.native.as_ref().is_some_and(|native| {
        !native.internals.is_empty()
            || native
                .injection
                .iter()
                .any(|injection| injection.raw.is_some())
    })
}

pub(super) fn package(id: &str) -> String {
    format!("horizon-internals-{id}")
}
pub(super) fn relative(id: &str) -> String {
    format!("native-internals/{id}")
}

pub(super) fn stage(
    stage: &Path,
    project: &api::Project<'_>,
    facade: &Facade,
) -> Result<(), String> {
    let manifest = &project.bundle.manifest;
    let dest = stage.join(relative(&manifest.id));
    fs::create_dir_all(dest.join("src")).map_err(|e| e.to_string())?;
    let mut cargo = format!("[package]\nname = {:?}\nversion = \"0.1.0\"\nedition = \"2024\"\n[lib]\nname = \"internals\"\n[dependencies]\nhorizon-client = {{ path = \"../../crates/horizon-client\", default-features = false }}\nmod_points = {{ package = \"horizon-mod-points\", path = \"../../crates/horizon-mod-points\" }}\n", package(&manifest.id));
    for krate in &facade.crates {
        if krate != "horizon_client" {
            let package = krate.replace('_', "-");
            cargo.push_str(&format!(
                "{krate} = {{ package = {package:?}, path = \"../../crates/{package}\" }}\n"
            ));
        }
    }
    fs::write(dest.join("Cargo.toml"), cargo).map_err(|e| e.to_string())?;
    fs::write(dest.join("src/lib.rs"), &facade.source).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> (std::path::PathBuf, super::super::Bundle, Catalog) {
        let dir = super::super::tests::dir(&format!("internals-{name}"));
        super::super::tests::write_bundle(&dir, "example", "");
        fs::write(dir.join("Cargo.toml"), "[package]\nname='example'\nversion='1.0.0'\nedition='2024'\n[dependencies]\nhorizon-mod-api={version='0.1'}\n").unwrap();
        let path = dir.join(horizon_mods::manifest::MANIFEST);
        let text = fs::read_to_string(&path).unwrap().replace("[native]", "[native]\ninternals = [{ path = 'horizon_client::game::Game', hash = '1234' }, { path = 'horizon_client::game::maps::dimension_id', hash = '5678' }]")
            + "[[native.injection]]\nid = 'count'\ntarget = \"horizon_client::game::Game<'_>::tick\"\nkind = 'HEAD'\nraw = 'count'\n";
        fs::write(path, text).unwrap();
        let bundle = super::super::bundle_in(&dir).unwrap().unwrap();
        let catalog = Catalog {
            internals: serde_json::from_value(serde_json::json!([
                { "path": "horizon_client::game::Game", "kind": "struct", "signature": "struct Game<'a> { world: World }", "hash": "1234" },
                { "path": "horizon_client::game::maps::dimension_id", "kind": "fn", "signature": "fn dimension_id() -> u8", "hash": "5678" },
                { "path": "horizon_client::game::maps::Game<'_>::dimension_id", "kind": "method", "signature": "impl Game<'_> { fn dimension_id(&self) -> u8 }", "hash": "abcd" }
            ])).unwrap(),
            points: serde_json::from_value(serde_json::json!([
                { "name": "tick", "target": "horizon_client::game::Game<'_>::tick", "raw": true,
                    "listener": "horizon_client::game::__TickListener", "return_listener": "horizon_client::game::__TickReturnListener" }
            ])).unwrap(),
        };
        (dir, bundle, catalog)
    }

    #[test]
    fn facade_has_only_two_declared_internals_and_one_typed_raw_constructor() {
        let (dir, bundle, catalog) = fixture("facade");
        let text = catalog.facade(&bundle.manifest).unwrap().source;
        assert_eq!(
            text,
            r#"//! This mod's declared game internals, generated from its pinned build.
pub mod game {
    pub use horizon_client::game::Game;
    pub mod maps {
        pub use horizon_client::game::maps::dimension_id;
    }
}
pub mod raw {
    pub use horizon_client::game::__TickListener;
    pub use horizon_client::game::__TickRaw;
    /// Registers a function of this place's exact listener type.
    pub const fn tick(id: &'static str, f: __TickListener) -> mod_points::__private::RawListener {
        // SAFETY: `f` has this place's listener type, checked by the signature.
        unsafe { mod_points::__private::RawListener::new(id, mod_points::PointId::Tick, f as *const ()) }
    }
}
"#
        );
        syn::parse_file(&text).unwrap();
        assert!(!bundle.needs_mixins());
        let project = api::Project::read(&bundle).unwrap();
        assert_eq!(super::super::prebuilt::project_refusal(&project), None);
        let main = api::main_rs(
            include_str!("../../../crates/horizon-client/src/native_main.rs.template"),
            &[&bundle.manifest],
            &catalog.points,
        );
        assert!(main.contains("#![forbid(unsafe_code)]"));
        assert!(main.contains(
            "mod_0_internals::raw::tick(\"example\", mod_0::count), // injection \"count\""
        ));
        assert!(!main.contains("/* MOD_RAW */"));
        assert!(!main.contains("mod_0::__horizon_points"));
        assert!(!main.contains("unsafe {"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn freshness_refuses_missing_changed_gated_and_shape_only_entries() {
        let (dir, bundle, mut catalog) = fixture("freshness");
        let manifest = &bundle.manifest;
        catalog.check(manifest).unwrap();
        catalog.internals[0].hash = "new".into();
        let error = catalog.check(manifest).unwrap_err();
        assert!(matches!(error, Error::Changed { .. }));
        assert!(error.to_string().contains("E_INTERNAL_CHANGED"));
        assert!(error
            .to_string()
            .contains("struct Game<'a> { world: World }"));
        catalog.internals[0].cfg = "feature = \"controllers\"".into();
        assert!(matches!(catalog.check(manifest), Err(Error::Gated { .. })));
        assert!(catalog
            .check(manifest)
            .unwrap_err()
            .to_string()
            .contains("E_INTERNAL_GATED"));
        catalog.internals[0].shape_only = true;
        assert!(matches!(
            catalog.check(manifest),
            Err(Error::Missing { .. })
        ));
        catalog.internals.remove(0);
        assert!(matches!(
            catalog.check(manifest),
            Err(Error::Missing { .. })
        ));
        assert!(catalog
            .check(manifest)
            .unwrap_err()
            .to_string()
            .contains("E_INTERNAL_MISSING"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_raw_target_must_have_a_raw_form_on_both_routes() {
        let (dir, bundle, mut catalog) = fixture("not-raw");
        for missing in [false, true] {
            if missing {
                catalog.points.clear();
            } else {
                catalog.points[0].raw = false;
            }
            let error = catalog.check(&bundle.manifest).unwrap_err();
            assert_eq!(
                error,
                Error::NotRaw {
                    mod_id: "example".into(),
                    target: "horizon_client::game::Game<'_>::tick".into()
                }
            );
            assert!(error.to_string().contains("E_NOT_RAW"));
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_lower_crate_raw_listener_adds_its_crate_to_the_facade() {
        let (dir, bundle, mut catalog) = fixture("lower-raw");
        catalog.points[0].listener = Some("horizon_world::ticking::__TickListener".into());
        catalog.points[0].return_listener =
            Some("horizon_world::ticking::__TickReturnListener".into());
        let facade = catalog.facade(&bundle.manifest).unwrap();
        assert!(facade.crates.contains("horizon_world"));
        assert!(facade
            .source
            .contains("pub use horizon_world::ticking::__TickRaw;"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn method_entries_export_the_owner_and_return_listeners_use_the_return_place() {
        let (dir, mut bundle, catalog) = fixture("owner-return");
        let native = bundle.manifest.native.as_mut().unwrap();
        native.internals[1] = horizon_mods::manifest::Internal {
            path: "horizon_client::game::maps::Game<'_>::dimension_id".into(),
            hash: "abcd".into(),
        };
        native.injection[0].kind = horizon_mods::manifest::InjectionKind::Return;
        let text = catalog.facade(&bundle.manifest).unwrap().source;
        assert_eq!(
            text.matches("pub use horizon_client::game::Game;").count(),
            1
        );
        assert!(!text.contains("maps::Game"));
        assert!(text.contains("pub use horizon_client::game::__TickReturnListener;"));
        assert!(
            text.contains("pub const fn tick_return(id: &'static str, f: __TickReturnListener)")
        );
        assert!(text.contains("PointId::TickReturn"));
        let main = api::main_rs("/* MOD_RAW */", &[&bundle.manifest], &catalog.points);
        assert!(main.contains("mod_0_internals::raw::tick_return(\"example\", mod_0::count)"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn source_staging_uses_a_per_mod_facade_and_preserves_the_library_name() {
        let (dir, bundle, catalog) = fixture("stage");
        let path = dir.join("Cargo.toml");
        let text = fs::read_to_string(&path).unwrap()
            + "horizon-internals = { git = 'https://example.invalid/horizon', rev = 'pin' }\n";
        fs::write(&path, text).unwrap();
        let project = api::Project::read(&bundle).unwrap();
        assert_eq!(super::super::prebuilt::project_refusal(&project), None);
        let stage = dir.with_extension("stage");
        fs::create_dir_all(stage.join("crates/horizon-client/src")).unwrap();
        fs::write(
            stage.join("crates/horizon-client/Cargo.toml"),
            "[package]\nname='horizon-client'\nversion='0.1.0'\n",
        )
        .unwrap();
        fs::write(
            stage.join("crates/horizon-client/src/native_main.rs.template"),
            include_str!("../../../crates/horizon-client/src/native_main.rs.template"),
        )
        .unwrap();
        fs::write(stage.join("Cargo.toml"), "[workspace]\nmembers=[]\n").unwrap();
        api::stage(
            &stage,
            &[project],
            &[vec![]],
            &super::super::dependencies::Resolution { order: vec![0] },
            &catalog,
        )
        .unwrap();
        let manifest = api::read(&stage.join("native-api/example/Cargo.toml")).unwrap();
        assert_eq!(
            manifest["dependencies"]["internals"]["package"].as_str(),
            Some("horizon-internals-example")
        );
        assert_eq!(
            manifest["dependencies"]["internals"]["path"].as_str(),
            Some("../../native-internals/example")
        );
        let facade = fs::read_to_string(stage.join("native-internals/example/src/lib.rs")).unwrap();
        assert_eq!(facade, catalog.facade(&bundle.manifest).unwrap().source);
        let game = api::read(&stage.join("crates/native-game/Cargo.toml")).unwrap();
        assert_eq!(
            game["dependencies"]["mod_0_internals"]["package"].as_str(),
            Some("horizon-internals-example")
        );
        assert!(super::super::crates::wanted(&stage, &BTreeMap::new())
            .unwrap()
            .is_empty());
        fs::remove_dir_all(stage).unwrap();
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn direct_game_dependencies_are_refused_in_every_cargo_dependency_table() {
        let (dir, bundle, _) = fixture("game-dependency");
        let path = dir.join("Cargo.toml");
        let original = fs::read_to_string(&path).unwrap();
        for table in [
            "dependencies",
            "dev-dependencies",
            "build-dependencies",
            "target.'cfg(unix)'.dependencies",
            "target.'cfg(unix)'.build-dependencies",
        ] {
            let extra = "game = { package = 'horizon-client', version = '0.1' }\n";
            let text = if table == "dependencies" {
                format!("{original}{extra}")
            } else {
                format!("{original}[{table}]\n{extra}")
            };
            fs::write(&path, text).unwrap();
            let Err(error) = api::Project::read(&bundle) else {
                panic!("accepted {table}");
            };
            assert!(matches!(error, api::ProjectError::GameDependency { .. }));
            assert!(error.to_string().contains("E_GAME_DEPENDENCY"));
        }
        fs::remove_dir_all(dir).unwrap();
    }
}
