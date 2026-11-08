//! One constructor emitter for native and wasm API injection listeners.
use horizon_mod_package::{
    Manifest,
    manifest::inject::{Access, Kind},
};

/// The part of a staged `points.json` entry needed to resolve an API listener constructor.
#[derive(Clone, Debug, serde::Deserialize)]
pub struct Point {
    /// The constructor's head-place name.
    pub name: String,
    /// The function's logical path.
    pub target: String,
    /// A private staged source target has no public API constructor.
    #[serde(default)]
    pub private: bool,
    /// Whether an API return place exists (not a borrowed return).
    pub at_return: bool,
}

/// Remove type arguments and whitespace exactly as the native launcher resolves point paths.
pub fn plain_path(path: &str) -> String {
    let mut depth = 0usize;
    path.chars()
        .filter(|&c| match c {
            '<' => {
                depth += 1;
                false
            }
            '>' => {
                depth = depth.saturating_sub(1);
                false
            }
            _ => depth == 0 && !c.is_whitespace(),
        })
        .collect()
}

/// Emit API head/return constructor expressions in manifest order, without surrounding syntax.
/// `prefix` is the compiler-owned Rust module path preceding each manifest `with` function,
/// for example `mod_0::` in the native launcher or `original_mod::` in a wasm wrapper.
/// Call-site injections and internals listeners are emitted by their own slot routes.
pub fn api(manifest: &Manifest, points: &[Point], prefix: &str) -> Result<Vec<String>, String> {
    Ok(resolve(manifest, points)?
        .into_iter()
        .map(|(place, entry)| {
            format!(
                "mod_points::listeners::{place}({:?}, {}, {:?}, {prefix}{})",
                manifest.id, entry.priority, entry.id, entry.with
            )
        })
        .collect())
}

/// Place names selected by the same resolver as constructor emission. The wasm build passes
/// these to the points crate so it compiles only the guest record codecs and exports it uses.
pub fn places(manifest: &Manifest, points: &[Point]) -> Result<Vec<String>, String> {
    Ok(resolve(manifest, points)?
        .into_iter()
        .map(|(place, _)| place)
        .collect())
}

fn resolve<'a>(
    manifest: &'a Manifest,
    points: &[Point],
) -> Result<Vec<(String, &'a horizon_mod_package::manifest::inject::Inject)>, String> {
    manifest
        .inject
        .iter()
        .filter(|entry| entry.access == Access::Api && entry.call.is_none())
        .map(|entry| {
            if !matches!(
                entry.kind,
                Kind::Head | Kind::ModifyArg | Kind::Replace | Kind::Return
            ) {
                return Err(format!(
                    "{}: expected an API head/return injection",
                    entry.id
                ));
            }
            let point = points
                .iter()
                .find(|point| plain_path(&point.target) == plain_path(&entry.at))
                .ok_or_else(|| format!("{}: unknown API point {}", entry.id, entry.at))?;
            if point.private || entry.kind == Kind::Return && !point.at_return {
                return Err(format!("{}: no public API place at {}", entry.id, entry.at));
            }
            let place = if entry.kind == Kind::Return {
                format!("{}_return", point.name)
            } else {
                point.name.clone()
            };
            Ok((place, entry))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_and_wasm_share_the_existing_constructor_expressions() {
        let manifest = Manifest::parse(
            r#"
id = "fixture"
name = "Fixture"
version = "0.1.0"
api = "0.2"
capabilities = ["registries"]
[native]
game = "26.3"
horizon = "0000000000000000000000000000000000000000"
mixins = "0000000000000000000000000000000000000000"
[[inject]]
id = "before"
at = "horizon_client::game::Game<'_>::tick"
kind = "head"
with = "head"
access = "api"
priority = 12
hash = "pin"
[[inject]]
id = "after"
at = "horizon_client::game::Game::tick"
kind = "return"
with = "tail"
access = "api"
hash = "pin"
"#,
        )
        .unwrap();
        let points = [Point {
            name: "tick".into(),
            target: "horizon_client::game::Game < '_ > :: tick".into(),
            private: false,
            at_return: true,
        }];
        let native = api(&manifest, &points, "mod_3::")
            .unwrap()
            .iter()
            .map(|expression| format!("        {expression},\n"))
            .collect::<String>();
        assert_eq!(
            native,
            concat!(
                "        mod_points::listeners::tick(\"fixture\", 12, \"before\", mod_3::head),\n",
                "        mod_points::listeners::tick_return(\"fixture\", 0, \"after\", mod_3::tail),\n"
            )
        );
        for kind in [Kind::ModifyArg, Kind::Replace] {
            let mut alternate = manifest.clone();
            alternate.inject[0].kind = kind;
            assert_eq!(
                api(&alternate, &points, "mod_3::").unwrap()[0],
                api(&manifest, &points, "mod_3::").unwrap()[0]
            );
        }
        let wasm = api(&manifest, &points, "original_mod::").unwrap();
        assert_eq!(
            wasm[0],
            "mod_points::listeners::tick(\"fixture\", 12, \"before\", original_mod::head)"
        );
        let mut head_only = points[0].clone();
        head_only.at_return = false;
        assert!(api(&manifest, &[head_only], "mod_3::").is_err());
        assert!(api(&manifest, &[], "mod_3::").is_err());
    }
}
