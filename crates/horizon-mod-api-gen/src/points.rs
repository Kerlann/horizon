//! Injection points (#589): the game's `#[point]` methods, read from its source like the hooks'
//! functions, as the slot statics the launcher fills (`points_generated.rs`) and the list it
//! routes injections by (`points.json`). Both are build outputs, never committed.
#[path = "../../horizon-mod-api-macros/src/point_shape.rs"]
mod point_shape;

use quote::{ToTokens, quote};
use std::{collections::BTreeMap, path::Path};

/// One point, as `points.json` lists it.
#[derive(Debug, serde::Serialize)]
pub struct GamePoint {
    pub name: String,
    /// The function, as an injection's `target` (and a hook's) names it.
    pub target: String,
    pub side: &'static str,
    /// The listener's host: `host` (`&self`), or the side's writing host (`&mut self`).
    pub host: &'static str,
    pub hot: bool,
    /// The record its listeners get, in `mod_api::points`.
    pub record: String,
    /// The parameters passed, with their types, in order.
    pub args: Vec<(String, String)>,
    /// What its listeners may do with the return: `cancel`, `result` (an API-typed value they
    /// may set at the head or change at the return) or `none`.
    pub returns: &'static str,
    #[serde(skip)]
    listener: String,
    #[serde(skip)]
    record_item: String,
    /// Its `Points` methods, `PointId` variants and dispatchers ([`point_shape::Shape::api_items`]).
    #[serde(skip)]
    api: [String; 3],
}

/// The `#[point]` methods of `Game`, in `game.rs` and under `game/`, by name. Two with one name
/// are refused, naming both: the second takes `#[point(name = "…")]`.
pub fn game_points(root: &Path) -> Result<Vec<GamePoint>, String> {
    let src = root.join("crates/horizon-client/src");
    let mut methods = Vec::new();
    super::file_game_methods(&src.join("game.rs"), "horizon_client::game", &mut methods)?;
    super::game_methods(&src.join("game"), "horizon_client::game", &mut methods)?;
    let mut points = BTreeMap::<String, GamePoint>::new();
    for (module, method) in methods {
        let Some(attribute) = method.attrs.iter().find(|attr| {
            attr.path()
                .segments
                .last()
                .is_some_and(|segment| segment.ident == "point")
        }) else {
            continue;
        };
        let tokens = match &attribute.meta {
            syn::Meta::List(list) => list.tokens.clone(),
            _ => Default::default(),
        };
        let target = format!("{module}::Game<'_>::{}", method.sig.ident);
        let shape =
            point_shape::shape(tokens, &method.sig).map_err(|e| format!("{target}: {e}"))?;
        let record_item = shape.record_item(&target).to_string();
        let api = shape.api_items(&target);
        let point = GamePoint {
            record_item,
            api: [api.0.to_string(), api.1.to_string(), api.2.to_string()],
            target,
            side: shape.side.name(),
            host: match (shape.mutable, shape.side) {
                (false, _) => "host",
                (true, side) => side.name(),
            },
            hot: shape.hot,
            args: shape
                .args
                .iter()
                .map(|arg| {
                    (
                        arg.name.to_string(),
                        arg.ty.to_token_stream().to_string().replace("& ", "&"),
                    )
                })
                .collect(),
            listener: shape.listener().to_string(),
            record: shape.record().to_string(),
            returns: match shape.returns {
                point_shape::Returns::Unit | point_shape::Returns::UnitResult => "cancel",
                point_shape::Returns::Value(_) => "result",
                point_shape::Returns::Opaque => "none",
            },
            name: shape.name,
        };
        if let Some(other) = points.get(&point.name) {
            return Err(format!(
                "{} and {} are both the point {:?}: give one #[point(name = \"…\")]",
                other.target, point.target, point.name
            ));
        }
        points.insert(point.name.clone(), point);
    }
    Ok(points.into_values().collect())
}

