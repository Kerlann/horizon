//! Point injections (#589): a `trait_method` injection listens at a game function's injection
//! point through the mod's `impl mod_points::Points`, at its head or its return, with no
//! source of its own spliced into the game. Every `Game` method the staging pass admits is a
//! point, on both routes: the prebuilt game was staged before it was built, and the source route
//! runs the revision's own pass on its copy; `points.json` lists them.
use horizon_mods::manifest::InjectionKind;
use std::{fs, path::Path};

/// One `trait_method` injection: the mod's method, the function it listens at, and where.
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
    pub(super) target: String,
    pub(super) raw: bool,
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

/// Why a point injection can't be served as written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum PointError {
    /// The target isn't a point in this game build.
    NotAPoint { injection: Box<PointInjection> },
    /// The method isn't that point's head or return method.
    WrongMethod {
        injection: Box<PointInjection>,
        expected: String,
    },
}

impl std::fmt::Display for PointError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAPoint { injection } => write!(
                f,
                "{}: injection {} listens at {}, which isn't an injection point in this game",
                injection.mod_id, injection.id, injection.target
            ),
            Self::WrongMethod {
                injection,
                expected,
            } => write!(
                f,
                "{}: injection {} names trait_method {}, but {} at {} is Points::{expected}",
                injection.mod_id,
                injection.id,
                injection.method,
                if injection.at_return {
                    "the return"
                } else {
                    "the head"
                },
                injection.target
            ),
        }
    }
}

/// A mod's point injections, in manifest order. The manifest's own check has already refused any
/// that isn't a plain HEAD or RETURN on a `target`.
pub(super) fn of(manifest: &horizon_mods::Manifest) -> Vec<PointInjection> {
    injections(manifest, false)
}

pub(super) fn raw_of(manifest: &horizon_mods::Manifest) -> Vec<PointInjection> {
    injections(manifest, true)
}

fn injections(manifest: &horizon_mods::Manifest, raw: bool) -> Vec<PointInjection> {
    manifest
        .native
        .iter()
        .flat_map(|native| &native.injection)
        .filter_map(|injection| {
            Some(PointInjection {
                mod_id: manifest.id.clone(),
                id: injection.id.clone(),
                target: injection.target.clone()?,
                method: if raw {
                    injection.raw.clone()?
                } else {
                    injection.trait_method.clone()?
                },
                at_return: matches!(injection.kind, InjectionKind::Return),
            })
        })
        .collect()
}

/// Checks `injections` against a game build's `points`: each target is a point there, and each
/// method is that point's head (`<point>`) or return (`<point>_return`) method.
pub(super) fn check(
    injections: &[PointInjection],
    points: &[ListedPoint],
) -> Result<(), PointError> {
    for injection in injections {
        let Some(point) = points.iter().find(|point| point.target == injection.target) else {
            return Err(PointError::NotAPoint {
                injection: Box::new(injection.clone()),
            });
        };
        let expected = point.place(injection.at_return);
        if injection.method != expected {
            return Err(PointError::WrongMethod {
                injection: Box::new(injection.clone()),
                expected,
            });
        }
    }
    Ok(())
}

/// A game build's points, from its `points.json`.
pub(super) fn listed(path: &Path) -> Result<Vec<ListedPoint>, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// The generated `main.rs`'s check, at compile time, that mod `index` overrides each point method
/// its injections name: a typed error naming the injection, not a silent no-op.
pub(super) fn main_checks(index: usize, injections: &[PointInjection]) -> String {
    injections
        .iter()
        .map(|injection| {
            format!(
                "const _: () = assert!(horizon_client::entry::point_listed(mod_{index}::__HORIZON_POINTS, {:?}), {:?});\n",
                injection.method,
                format!(
                    "{}: injection {} names Points::{}, which the mod's #[mod_api::horizon_mod] impl mod_points::Points doesn't override",
                    injection.mod_id, injection.id, injection.method
                )
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::{Bundle, MANIFEST};
    use super::*;

    fn fixture(name: &str, injection: &str) -> (std::path::PathBuf, Bundle) {
        let root = super::super::tests::dir(&format!("inject-{name}"));
        super::super::tests::write_bundle(&root, "example", "");
        let path = root.join(horizon_mods::manifest::MANIFEST);
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
        let path = root.join(horizon_mods::manifest::MANIFEST);
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
        let path = root.join(horizon_mods::manifest::MANIFEST);
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
        let path = root.join(horizon_mods::manifest::MANIFEST);
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

    fn injection(target: &str, method: &str, at_return: bool) -> PointInjection {
        PointInjection {
            mod_id: "example".into(),
            id: "hook".into(),
            target: target.into(),
            method: method.into(),
            at_return,
        }
    }

    #[test]
    fn a_point_injection_names_the_points_head_or_return_method() {
        let tick = "horizon_client::game::Game<'_>::tick";
        let points = [ListedPoint {
            raw: false,
            listener: None,
            return_listener: None,
            name: "tick".into(),
            target: tick.into(),
        }];
        assert_eq!(check(&[injection(tick, "tick", false)], &points), Ok(()));
        assert_eq!(
            check(&[injection(tick, "tick_return", true)], &points),
            Ok(())
        );
        let wrong = check(&[injection(tick, "tick", true)], &points).unwrap_err();
        assert_eq!(
            wrong,
            PointError::WrongMethod {
                injection: Box::new(injection(tick, "tick", true)),
                expected: "tick_return".into()
            }
        );
        assert!(wrong.to_string().contains("injection hook"), "{wrong}");
        let other = "horizon_client::game::hand::Game<'_>::use_with_hand";
        assert_eq!(
            check(&[injection(other, "use_with_hand", false)], &points),
            Err(PointError::NotAPoint {
                injection: Box::new(injection(other, "use_with_hand", false))
            })
        );
    }

    #[test]
    fn main_checks_name_the_injection() {
        let checks = main_checks(2, &[injection("t", "tick", false)]);
        assert!(
            checks.contains("mod_2::__HORIZON_POINTS, \"tick\""),
            "{checks}"
        );
        assert!(
            checks.contains("injection hook names Points::tick"),
            "{checks}"
        );
    }
}
