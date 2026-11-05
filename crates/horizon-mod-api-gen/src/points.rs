//! Injection points (#589): every `Game` method [`point_shape::eligible`] admits is a point. The
//! staging pass ([`stage`]) puts `#[point]` on each in a staged copy of the game's source, the copy
//! the prebuilt game and the launcher's source builds compile; the generator lists the same points
//! from any copy, staged or not, as `mod_points` (each point's record, the `Points` trait,
//! `PointId`) and as `points.json`, the list the launcher routes injections by. All of them are
//! build outputs, never committed.

use quote::{ToTokens, quote};
use std::{
    fs,
    path::{Path, PathBuf},
};
use syn::{ImplItem, Item, Type};

/// One point, as `points.json` lists it.
#[derive(Debug, serde::Serialize)]
pub struct GamePoint {
    pub name: String,
    /// The function, as an injection's `target` (and a hook's) names it.
    pub target: String,
    pub side: &'static str,
    /// The listener's host: `host` (`&self`), or the side's writing host (`&mut self`).
    pub host: &'static str,
    /// The record its listeners get, in `mod_points`.
    pub record: String,
    /// The parameters passed, with their types, in order.
    pub args: Vec<(String, String)>,
    /// What its listeners may do with the return: `cancel`, `result` (an API-typed value they
    /// may set at the head or change at the return) or `none`.
    pub returns: &'static str,
    /// Whether it has a return place: not when the value returned borrows the game.
    pub at_return: bool,
    #[serde(skip)]
    record_item: String,
    /// Its `Points` methods, `PointId` variants, subscription consts and forwarding methods
    /// ([`point_shape::Shape::api_items`]).
    #[serde(skip)]
    api: [String; 4],
    /// Its record's shape, as a wasm mod's listener and the game check it
    /// ([`point_shape::Shape::wire_shape`]).
    #[serde(skip)]
    wire_shape: String,
}

/// The attribute the staging pass writes: every point fires on both sides, its listeners
/// matching on `HostSide` where it takes `&mut self`.
const ATTRIBUTE: &str = "side = \"both\"";

/// A source file of the game module, with its module path and the `cfg`s it is compiled under.
pub(crate) struct GameFile {
    pub(crate) module: String,
    pub(crate) path: PathBuf,
    /// The conditions of the `mod` declarations that lead to it, outermost first.
    pub(crate) cfg: Vec<String>,
}

/// The `cfg` conditions in `attrs`, as written, except `cfg(test)`.
pub(crate) fn conditions(attrs: &[syn::Attribute]) -> Vec<String> {
    attrs
        .iter()
        .filter(|attr| attr.path().is_ident("cfg") && !test_only(std::slice::from_ref(attr)))
        .filter_map(|attr| attr.parse_args::<syn::Meta>().ok())
        .map(|meta| meta.to_token_stream().to_string())
        .collect()
}

/// Whether `attrs` hold `#[cfg(test)]`: compiled only into tests.
pub(crate) fn test_only(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("cfg")
            && matches!(attr.parse_args::<syn::Meta>(), Ok(syn::Meta::Path(path)) if path.is_ident("test"))
    })
}