/// The game's `points` module: a private head and return slot per point, and `attach_installed`,
/// which fills each slot an installed mod subscribes to with `mod_api`'s dispatcher for it.
pub(crate) fn slots(points: &[GamePoint]) -> String {
    let ident = |name: String| syn::Ident::new(&name, proc_macro2::Span::call_site());
    let slots: Vec<_> = points
        .iter()
        .flat_map(|point| {
            let upper = point.name.to_uppercase();
            [
                (ident(upper.clone()), point, ident("Head".into())),
                (ident(upper + "_RETURN"), point, ident("Return".into())),
            ]
        })
        .collect();
    let statics = slots.iter().map(|(slot, point, at)| {
        let listener: proc_macro2::TokenStream =
            point.listener.parse().expect("generated listener type");
        let target = &point.target;
        quote!(pub(crate) static #slot: Point<#listener> = Point::new(#target, Place::#at);)
    });
    let attach = slots.iter().map(|(slot, point, at)| {
        let listener: proc_macro2::TokenStream =
            point.listener.parse().expect("generated listener type");
        let method = match at.to_string().as_str() {
            "Head" => point.name.clone(),
            _ => format!("{}_return", point.name),
        };
        let dispatcher = ident(format!("dispatch_{method}"));
        let method = ident(method);
        quote! {
            if mod_api::__private::point_listened(mod_api::points::#method) {
                let listener: #listener = mod_api::points::#dispatcher;
                #slot.attach(Box::new(vec![listener])).expect("a slot is filled once, with its own type");
            }
        }
        .to_string()
    });
    // One `if` per line: the slots attach independently, and tokens on one line read to clippy as
    // a missing `else`.
    let attach = attach.collect::<Vec<_>>().join("\n");
    let items = quote! {
        use mod_api::__private::point::{Attach, Place, Point};
        pub(crate) use mod_api::points::*;
        #(#statics)*
    };
    format!(
        "{items}\n/// Fills each slot an installed mod subscribes to, once, after the mods are installed.\npub(crate) fn attach_installed() {{\n{attach}\n}}\n"
    )
}

/// `mod_api::points`: each point's record, the type its listeners take, and the `Points` trait a
/// mod implements to listen, with its `PointId`s and the dispatchers the game's slots hold.
pub(crate) fn records(points: &[GamePoint]) -> String {
    let parse = |code: &str| {
        code.parse::<proc_macro2::TokenStream>()
            .expect("generated points")
    };
    let records = points.iter().map(|point| parse(&point.record_item));
    let methods = points.iter().map(|point| parse(&point.api[0]));
    let variants = points.iter().map(|point| parse(&point.api[1]));
    let dispatch = points.iter().map(|point| parse(&point.api[2]));
    let count = points.len() * 2;
    quote! {
        #(#records)*
        /// A point's place: each point's head and return, in the order of [`Points`]' methods.
        #[doc(hidden)]
        #[repr(usize)]
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #[non_exhaustive]
        pub enum PointId { #(#variants)* }
        pub(crate) const POINT_COUNT: usize = #count;
        /// The game's injection points a mod listens to (#589): one method per point's head and
        /// return, each a no-op until the mod overrides it. Implement it with
        /// `#[mod_api::horizon_mod]`, return it from [`Hooks::points`](crate::Hooks::points), and
        /// name each method in a `[[native.injection]]` with the point's function as `target`.
        pub trait Points: Send + Sync {
            #[doc(hidden)]
            fn subscribed(&self) -> &'static [Option<PointId>];
            #(#methods)*
        }
        #(#dispatch)*
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(attribute: &str, method: &str) -> Result<point_shape::Shape, String> {
        let method: syn::ImplItemFn = syn::parse_str(method).map_err(|e| e.to_string())?;
        point_shape::shape(attribute.parse().unwrap(), &method.sig).map_err(|e| e.to_string())
    }

    #[test]
    fn a_point_passes_only_api_typed_parameters() {
        let shape = point(
            r#"side = "client""#,
            "fn turn(&mut self, mut yaw: f64, button: MouseButton, text: &str, batch: &mut HudBatch) {}",
        )
        .unwrap();
        let args: Vec<_> = shape.args.iter().map(|arg| arg.name.to_string()).collect();
        assert_eq!(args, ["yaw", "text"]);
        assert_eq!(
            shape.listener().to_string(),
            quote!(fn(&mut dyn ::mod_api::ClientHost, &mut ::mod_api::points::TurnArgs<'_>))
                .to_string()
        );
        assert!(matches!(shape.returns, point_shape::Returns::Unit));
        let item = shape.record_item("t").to_string();
        assert!(
            item.contains("pub yaw : f64")
                && item.contains("pub text : & 'a str")
                && item.contains("pub cancel : bool"),
            "{item}"
        );
        let read = point(
            r#"side = "server", name = "reach_of""#,
            "fn reach(&self) -> f64 { 0.0 }",
        )
        .unwrap();
        assert_eq!(
            (read.name.as_str(), read.slot().to_string()),
            ("reach_of", "REACH_OF".into())
        );
        assert_eq!(
            read.listener().to_string(),
            quote!(fn(&dyn ::mod_api::Host, &mut ::mod_api::points::ReachOfArgs)).to_string()
        );
        assert!(
            read.record_item("t")
                .to_string()
                .contains("pub result : Option < f64 >")
        );
    }

    #[test]
    fn what_a_listener_may_do_follows_the_return_type() {
        let returns = |sig: &str| point(r#"side = "both""#, sig).map(|shape| shape.returns);
        assert!(matches!(
            returns("fn tick(&mut self) -> Result<()> {}"),
            Ok(point_shape::Returns::UnitResult)
        ));
        assert!(matches!(
            returns("fn hit(&self) -> bool {}"),
            Ok(point_shape::Returns::Value(_))
        ));
        assert!(matches!(
            returns("fn hit(&self) -> Option<(usize, f64)> {}"),
            Ok(point_shape::Returns::Opaque)
        ));
        assert!(
            point(r#"side = "both""#, "fn f(&mut self, cancel: bool) {}")
                .err()
                .expect("refused")
                .contains("rename")
        );
    }

    #[test]
    fn a_point_needs_a_side_and_a_game_receiver() {
        let refused = |attribute, method| point(attribute, method).err().expect("refused");
        assert!(refused("", "fn tick(&mut self) {}").contains("side"));
        assert!(refused(r#"side = "both""#, "fn free(x: u8) {}").contains("method"));
        assert!(
            refused(r#"side = "up""#, "fn tick(&mut self) {}").contains("server, client or both")
        );
    }

    #[test]
    fn two_points_with_one_name_are_refused_naming_both() {
        let root = std::env::temp_dir().join(format!("horizon-points-{}", std::process::id()));
        let src = root.join("crates/horizon-client/src");
        std::fs::create_dir_all(src.join("game")).unwrap();
        std::fs::write(
            src.join("game.rs"),
            "impl Game<'_> { #[mod_api::point(side = \"both\")] fn tick(&mut self) {} }",
        )
        .unwrap();
        std::fs::write(
            src.join("game/remote.rs"),
            "impl Game<'_> { #[point(side = \"client\")] fn tick(&mut self, input: u8) {} fn plain(&self) {} }",
        )
        .unwrap();
        let error = game_points(&root).unwrap_err();
        assert!(
            error.contains("horizon_client::game::Game<'_>::tick"),
            "{error}"
        );
        assert!(
            error.contains("horizon_client::game::remote::Game<'_>::tick"),
            "{error}"
        );
        std::fs::write(
            src.join("game/remote.rs"),
            "impl Game<'_> { #[point(side = \"client\", name = \"remote_tick\")] fn tick(&mut self, input: u8) {} }",
        )
        .unwrap();
        let points = game_points(&root).unwrap();
        let targets: Vec<_> = points
            .iter()
            .map(|p| (p.name.as_str(), p.target.as_str()))
            .collect();
        assert_eq!(
            targets,
            [
                (
                    "remote_tick",
                    "horizon_client::game::remote::Game<'_>::tick"
                ),
                ("tick", "horizon_client::game::Game<'_>::tick"),
            ]
        );
        let slots = slots(&points);
        assert!(slots.contains("attach_installed"));
        // Each slot attaches in its own statement: clippy reads `} if` on one line as a missing `else`.
        assert!(!slots.contains("} if"), "{slots}");
        let api = records(&points);
        assert!(
            api.contains("fn remote_tick_return") && api.contains("dispatch_tick"),
            "{api}"
        );
        std::fs::remove_dir_all(&root).unwrap();
    }
}
