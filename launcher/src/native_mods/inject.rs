//! Point injections (#589): a `trait_method` injection listens at a game function's injection
//! point through the mod's `impl mod_api::points::Points`, at its head or its return, with no
//! source of its own spliced into the game. On the prebuilt route the game already has the point
//! (its `points.json`); on the source route the launcher marks the function `#[point]` in the
//! staged copy, so the mod compiles the same way on both.
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
}

/// The `points.json` the game's build script writes, beside the prebuilt game or in the
/// staged game's build output.
pub(super) const POINTS: &str = "points.json";

/// Why a point injection can't be served as written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum PointError {
    /// The target isn't a point in this game build.
    NotAPoint { injection: PointInjection },
    /// The method isn't that point's head or return method.
    WrongMethod {
        injection: PointInjection,
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
    manifest
        .native
        .iter()
        .flat_map(|native| &native.injection)
        .filter_map(|injection| {
            Some(PointInjection {
                mod_id: manifest.id.clone(),
                id: injection.id.clone(),
                target: injection.target.clone()?,
                method: injection.trait_method.clone()?,
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
                injection: injection.clone(),
            });
        };
        let expected = match injection.at_return {
            true => format!("{}_return", point.name),
            false => point.name.clone(),
        };
        if injection.method != expected {
            return Err(PointError::WrongMethod {
                injection: injection.clone(),
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

/// The attribute the source route marks a target with: `both` sides, as the launcher can't know
/// a function's side; a listener there takes `HostSide`.
const POINT: &str = "#[horizon_mod_api_macros::point(side = \"both\")]";

/// Marks each target of `injections` that isn't already a point `#[point]` in the staged game at
/// `stage`, so its `Points` methods exist when the mod API builds. A target is a `Game` method
/// named by its module path, as a hook's `target` is.
pub(super) fn mark(stage: &Path, injections: &[PointInjection]) -> Result<(), String> {
    for injection in injections {
        let (path, method) = injection
            .target
            .strip_prefix("horizon_client::game::")
            .and_then(|rest| rest.rsplit_once("::"))
            .filter(|(path, _)| path.ends_with("Game<'_>"))
            .ok_or_else(|| {
                format!(
                    "{}: injection {}: {} isn't a Game method; only Game methods take points yet",
                    injection.mod_id, injection.id, injection.target
                )
            })?;
        let module = path.trim_end_matches("Game<'_>").trim_end_matches("::");
        let file = match module {
            "" => stage.join("crates/horizon-client/src/game.rs"),
            module => stage
                .join("crates/horizon-client/src/game")
                .join(format!("{}.rs", module.replace("::", "/"))),
        };
        let source = fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        let lines: Vec<&str> = source.lines().collect();
        let header = format!("fn {method}(");
        let found: Vec<usize> = (0..lines.len())
            .filter(|&i| {
                let line = lines[i].trim_start();
                let line = line
                    .strip_prefix("pub(crate) ")
                    .or_else(|| line.strip_prefix("pub(super) "))
                    .or_else(|| line.strip_prefix("pub "))
                    .unwrap_or(line);
                line.starts_with(&header)
            })
            .collect();
        let [at] = found[..] else {
            return Err(format!(
                "{}: injection {}: {} has {} functions named {method}; a point needs exactly one",
                injection.mod_id,
                injection.id,
                file.display(),
                found.len()
            ));
        };
        if lines[at.saturating_sub(1)].contains("::point(") {
            continue;
        }
        let indent = &lines[at][..lines[at].len() - lines[at].trim_start().len()];
        let mut out = String::with_capacity(source.len() + 64);
        for (i, line) in lines.iter().enumerate() {
            if i == at {
                out.push_str(indent);
                out.push_str(POINT);
                out.push('\n');
            }
            out.push_str(line);
            out.push('\n');
        }
        fs::write(&file, out).map_err(|e| format!("{}: {e}", file.display()))?;
    }
    Ok(())
}

/// The generated `main.rs`'s check, at compile time, that mod `index` overrides each point method
/// its injections name: a typed error naming the injection, not a silent no-op.
pub(super) fn main_checks(index: usize, injections: &[PointInjection]) -> String {
    injections
        .iter()
        .map(|injection| {
            format!(
                "const _: () = assert!(mod_api::__private::point::lists(mod_{index}::__HORIZON_POINTS, {:?}), {:?});\n",
                injection.method,
                format!(
                    "{}: injection {} names Points::{}, which the mod's #[mod_api::horizon_mod] impl Points doesn't override",
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
                super::super::Profile::Performance,
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
            super::super::Profile::Performance,
            super::super::Target::Client,
            &[first.clone(), second.clone()],
        )
        .unwrap();
        let b = super::super::version_for(
            "0123456789012345678901234567890123456789",
            "mixins",
            super::super::Profile::Performance,
            super::super::Target::Client,
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
                injection: injection(tick, "tick", true),
                expected: "tick_return".into()
            }
        );
        assert!(wrong.to_string().contains("injection hook"), "{wrong}");
        let other = "horizon_client::game::hand::Game<'_>::use_with_hand";
        assert_eq!(
            check(&[injection(other, "use_with_hand", false)], &points),
            Err(PointError::NotAPoint {
                injection: injection(other, "use_with_hand", false)
            })
        );
    }

    #[test]
    fn the_source_route_marks_a_target_once() {
        let root = super::super::tests::dir("inject-mark");
        let game = root.join("crates/horizon-client/src/game");
        fs::create_dir_all(&game).unwrap();
        fs::write(
            game.join("hand.rs"),
            "impl Game<'_> {\n    /// Use.\n    pub(super) fn use_with_hand(&mut self) {}\n    fn use_with_hands(&self) {}\n}\n",
        )
        .unwrap();
        let use_hand = injection(
            "horizon_client::game::hand::Game<'_>::use_with_hand",
            "use_with_hand",
            false,
        );
        mark(&root, &[use_hand.clone(), use_hand.clone()]).unwrap();
        let marked = fs::read_to_string(game.join("hand.rs")).unwrap();
        assert_eq!(marked.matches("::point(").count(), 1, "{marked}");
        assert!(marked.contains("/// Use.\n    #[horizon_mod_api_macros::point(side = \"both\")]\n    pub(super) fn use_with_hand"));
        let free = injection("horizon_world::loot::load", "load", false);
        assert!(mark(&root, &[free])
            .unwrap_err()
            .contains("isn't a Game method"));
        fs::remove_dir_all(root).unwrap();
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