/// The files of `horizon_client::game`, following its `mod` declarations (and `#[path]`s, and
/// inline modules that declare files) from `game.rs`. With `every_build`, a module behind a `cfg`
/// is left out with everything under it (the points: functions every build compiles); without, it
/// is kept with its conditions (the internals, listed with the builds they exist in). Test-only
/// modules are always left out.
pub(crate) fn game_files(src: &Path, every_build: bool) -> Result<Vec<GameFile>, String> {
    let mut files = Vec::new();
    let mut pending = vec![(
        "horizon_client::game".to_owned(),
        src.join("game.rs"),
        src.join("game"),
        Vec::new(),
    )];
    while let Some((module, path, dir, cfg)) = pending.pop() {
        let source = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let file = syn::parse_file(&source).map_err(|e| format!("{}: {e}", path.display()))?;
        let here = path
            .parent()
            .expect("a source file has a folder")
            .to_owned();
        children(
            &module,
            &file.items,
            &here,
            &dir,
            &cfg,
            every_build,
            &mut pending,
        );
        files.push(GameFile { module, path, cfg });
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

/// A module file still to read: its module path, file, children's folder and conditions.
type Pending = (String, PathBuf, PathBuf, Vec<String>);

/// Queues the module files `items` declare, looking inside inline modules too: a file module
/// declared in `mod a { mod b; }` lives in `dir/a/b.rs`. A `#[path]` is relative to `here`.
fn children(
    module: &str,
    items: &[Item],
    here: &Path,
    dir: &Path,
    cfg: &[String],
    every_build: bool,
    pending: &mut Vec<Pending>,
) {
    for item in items {
        let Item::Mod(declared) = item else { continue };
        if test_only(&declared.attrs) || (every_build && gated(&declared.attrs)) {
            continue;
        }
        let name = declared.ident.to_string();
        let child = format!("{module}::{name}");
        let cfg: Vec<String> = cfg
            .iter()
            .cloned()
            .chain(conditions(&declared.attrs))
            .collect();
        let explicit = declared.attrs.iter().find_map(|attr| match &attr.meta {
            syn::Meta::NameValue(value) if attr.path().is_ident("path") => match &value.value {
                syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(path),
                    ..
                }) => Some(path.value()),
                _ => None,
            },
            _ => None,
        });
        if let Some((_, inner)) = &declared.content {
            // An inline module's children live in its folder, named by its own `#[path]` if any.
            let inner_dir = dir.join(explicit.as_deref().unwrap_or(&name));
            children(
                &child,
                inner,
                &inner_dir,
                &inner_dir,
                &cfg,
                every_build,
                pending,
            );
            continue;
        }
        let (file, child_dir) = match explicit {
            Some(relative) => {
                let file = here.join(relative);
                let dir = file.with_extension("");
                (file, dir)
            }
            None if dir.join(format!("{name}.rs")).is_file() => {
                (dir.join(format!("{name}.rs")), dir.join(&name))
            }
            None => (dir.join(&name).join("mod.rs"), dir.join(&name)),
        };
        pending.push((child, file, child_dir, cfg));
    }
}

/// The game module's files, for a build script to track.
pub fn tracked(src: &Path) -> Result<Vec<PathBuf>, String> {
    Ok(game_files(src, true)?
        .into_iter()
        .map(|file| file.path)
        .collect())
}

/// Whether `attrs` hold a `cfg` (or `cfg_attr`): the item is compiled only in some builds.
fn gated(attrs: &[syn::Attribute]) -> bool {
    attrs
        .iter()
        .any(|attr| attr.path().is_ident("cfg") || attr.path().is_ident("cfg_attr"))
}

/// The point methods of one parsed file: each with its module path, from the file's inherent
/// `impl Game` blocks and the inline modules every build compiles.
fn file_points<'a>(module: &str, items: &'a [Item], out: &mut Vec<(String, &'a syn::ImplItemFn)>) {
    for item in items {
        match item {
            Item::Mod(inline) if !gated(&inline.attrs) => {
                if let Some((_, inner)) = &inline.content {
                    file_points(&format!("{module}::{}", inline.ident), inner, out);
                }
            }
            Item::Impl(block) if block.trait_.is_none() && !gated(&block.attrs) => {
                let Type::Path(ty) = &*block.self_ty else {
                    continue;
                };
                if ty.qself.is_some()
                    || ty.path.segments.len() != 1
                    || ty.path.segments[0].ident != "Game"
                {
                    continue;
                }
                for member in &block.items {
                    if let ImplItem::Fn(method) = member
                        && point_shape::eligible(&method.attrs, &method.sig)
                    {
                        out.push((module.to_owned(), method));
                    }
                }
            }
            _ => {}
        }
    }
}

/// The game's points, by name: every eligible inherent `Game` method of the game module, whose
/// names Rust already keeps unique.
pub fn game_points(root: &Path) -> Result<Vec<GamePoint>, String> {
    let mut points = Vec::new();
    for file in game_files(&root.join("crates/horizon-client/src"), true)? {
        let source = fs::read_to_string(&file.path).map_err(|e| e.to_string())?;
        let parsed =
            syn::parse_file(&source).map_err(|e| format!("{}: {e}", file.path.display()))?;
        let mut methods = Vec::new();
        file_points(&file.module, &parsed.items, &mut methods);
        for (module, method) in methods {
            let target = format!("{module}::Game<'_>::{}", method.sig.ident);
            let shape = point_shape::shape(ATTRIBUTE.parse().expect("the attribute"), &method.sig)
                .map_err(|e| format!("{target}: {e}"))?;
            let api = shape.api_items(&target);
            points.push(GamePoint {
                record_item: shape.record_item(&target).to_string(),
                api: [
                    api.methods.to_string(),
                    api.variants.to_string(),
                    api.consts.to_string(),
                    api.forwards.to_string(),
                ],
                wire_shape: shape.wire_shape(),
                at_return: shape.has_return(),
                target,
                side: shape.side.name(),
                host: match (shape.mutable, shape.side) {
                    (false, _) => "host",
                    (true, side) => side.name(),
                },
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
                record: shape.record().to_string(),
                returns: match shape.returns {
                    point_shape::Returns::Unit | point_shape::Returns::UnitResult => "cancel",
                    point_shape::Returns::Value(_) => "result",
                    point_shape::Returns::Opaque => "none",
                },
                name: shape.name,
            });
        }
    }
    points.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(points)
}

