//! Raw function declarations and the staged point catalog.
use horizon_mod_package::manifest::InjectionKind;
use std::{fs, path::Path};

/// One raw function declaration and its head or return target.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PointInjection {
    pub(super) mod_id: String,
    pub(super) id: String,
    pub(super) target: String,
    pub(super) method: String,
    /// At the function's return, not its head.
    pub(super) at_return: bool,
}

/// A game build's point, as its `points.json` lists it.
#[derive(Clone, Debug, serde::Deserialize)]
pub(super) struct ListedPoint {
    pub(super) name: String,
    #[serde(default)]
    pub(super) private: bool,
    #[serde(default)]
    pub(super) record_name: Option<String>,
    pub(super) target: String,
    pub(super) raw: bool,
    pub(super) at_return: bool,
    pub(super) listener: Option<String>,
    pub(super) return_listener: Option<String>,
}

impl ListedPoint {
    pub(super) fn place(&self, at_return: bool) -> String {
        if at_return {
            format!("{}_return", self.name)
        } else {
            self.name.clone()
        }
    }
}

/// The `points.json` the game's build script writes, beside the prebuilt game or in the
/// staged game's build output.
pub(super) const POINTS: &str = "points.json";

pub(super) fn raw_of(
    listed: &[ListedPoint],
    manifest: &horizon_mod_package::Manifest,
) -> Vec<PointInjection> {
    manifest
        .native
        .iter()
        .flat_map(|native| &native.injection)
        .filter(|injection| injection.selector.is_none())
        .filter_map(|injection| {
            Some(PointInjection {
                mod_id: manifest.id.clone(),
                id: injection.id.clone(),
                target: injection.target.clone()?,
                method: injection.raw.clone()?,
                at_return: matches!(injection.kind, InjectionKind::Return),
            })
        })
        .chain(super::slots::points(listed, manifest))
        .collect()
}

#[derive(serde::Deserialize)]
pub(super) struct Catalog {
    pub points: Vec<ListedPoint>,
    pub sites: Vec<super::sites::Site>,
}

pub(super) fn catalog(path: &Path) -> Result<Catalog, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::super::{Bundle, MANIFEST};
    use super::*;

    fn fixture(name: &str, injection: &str) -> (std::path::PathBuf, Bundle) {
        let root = super::super::tests::dir(&format!("inject-{name}"));
        super::super::tests::write_bundle(&root, "example", "");
        let path = root.join(horizon_mod_package::manifest::MANIFEST);
        let text = fs::read_to_string(&path).unwrap()
            + &injection.replace("[[injection]]", "[[native.injection]]");
        fs::write(path, text).unwrap();
        let bundle = super::super::bundle_in(&root).unwrap().unwrap();
        (root, bundle)
    }

    #[test]
    fn a_module_stages_with_a_helper_that_calls_into_it() {
        let (root, _bundle) = fixture(
            "module",
            "[[injection]]\nid = 'hook'\ntarget = 'horizon_client::game::tick'\nkind = 'HEAD'\nhelper = 'src/lib.rs'\nbindings = []\n",
        );
        // The helper calls into the module; both must reach the tool together.
        fs::write(
            root.join("src/lib.rs"),
            "pub fn tick_hook() { crate::runtime::bump(&mut 0); }\n",
        )
        .unwrap();
        fs::write(
            root.join("src/runtime.rs"),
            "pub(crate) fn bump(value: &mut i32) { *value += 1; }\n",
        )
        .unwrap();
        let path = root.join(horizon_mod_package::manifest::MANIFEST);
        let text = fs::read_to_string(&path).unwrap()
            + "[[native.module]]\nid = 'runtime'\nfile = 'crates/horizon-client/src/game.rs'\nname = 'runtime'\nsource = 'src/runtime.rs'\n";
        fs::write(&path, text).unwrap();
        let bundle = super::super::bundle_in(&root).unwrap().unwrap();
        let mixin_input = root.with_extension("mixins");
        let tool_mods =
            super::super::stage_mixins(std::slice::from_ref(&bundle), &mixin_input).unwrap();
        assert_eq!(tool_mods.len(), 1);
        let staged = &tool_mods[0].dir;
        let manifest = fs::read_to_string(staged.join(MANIFEST)).unwrap();
        assert!(manifest.contains("[[module]]"));
        assert!(manifest.contains("name = \"runtime\""));
        assert!(manifest.contains("source = \"src/runtime.rs\""));
        // `visibility` stays out of the tool manifest until a package sets it.
        assert!(!manifest.contains("visibility"));
        assert_eq!(
            fs::read_to_string(staged.join("src/runtime.rs")).unwrap(),
            "pub(crate) fn bump(value: &mut i32) { *value += 1; }\n"
        );
        assert!(staged.join("src/lib.rs").is_file());
    }

    #[test]
    fn a_module_only_bundle_still_needs_the_tool() {
        let root = super::super::tests::dir("inject-module-only");
        super::super::tests::write_bundle(&root, "example", "");
        let path = root.join(horizon_mod_package::manifest::MANIFEST);
        let text = fs::read_to_string(&path).unwrap()
            + "[[native.module]]\nid = 'runtime'\nfile = 'crates/horizon-client/src/game.rs'\nname = 'runtime'\nsource = 'src/lib.rs'\n";
        fs::write(&path, text).unwrap();
        let bundle = super::super::bundle_in(&root).unwrap().unwrap();
        let mixin_input = root.with_extension("mixins");
        let tool_mods =
            super::super::stage_mixins(std::slice::from_ref(&bundle), &mixin_input).unwrap();
        assert_eq!(tool_mods.len(), 1);
        assert!(tool_mods[0].dir.join(MANIFEST).is_file());
    }

    #[test]
    fn mixin_inputs_inside_api_are_not_excluded_from_cache() {
        let (root, bundle) = fixture(
            "api-helper",
            "[[injection]]\nid = 'hook'\ntarget = 'horizon_client::game::tick'\nkind = 'HEAD'\nhelper = 'src/lib.rs'\n",
        );
        let key = || {
            super::super::inject_cache::key(
                "revision",
                "mixins",
                super::super::Profile::Balanced,
                std::slice::from_ref(&bundle),
            )
            .unwrap()
        };
        let before = key();
        fs::write(root.join("src/lib.rs"), "fn changed() {}\n").unwrap();
        assert_ne!(before, key());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_manual_tool_crate_declarations() {
        let (root, _) = fixture("tool-route", "");
        let path = root.join(horizon_mod_package::manifest::MANIFEST);
        let text = fs::read_to_string(&path).unwrap();
        fs::write(path, text + "[[crate]]\nid='code'\npath='src'\n").unwrap();
        assert!(super::super::bundle_in(&root).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn installed_build_key_tracks_dispatch_order() {
        let (root, first) = fixture("order", "");
        let mut second = first.clone();
        second.manifest.id = "second".into();
        let a = super::super::version_for(
            "0123456789012345678901234567890123456789",
            "mixins",
            super::super::Profile::Balanced,
            super::super::Target::Client,
            None,
            &[first.clone(), second.clone()],
        )
        .unwrap();
        let b = super::super::version_for(
            "0123456789012345678901234567890123456789",
            "mixins",
            super::super::Profile::Balanced,
            super::super::Target::Client,
            None,
            &[second, first],
        )
        .unwrap();
        assert_ne!(a, b);
        fs::remove_dir_all(root).unwrap();
    }
}