/// What [`stage`] did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Staged {
    /// Points given a slot.
    pub points: usize,
    /// Of those, the ones with a head only (their return value borrows the game).
    pub head_only: usize,
    /// Files rewritten.
    pub files: usize,
}

/// The staging pass: puts `#[point]` on every point method of the staged copy at `root`, in
/// place. Run on a copy, never on a working tree: the source tree carries no `#[point]`.
pub fn stage(root: &Path) -> Result<Staged, String> {
    let mut staged = Staged::default();
    for file in game_files(&root.join("crates/horizon-client/src"), true)? {
        let source = fs::read_to_string(&file.path).map_err(|e| e.to_string())?;
        let parsed =
            syn::parse_file(&source).map_err(|e| format!("{}: {e}", file.path.display()))?;
        let mut methods = Vec::new();
        file_points(&file.module, &parsed.items, &mut methods);
        if methods.is_empty() {
            continue;
        }
        let lines: Vec<usize> = std::iter::once(0)
            .chain(source.match_indices('\n').map(|(i, _)| i + 1))
            .collect();
        let mut at: Vec<usize> = methods
            .iter()
            .map(|(_, method)| {
                if let Some(attr) = method.attrs.iter().find(|attr| {
                    attr.path()
                        .segments
                        .last()
                        .is_some_and(|segment| segment.ident == "point")
                }) {
                    return Err(format!(
                        "{}: {} already has {}: the source carries no #[point]",
                        file.path.display(),
                        method.sig.ident,
                        attr.to_token_stream()
                    ));
                }
                let start = match &method.vis {
                    syn::Visibility::Inherited => method.sig.fn_token.span.start(),
                    vis => syn::spanned::Spanned::span(vis).start(),
                };
                let line = lines[start.line - 1];
                Ok(source[line..]
                    .char_indices()
                    .nth(start.column)
                    .map_or(source.len(), |(i, _)| line + i))
            })
            .collect::<Result<_, String>>()?;
        staged.points += methods.len();
        staged.head_only += methods
            .iter()
            .filter(|(_, method)| {
                point_shape::shape(ATTRIBUTE.parse().expect("the attribute"), &method.sig)
                    .is_ok_and(|shape| !shape.has_return())
            })
            .count();
        at.sort_unstable();
        let mut out = String::with_capacity(source.len() + at.len() * 48);
        let mut from = 0;
        for offset in at {
            out.push_str(&source[from..offset]);
            out.push_str(&format!("#[horizon_mod_api_macros::point({ATTRIBUTE})] "));
            from = offset;
        }
        out.push_str(&source[from..]);
        fs::write(&file.path, out).map_err(|e| format!("{}: {e}", file.path.display()))?;
        staged.files += 1;
    }
    Ok(staged)
}

/// `mod_points`: each point's record, the type its listeners take, and the `Points` trait a
/// mod implements to listen, with its `PointId`s and subscription consts.
pub(crate) fn records(points: &[GamePoint]) -> String {
    let parse = |code: &str| {
        code.parse::<proc_macro2::TokenStream>()
            .expect("generated points")
    };
    let records = points.iter().map(|point| parse(&point.record_item));
    let methods = points.iter().map(|point| parse(&point.api[0]));
    let variants = points.iter().map(|point| parse(&point.api[1]));
    let consts = points.iter().map(|point| parse(&point.api[2]));
    let forwards = points.iter().map(|point| parse(&point.api[3]));
    // Each place's name, function and record shape, in `PointId` order: a point's head, then its
    // return.
    let places: Vec<(String, &str, &str)> = points
        .iter()
        .flat_map(|point| {
            let head = (point.name.clone(), &*point.target, &*point.wire_shape);
            let back = (
                format!("{}_return", point.name),
                &*point.target,
                &*point.wire_shape,
            );
            std::iter::once(head).chain(point.at_return.then_some(back))
        })
        .collect();
    let names = places.iter().map(|place| &place.0);
    let targets = places.iter().map(|place| place.1);
    let shapes = places.iter().map(|place| place.2);
    let ids = places.iter().map(|place| {
        proc_macro2::Ident::new(
            &point_shape::camel(&place.0),
            proc_macro2::Span::call_site(),
        )
    });
    quote! {
        #(#records)*
        /// A point's place: each point's head and return, in the order of [`Points`]' methods.
        #[doc(hidden)]
        #[repr(usize)]
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #[non_exhaustive]
        pub enum PointId { #(#variants)* __Count }
        /// The number of places, the length of the game's slot table.
        #[doc(hidden)]
        pub const POINT_COUNT: usize = PointId::__Count as usize;
        /// The game's injection points a mod listens to (#589): one method per point's head and
        /// return, each a no-op until the mod overrides it. Implement it with
        /// `#[mod_api::horizon_mod]` on the same type as the mod's `Hooks`, and name each method
        /// in a `[[native.injection]]` with the point's function as `target`.
        pub trait Points: Send + Sync {
            #[doc(hidden)]
            fn subscribed(&self) -> &'static [Option<PointId>];
            #(#methods)*
        }
        /// Each place's `PointId`, by its `Points` method's name: the subscriptions
        /// `#[horizon_mod]` derives.
        #[doc(hidden)]
        pub mod ids {
            use super::PointId;
            #(#consts)*
        }
        /// A wasm mod's listeners as the game's `Points`: every place forwarded with its record.
        #[cfg(all(feature = "wasm-host", not(target_arch = "wasm32")))]
        impl Points for crate::wire::Forwarded {
            fn subscribed(&self) -> &'static [Option<PointId>] {
                self.0.subscribed()
            }
            #(#forwards)*
        }
        /// Each place's name, function and record shape, by `PointId`: what the game checks a
        /// wasm mod's listeners against at load.
        #[cfg(all(feature = "wasm-host", not(target_arch = "wasm32")))]
        #[doc(hidden)]
        pub mod places {
            use super::{POINT_COUNT, PointId};
            /// Each place.
            pub static IDS: [PointId; POINT_COUNT] = [#(PointId::#ids),*];
            /// Each place's `Points` method.
            pub static NAMES: [&str; POINT_COUNT] = [#(#names),*];
            /// Each place's function, as an injection's `target` names it.
            pub static TARGETS: [&str; POINT_COUNT] = [#(#targets),*];
            /// Each place's record shape.
            pub static SHAPES: [&str; POINT_COUNT] = [#(#shapes),*];
        }
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

    fn eligible(method: &str) -> bool {
        let method: syn::ImplItemFn = syn::parse_str(method).unwrap();
        point_shape::eligible(&method.attrs, &method.sig)
    }

    #[test]
    fn a_point_passes_only_api_typed_parameters() {
        let shape = point(
            r#"side = "client""#,
            "fn turn(&mut self, mut yaw: f64, button: MouseButton, text: &str, event: &'static str, batch: &mut HudBatch) {}",
        )
        .unwrap();
        let args: Vec<_> = shape.args.iter().map(|arg| arg.name.to_string()).collect();
        assert_eq!(args, ["yaw", "text"]);
        assert!(matches!(shape.returns, point_shape::Returns::Unit));
        let item = shape.record_item("t").to_string();
        assert!(
            item.contains("pub yaw : f64")
                && item.contains("pub text : :: std :: borrow :: Cow < 'a , str >")
                && item.contains("pub cancel : bool"),
            "{item}"
        );
        let read = point(
            r#"side = "server", name = "reach_of""#,
            "fn reach(&self) -> f64 { 0.0 }",
        )
        .unwrap();
        assert_eq!(
            (read.name.as_str(), read.variant().to_string()),
            ("reach_of", "ReachOf".into())
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
        let at_return = |sig: &str| point(r#"side = "both""#, sig).unwrap().has_return();
        assert!(at_return("fn hit(&self) -> Option<(usize, f64)> {}"));
        assert!(!at_return(
            "fn slot(&mut self, i: u32) -> Option<&mut ItemStack> {}"
        ));
        assert!(!at_return("fn view(&self) -> View<'_> {}"));
        // A `'static` return borrows nothing from the receiver.
        assert!(at_return("fn jab(&self) -> Option<&'static str> {}"));
    }

    #[test]
    fn every_plain_game_method_is_a_point() {
        assert!(eligible("fn tick(&mut self) -> Result<()> {}"));
        assert!(eligible("pub(super) fn reach(&self) -> f64 { 0.0 }"));
        assert!(eligible("fn get<T: Copy>(&self) -> T { todo!() }"));
        for left_out in [
            "fn native_server_tick(&mut self) {}",
            "fn new() -> Self {}",
            "fn take(self) {}",
            "const fn size(&self) -> usize { 0 }",
            "async fn wait(&self) {}",
            "#[cfg(test)] fn probe(&self) {}",
            "#[inline(always)] fn hot(&self) {}",
            "fn each(&self, f: impl Fn()) {}",
            "fn iter(&self) -> impl Iterator<Item = u8> { todo!() }",
            "fn stop(&self) -> ! { loop {} }",
            "fn f(&mut self, cancel: bool) {}",
        ] {
            assert!(!eligible(left_out), "{left_out}");
        }
    }

    #[test]
    fn the_pass_marks_the_methods_the_generator_lists() {
        let root = std::env::temp_dir().join(format!("horizon-points-{}", std::process::id()));
        let src = root.join("crates/horizon-client/src");
        std::fs::create_dir_all(src.join("game/remote")).unwrap();
        std::fs::write(
            src.join("game.rs"),
            "mod remote;\nmod nested { mod leaf; }\n#[path = \"elsewhere\"]\nmod moved { mod deep; }\n#[cfg(test)]\nmod probes;\nimpl Game<'_> {\n    fn tick(&mut self) {}\n    pub(super) fn slot(&mut self) -> &mut u8 { todo!() }\n    fn native_tick(&mut self) {}\n}\n",
        )
        .unwrap();
        std::fs::write(
            src.join("game/remote.rs"),
            "mod lan;\nimpl Game<'_> { fn send(&self, id: u32) {} }\nimpl Other { fn send(&self) {} }\n",
        )
        .unwrap();
        std::fs::write(
            src.join("game/remote/lan.rs"),
            "impl Game<'_> { fn join(&mut self) {} }",
        )
        .unwrap();
        std::fs::create_dir_all(src.join("game/elsewhere")).unwrap();
        std::fs::write(
            src.join("game/elsewhere/deep.rs"),
            "impl Game<'_> { fn dive(&mut self) {} }",
        )
        .unwrap();
        std::fs::create_dir_all(src.join("game/nested")).unwrap();
        std::fs::write(
            src.join("game/nested/leaf.rs"),
            "impl Game<'_> { fn fall(&mut self) {} }",
        )
        .unwrap();
        std::fs::write(
            src.join("game/probes.rs"),
            "impl Game<'_> { fn probe(&self) {} }",
        )
        .unwrap();
        let points = game_points(&root).unwrap();
        let targets: Vec<_> = points.iter().map(|p| p.target.as_str()).collect();
        assert_eq!(
            targets,
            [
                "horizon_client::game::moved::deep::Game<'_>::dive",
                "horizon_client::game::nested::leaf::Game<'_>::fall",
                "horizon_client::game::remote::lan::Game<'_>::join",
                "horizon_client::game::remote::Game<'_>::send",
                "horizon_client::game::Game<'_>::slot",
                "horizon_client::game::Game<'_>::tick",
            ]
        );
        let api = records(&points);
        assert!(
            api.contains("fn tick_return") && !api.contains("fn slot_return"),
            "{api}"
        );
        assert_eq!(
            stage(&root).unwrap(),
            Staged {
                points: 6,
                head_only: 1,
                files: 5
            }
        );
        let game = std::fs::read_to_string(src.join("game.rs")).unwrap();
        assert!(
            game.contains("    #[horizon_mod_api_macros::point(side = \"both\")] fn tick(")
                && game.contains(
                    "#[horizon_mod_api_macros::point(side = \"both\")] pub(super) fn slot("
                )
                && !game.contains("point(side = \"both\")] fn native_tick"),
            "{game}"
        );
        // A second pass finds the attribute and refuses: the source tree never carries one.
        assert!(stage(&root).unwrap_err().contains("already has"));
        std::fs::remove_dir_all(&root).unwrap();
    }
}
