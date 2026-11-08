//! Injection points (#589): every function of the game module that [`point_shape::eligible`]
//! admits is a point, and so is each function `slots/*.txt` lists in the crates below it. The staging
//! pass ([`stage`]) puts `#[point]` on each in a staged copy of the game's source, the copy the
//! prebuilt game and the launcher's source builds compile, with each raw form's types beside it;
//! the generator lists the same points from any copy, staged or not, as `mod_points` (each point's
//! record, typed function slots, `PointId`) and as `points.json`, the list the launcher routes
//! injections by. All of them are build outputs, never committed.

use quote::{ToTokens, quote};
use std::{
    fs,
    path::{Path, PathBuf},
};
use syn::{ImplItem, Item, Type};

pub mod source;

/// One point, as `points.json` lists it.
#[derive(Debug, serde::Serialize)]
pub struct GamePoint {
    /// Why the registration-policy reader cannot be generated, absent when it can.
    pub runtime_refusal: Option<String>,
    pub name: String,
    /// The function, as an injection's `target` names it.
    pub target: String,
    /// The side of a `Game` method's point, which picks its API listeners' host; `none` off `Game`.
    pub side: &'static str,
    /// The API listener's host: `host` (`&self`), the side's writing host (`&mut self`), or `none`.
    pub host: &'static str,
    /// The record its API listeners get, in `mod_points`.
    pub record: String,
    /// The parameters passed to API listeners, with their types, in order.
    pub args: Vec<(String, String)>,
    /// What its listeners may do with the return: `cancel`, `result` (an API-typed value they
    /// may set at the head or change at the return) or `none`.
    pub returns: &'static str,
    /// Whether its API listeners run at the return: not when the value returned borrows.
    pub at_return: bool,
    /// Whether it has a raw form: listeners with the receiver and every parameter by its own type.
    pub raw: bool,
    #[serde(skip)]
    snapshot: bool,
    /// Full raw listener contract, including engine-typed parameters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_shape: Option<point_shape::RawShape>,
    /// Its raw listeners' function type, by path: a mod's raw listener has this type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub listener: Option<String>,
    /// Its raw return listeners' function type, by path: they get the value returned.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_listener: Option<String>,
    #[serde(skip)]
    record_item: String,
    /// Its typed function constructors, place ids and dispatch functions
    /// ([`point_shape::Shape::api_items`]).
    #[serde(skip)]
    api: [String; 3],
}

/// A source file of the game's crates, with its module path and the `cfg`s it is compiled under.
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

/// Whether an item is compiled only into tests, explicitly or by `#[test]`/`#[bench]`.
pub(crate) fn test_only(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("test")
            || attr.path().is_ident("bench")
            || (attr.path().is_ident("cfg")
                && matches!(attr.parse_args::<syn::Meta>(), Ok(syn::Meta::Path(path)) if path.is_ident("test")))
    })
}

/// A module file still to read: its module path, file, children's folder and conditions.
type Pending = (String, PathBuf, PathBuf, Vec<String>);

/// The files of the module `module`, read from `path` (its children in `dir`), following `mod`
/// declarations (and `#[path]`s, and inline modules that declare files). With `every_build`, a
/// module behind a `cfg` is left out with everything under it (the points: functions every build
/// compiles); without, it is kept with its conditions (the internals, listed with the builds they
/// exist in). Test-only modules are always left out, including files gated by inner attributes.
pub(crate) fn module_files(
    module: String,
    path: PathBuf,
    dir: PathBuf,
    every_build: bool,
) -> Result<Vec<GameFile>, String> {
    let mut files = Vec::new();
    let mut pending = vec![(module, path, dir, Vec::new())];
    while let Some((module, path, dir, cfg)) = pending.pop() {
        // A declared module with no file is one another target builds (its file named by a
        // `cfg_attr` path this walk already followed): nothing here compiles from it.
        if !path.is_file() {
            continue;
        }
        let source = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let file = crate::game_modules::parse(&path, &source)?;
        if test_only(&file.attrs) || (every_build && gated(&file.attrs)) {
            continue;
        }
        let cfg = cfg
            .into_iter()
            .chain(conditions(&file.attrs))
            .collect::<Vec<_>>();
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
        if let Item::Macro(included) = item
            && included.mac.path.is_ident("include")
            && !test_only(&included.attrs)
            && !(every_build && gated(&included.attrs))
            && let Ok(relative) = syn::parse2::<syn::LitStr>(included.mac.tokens.clone())
        {
            pending.push((
                module.to_owned(),
                here.join(relative.value()),
                dir.to_owned(),
                cfg.to_vec(),
            ));
        }
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
        // `#[cfg_attr(unix, path = "a/unix.rs")]`: each target's file, under its condition.
        let per_target: Vec<(String, String)> = declared
            .attrs
            .iter()
            .filter(|attr| attr.path().is_ident("cfg_attr"))
            .filter_map(|attr| {
                let args = attr
                    .parse_args_with(
                        syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
                    )
                    .ok()?;
                let mut args = args.into_iter();
                let condition = args.next()?;
                let path = args.find_map(|meta| match meta {
                    syn::Meta::NameValue(value) if value.path.is_ident("path") => {
                        match value.value {
                            syn::Expr::Lit(syn::ExprLit {
                                lit: syn::Lit::Str(path),
                                ..
                            }) => Some(path.value()),
                            _ => None,
                        }
                    }
                    _ => None,
                })?;
                Some((condition.to_token_stream().to_string(), path))
            })
            .collect();
        if declared.content.is_none() && explicit.is_none() && !per_target.is_empty() {
            for (condition, relative) in per_target {
                let file = here.join(relative);
                let dir = file
                    .parent()
                    .expect("a module file has a folder")
                    .to_owned();
                let cfg = cfg.iter().cloned().chain([condition]).collect();
                pending.push((child.clone(), file, dir, cfg));
            }
            continue;
        }
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
                let dir = file
                    .parent()
                    .expect("a module file has a folder")
                    .to_owned();
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

/// The files of `horizon_client::game`; see [`module_files`] for `every_build`.
pub(crate) fn game_files(src: &Path, every_build: bool) -> Result<Vec<GameFile>, String> {
    module_files(
        "horizon_client::game".to_owned(),
        src.join("game.rs"),
        src.join("game"),
        every_build,
    )
}

/// The crate `krate`'s package folder and library root, from its manifest.
pub(crate) fn crate_root(root: &Path, krate: &str) -> Result<(PathBuf, PathBuf), String> {
    let package = root.join("crates").join(krate.replace('_', "-"));
    let manifest = package.join("Cargo.toml");
    let text = fs::read_to_string(&manifest).map_err(|e| format!("{}: {e}", manifest.display()))?;
    let manifest: toml::Value = toml::from_str(&text).map_err(|e| e.to_string())?;
    let lib = manifest
        .get("lib")
        .and_then(|lib| lib.get("path"))
        .and_then(toml::Value::as_str)
        .unwrap_or("src/lib.rs");
    let lib = package.join(lib);
    Ok((package, lib))
}

/// The files of the crate `krate` that every build compiles.
fn crate_files(root: &Path, krate: &str) -> Result<Vec<GameFile>, String> {
    let (_, lib) = crate_root(root, krate)?;
    let dir = lib.parent().expect("a source file has a folder").to_owned();
    module_files(krate.to_owned(), lib, dir, true)
}

/// The crates the points' own crate is built on (`horizon-mod-points` and everything under it), by
/// library name: their functions can't be points, since their slots would have to reach up to it.
pub(crate) fn below_points(root: &Path) -> Result<std::collections::BTreeSet<String>, String> {
    let mut seen = std::collections::BTreeSet::from(["horizon_mod_points".to_owned()]);
    let mut pending = vec![root.join("crates/horizon-mod-points")];
    while let Some(package) = pending.pop() {
        let manifest = package.join("Cargo.toml");
        let text =
            fs::read_to_string(&manifest).map_err(|e| format!("{}: {e}", manifest.display()))?;
        let manifest: toml::Value = toml::from_str(&text).map_err(|e| e.to_string())?;
        // Build dependencies too: `horizon-mod-points` is generated by `horizon-mod-api-gen`.
        for path in ["dependencies", "build-dependencies"]
            .into_iter()
            .filter_map(|kind| manifest.get(kind).and_then(toml::Value::as_table))
            .flatten()
            .filter_map(|(_, dep)| dep.get("path").and_then(toml::Value::as_str))
        {
            let dependency = package.join(path);
            let name = dependency
                .file_name()
                .map(|name| name.to_string_lossy().replace('-', "_"))
                .unwrap_or_default();
            if seen.insert(name) {
                pending.push(dependency);
            }
        }
    }
    Ok(seen)
}

/// Listed lower-crate functions and their diagnostic locations.
fn listed(root: &Path) -> Result<Vec<(String, String)>, String> {
    Ok(crate::sites::slots(&root.join(crate::sites::DIRECTORY))
        .map_err(|e| e.to_string())?
        .functions
        .into_iter()
        .map(|(path, origin)| (origin, path))
        .collect())
}

/// A path without generic arguments or spaces, as listed paths and found ones are compared.
pub(crate) fn plain_path(path: &str) -> String {
    let mut out = String::new();
    let mut depth = 0;
    for c in path.chars() {
        match c {
            '<' => depth += 1,
            '>' => depth -= 1,
            c if depth == 0 && !c.is_whitespace() => out.push(c),
            _ => {}
        }
    }
    out
}

/// The game module's files, slot lists and listed crates' files, for a build script to track.
pub fn tracked(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files: Vec<_> = game_files(&root.join("crates/horizon-client/src"), true)?
        .into_iter()
        .map(|file| file.path)
        .collect();
    let directory = root.join(crate::sites::DIRECTORY);
    files.push(directory.clone());
    if directory.exists() {
        files.extend(
            fs::read_dir(directory)
                .map_err(|e| e.to_string())?
                .map(|e| e.map(|e| e.path()))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?,
        );
    }
    let crates: std::collections::BTreeSet<_> = listed(root)?
        .into_iter()
        .filter_map(|(_, path)| path.split("::").next().map(str::to_owned))
        .collect();
    for krate in crates {
        files.extend(crate_files(root, &krate)?.into_iter().map(|file| file.path));
    }
    Ok(files)
}

/// Whether `attrs` hold a `cfg` (or `cfg_attr`): the item is compiled only in some builds.
fn gated(attrs: &[syn::Attribute]) -> bool {
    attrs
        .iter()
        .any(|attr| attr.path().is_ident("cfg") || attr.path().is_ident("cfg_attr"))
}

/// A point's function, as found in a parsed file.
pub(crate) struct Found<'a> {
    pub(crate) module: String,
    attrs: &'a [syn::Attribute],
    vis: &'a syn::Visibility,
    pub(crate) sig: &'a syn::Signature,
    pub(crate) body: &'a syn::Block,
    /// The `impl`'s type and generics; none for a free function.
    owner: Option<(&'a Type, &'a syn::Generics)>,
    /// Where the item that holds it ends: the `impl` block, or the function. Its raw types go there.
    pub(crate) end: proc_macro2::LineColumn,
}

impl Found<'_> {
    /// A method of `Game`, taking `self`: its point keeps the method's name and gets the game as
    /// its host. `Game`'s associated functions without `self` are named and hosted like any other
    /// type's.
    fn on_game(&self) -> bool {
        matches!(self.sig.inputs.first(), Some(syn::FnArg::Receiver(_)))
            && self.owner.is_some_and(|(ty, _)| matches!(ty, Type::Path(path) if path.qself.is_none() && path.path.segments.len() == 1 && path.path.segments[0].ident == "Game"))
    }

    /// Its path in its crate, as an injection's `target` names it.
    pub(crate) fn target(&self) -> String {
        match self.owner {
            Some((ty, _)) => format!("{}::{}::{}", self.module, owner(ty), self.sig.ident),
            None => format!("{}::{}", self.module, self.sig.ident),
        }
    }

    /// Its point's name: a `Game` method's own name; else its path from the game module (`game_…`),
    /// or in a lower crate from its crate's name less `horizon_`, snake-cased.
    fn name(&self) -> String {
        if self.on_game() {
            return unraw(&self.sig.ident);
        }
        let module = match self.module.strip_prefix("horizon_client::") {
            Some(rest) => rest.to_owned(),
            None => self
                .module
                .strip_prefix("horizon_")
                .unwrap_or(&self.module)
                .to_owned(),
        };
        let mut parts: Vec<String> = module
            .split("::")
            .filter(|part| !part.is_empty())
            .map(str::to_owned)
            .collect();
        if let Some((Type::Path(path), _)) = self.owner
            && let Some(last) = path.path.segments.last()
        {
            parts.push(snake(&unraw(&last.ident)));
        }
        parts.push(unraw(&self.sig.ident));
        parts.join("_")
    }

    /// The attribute the pass writes on it.
    fn attribute(&self) -> String {
        let mut parts = Vec::new();
        if self.on_game() {
            parts.push("side = \"both\"".to_owned());
        } else {
            parts.push(format!("name = \"{}\"", self.name()));
        }
        if self.raw() {
            parts.push("raw".into());
        }
        if self.owner.is_none() {
            parts.push("free".into());
        }
        parts.join(", ")
    }

    fn raw(&self) -> bool {
        point_shape::raw_able(self.sig, self.owner.map(|(_, generics)| generics))
    }

    fn shape(&self) -> Result<point_shape::Shape, String> {
        point_shape::shape(self.attribute().parse().expect("the attribute"), self.sig)
            .map_err(|e| format!("{}: {e}", self.target()))
    }
}

/// `r#type` as `type`.
pub(crate) fn unraw(ident: &syn::Ident) -> String {
    ident.to_string().trim_start_matches("r#").to_owned()
}

/// `MapData` as `map_data`.
fn snake(name: &str) -> String {
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// A type as a target names it: lifetimes as `'_`, no spaces.
fn owner(ty: &Type) -> String {
    struct EraseLifetimes;
    impl syn::visit_mut::VisitMut for EraseLifetimes {
        fn visit_lifetime_mut(&mut self, lifetime: &mut syn::Lifetime) {
            *lifetime = syn::Lifetime::new("'_", lifetime.span());
        }
    }
    let mut ty = ty.clone();
    syn::visit_mut::VisitMut::visit_type_mut(&mut EraseLifetimes, &mut ty);
    ty.to_token_stream().to_string().replace(' ', "")
}

/// The point functions of one parsed file, with their module paths: in the game module, every
/// eligible function of its inherent `impl`s (a `Game` method only with a `&self`/`&mut self`
/// receiver) and its free functions; elsewhere, those `keep` admits. Inline modules every build
/// compiles count too.
pub(crate) fn file_points<'a>(
    module: &str,
    items: &'a [Item],
    keep: &dyn Fn(&Found<'_>) -> bool,
    out: &mut Vec<Found<'a>>,
) {
    file_functions(module, items, keep, &point_shape::eligible, out);
}

pub(crate) fn file_functions<'a>(
    module: &str,
    items: &'a [Item],
    keep: &dyn Fn(&Found<'_>) -> bool,
    eligible: &dyn Fn(&[syn::Attribute], &syn::Signature) -> bool,
    out: &mut Vec<Found<'a>>,
) {
    for item in items {
        match item {
            Item::Mod(inline) if !gated(&inline.attrs) => {
                if let Some((_, inner)) = &inline.content {
                    file_functions(
                        &format!("{module}::{}", inline.ident),
                        inner,
                        keep,
                        eligible,
                        out,
                    );
                }
            }
            Item::Fn(function)
                if !gated(&function.attrs) && eligible(&function.attrs, &function.sig) =>
            {
                let found = Found {
                    module: module.to_owned(),
                    attrs: &function.attrs,
                    vis: &function.vis,
                    sig: &function.sig,
                    body: &function.block,
                    owner: None,
                    end: function.block.brace_token.span.close().end(),
                };
                if keep(&found) {
                    out.push(found);
                }
            }
            Item::Impl(block) if block.trait_.is_none() && !gated(&block.attrs) => {
                let Type::Path(ty) = &*block.self_ty else {
                    continue;
                };
                if ty.qself.is_some() {
                    continue;
                }
                for member in &block.items {
                    if let ImplItem::Fn(method) = member
                        && eligible(&method.attrs, &method.sig)
                    {
                        let found = Found {
                            module: module.to_owned(),
                            attrs: &method.attrs,
                            vis: &method.vis,
                            sig: &method.sig,
                            body: &method.block,
                            owner: Some((&block.self_ty, &block.generics)),
                            end: block.brace_token.span.close().end(),
                        };
                        if keep(&found) {
                            out.push(found);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

/// A parsed source file of the game's crates, with its points.
struct Scanned {
    path: PathBuf,
    module: String,
    source: String,
    parsed: syn::File,
    /// The crate's package, when it is a lower crate: the pass gives it the points' dependencies.
    package: Option<PathBuf>,
}

/// Every file with points: the game module's, then each listed lower crate's. Fails on a listed
/// path that names no point function, or a crate the points' own crate is built on.
fn scan(root: &Path) -> Result<(Vec<Scanned>, Vec<(String, String)>), String> {
    let mut files = Vec::new();
    for file in game_files(&root.join("crates/horizon-client/src"), true)? {
        files.push((file, None));
    }
    let list = listed(root)?;
    let below = below_points(root)?;
    let mut crates = std::collections::BTreeSet::new();
    for (origin, path) in &list {
        let krate = path.split("::").next().unwrap_or_default();
        if below.contains(krate) || krate == "horizon_client" {
            return Err(format!(
                "{origin}: {path}: {krate} is built under the points' own crate (or is the game's): its functions can't be listed points"
            ));
        }
        crates.insert(krate.to_owned());
    }
    for krate in &crates {
        let (package, _) = crate_root(root, krate)?;
        for file in crate_files(root, krate)? {
            files.push((file, Some(package.clone())));
        }
    }
    let mut scanned = Vec::new();
    for (file, package) in files {
        let source = fs::read_to_string(&file.path).map_err(|e| e.to_string())?;
        let parsed =
            syn::parse_file(&source).map_err(|e| format!("{}: {e}", file.path.display()))?;
        scanned.push(Scanned {
            path: file.path,
            module: file.module,
            source,
            parsed,
            package,
        });
    }
    Ok((scanned, list))
}

impl Scanned {
    /// This file's points: all of a game-module file's, or the listed ones of a lower crate's.
    fn points<'a>(&'a self, list: &[(String, String)], used: &mut Vec<bool>) -> Vec<Found<'a>> {
        let mut out = Vec::new();
        let used = std::cell::RefCell::new(used);
        let keep = |found: &Found<'_>| {
            if self.package.is_none() {
                return true;
            }
            let target = plain_path(&found.target());
            match list.iter().position(|(_, path)| *path == target) {
                Some(index) => {
                    used.borrow_mut()[index] = true;
                    true
                }
                None => false,
            }
        };
        file_points(&self.module, &self.parsed.items, &keep, &mut out);
        out
    }
}

/// Every point, scanned once: each with its file's index.
fn all_points<'a>(
    scanned: &'a [Scanned],
    list: &[(String, String)],
) -> Result<Vec<(usize, Found<'a>)>, String> {
    let mut used = vec![false; list.len()];
    let mut points = Vec::new();
    for (index, file) in scanned.iter().enumerate() {
        points.extend(
            file.points(list, &mut used)
                .into_iter()
                .map(|found| (index, found)),
        );
    }
    if let Some(((origin, path), _)) = list.iter().zip(&used).find(|(_, used)| !**used) {
        return Err(format!(
            "{origin}: {path}: no function every build compiles that can be a point"
        ));
    }
    let mut names = std::collections::BTreeMap::new();
    for (_, found) in &points {
        let shape = found.shape()?;
        for variant in [shape.variant(), shape.return_variant()] {
            if let Some(other) = names.insert(variant.to_string(), found.target()) {
                return Err(format!(
                    "the points of {other} and {} have one name, {}: rename one",
                    found.target(),
                    shape.name
                ));
            }
        }
    }
    Ok(points)
}

type Calls = std::collections::BTreeMap<String, Result<crate::native_raw::Call, String>>;

/// What each crate owning points cannot name in the graph, by layout (see `native_crate`).
type Blocked = std::collections::BTreeMap<String, Vec<Option<usize>>>;

/// One ordered graph over the whole catalog, before any name selection, and each raw point's
/// virtual call layouts in it. Guest and native generators share these descriptor IDs. A point
/// reaching a layout its own crate cannot name is refused.
fn catalog_graph(
    root: &Path,
    rule: &crate::crossing::Rule,
    ordered: &[(usize, Found<'_>)],
) -> Result<(crate::native_policy::Graph, Calls, Blocked), String> {
    let mut graph = crate::native_policy::Graph::default();
    let mut calls = Calls::new();
    for (_, found) in ordered {
        let shape = found.shape()?;
        if shape.raw {
            calls.insert(
                found.target(),
                crate::native_raw::register(
                    rule,
                    &found.module,
                    found.owner.map(|(ty, _)| ty),
                    found.sig,
                    &shape,
                    &mut graph,
                ),
            );
        }
    }
    let crates: std::collections::BTreeSet<String> = ordered
        .iter()
        .map(|(_, found)| crate::native_crate::owner(&found.module).to_owned())
        .chain(["horizon_client".to_owned()])
        .collect();
    let blocked: Blocked = crate::native_crate::read(root, &crates)?
        .into_iter()
        .map(|(krate, names)| (krate, names.blocked(rule, &graph)))
        .collect();
    for (_, found) in ordered {
        let krate = crate::native_crate::owner(&found.module);
        if let Some(Ok(call)) = calls.get(&found.target()) {
            let layouts: Vec<usize> = std::iter::once(call.head).chain(call.tail).collect();
            if let Some(reason) =
                crate::native_crate::refusal(&graph, &blocked[krate], krate, &layouts)
            {
                calls.insert(found.target(), Err(reason));
            }
        }
    }
    Ok((graph, calls, blocked))
}

/// The game's points, by name: every point function of the game module, and the listed ones of
/// the crates below it. Staged private source targets are never part of this public list.
pub fn game_points(root: &Path) -> Result<Vec<GamePoint>, String> {
    let private = source::read(root)?
        .into_iter()
        .map(|point| plain_path(&point.target))
        .collect::<std::collections::BTreeSet<_>>();
    let (scanned, list) = scan(root)?;
    let rule = crate::crossing::Rule::new(&crate::internals::rule_catalog(root)?);
    let mut points = Vec::new();
    let ordered = all_points(&scanned, &list)?;
    let (graph, calls, _) = catalog_graph(root, &rule, &ordered)?;
    for (_, found) in ordered {
        let target = found.target();
        if private.contains(&plain_path(&target)) {
            continue;
        }
        let shape = found.shape()?;
        let runtime = calls.get(&target).map(|call| {
            call.clone().and_then(|call| {
                crate::native_raw::generate_runtime(
                    &rule,
                    &found.module,
                    found.owner.map(|(ty, _)| ty),
                    found.sig,
                    &shape,
                    &graph,
                    call,
                )
                .map(|_| ())
            })
        });
        let admitted = matches!(runtime, Some(Ok(())));
        let api = shape.api_items(&target);
        points.push(GamePoint {
            record_item: shape.record_item(&target).to_string(),
            api: [
                api.listeners.to_string(),
                api.variants.to_string(),
                api.dispatch.to_string(),
            ],
            at_return: shape.has_return(),
            raw: shape.raw,
            snapshot: shape.snapshot_able(found.sig)
                || admitted
                || (shape.raw
                    && crate::native_raw::generate(&rule, &found.module, found.sig, &shape)
                        .is_ok()),
            runtime_refusal: runtime.and_then(Result::err),
            raw_shape: shape
                .raw
                .then(|| point_shape::raw_shape(&shape, found.sig, found.owner.map(|(ty, _)| ty))),
            listener: shape
                .raw
                .then(|| format!("{}::{}", found.module, shape.raw_listener())),
            return_listener: shape
                .raw
                .then(|| format!("{}::{}", found.module, shape.raw_return_listener())),
            target,
            side: shape.side.map_or("none", point_shape::Side::name),
            host: match (shape.receiver, shape.side) {
                (_, None) => "none",
                (point_shape::Receiver::Mut, Some(side)) => side.name(),
                (_, Some(_)) => "host",
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
    points.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(points)
}

/// What [`stage`] did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Staged {
    /// Points given a slot.
    pub points: usize,
    /// Of those, the ones whose API listeners get no return place (their value borrows).
    pub head_only: usize,
    /// Of those, the ones with a raw form.
    pub raw: usize,
    /// Files rewritten.
    pub files: usize,
}

/// The byte offset of `at` in `source`, whose line starts are `lines`.
pub(crate) fn offset(source: &str, lines: &[usize], at: proc_macro2::LineColumn) -> usize {
    let line = lines[at.line - 1];
    source[line..]
        .char_indices()
        .nth(at.column)
        .map_or(source.len(), |(i, _)| line + i)
}

/// The game crate root's no-op the source tree carries; staging gives it the catalog install.
const INSTALL_STUB: &str = "pub fn install_points_catalog() {}";

/// The head call layout of every raw point's head and return places, in `PointId` terms, and the
/// installer the staged game runs before the wasm host initializes. Both phases of a point share
/// the plan its head call names.
fn install_items(points: &[(usize, Found<'_>)], calls: &Calls) -> proc_macro2::TokenStream {
    let mut pairs = Vec::new();
    for (_, found) in points {
        let Some(Ok(call)) = calls.get(&found.target()) else {
            continue;
        };
        let head = call.head;
        let name = found.name();
        for place in [name.clone(), format!("{name}_return")] {
            let variant = quote::format_ident!("{}", point_shape::camel(&place));
            pairs.push(quote::quote!((mod_points::PointId::#variant, #head)));
        }
    }
    quote::quote! {
        /// Each raw place and the head call layout its staged plan is keyed by.
        pub fn head_calls() -> Vec<(mod_points::PointId, usize)> {
            vec![#(#pairs),*]
        }
        /// Gives the wasm host this catalog, to resolve manifest internals against.
        pub fn install() {
            horizon_mod_wasm::install_catalog(horizon_mod_wasm::InternalsCatalog {
                schema: catalog(),
                head_calls: head_calls(),
            });
        }
    }
}

/// The once-per-source readers, planners and decoders, and the pure schema catalog the loader
/// resolves manifest declarations against, appended to the game crate's root.
fn emit_runtime_module(
    root: &Path,
    rule: &crate::crossing::Rule,
    entries: &[crate::internals::Internal],
    graph: &crate::native_policy::Graph,
    points: &[(usize, Found<'_>)],
    calls: &Calls,
    blocked: &Blocked,
) -> Result<(), String> {
    let layouts = graph.tokens();
    let mut specs = Vec::new();
    for (_, found) in points {
        let Some(Ok(call)) = calls.get(&found.target()) else {
            continue;
        };
        let head = call.head;
        let tail = match call.tail {
            Some(tail) => quote::quote!(Some(#tail)),
            None => quote::quote!(None),
        };
        let receiver = match found.owner {
            Some((ty, _)) => graph
                .id(&rule.shape(&found.module, None, ty))
                .map_or(quote::quote!(None), |id| quote::quote!(Some(#id))),
            None => quote::quote!(None),
        };
        specs.push(quote::quote!(mod_points::endpoint_policy::PointSpec { head_call: #head, tail_call: #tail, receiver: #receiver }));
    }
    let mut fields = Vec::new();
    for entry in entries.iter().filter(|entry| entry.kind == "field") {
        let Some((owner, field)) = entry.path.rsplit_once("::") else {
            continue;
        };
        let Some(receiver) = graph.layouts.iter().position(|layout| layout.name == owner) else {
            continue;
        };
        let (path, hash) = (&entry.path, &entry.hash);
        fields.push(quote::quote!(mod_points::endpoint_policy::InternalField { path: String::from(#path), kind: String::from("field"), hash: String::from(#hash), receiver: #receiver, field: String::from(#field) }));
    }
    let install = install_items(points, calls);
    let catalog = quote::quote! {
        #install
        /// The exact catalog the generated readers were emitted from.
        pub fn catalog() -> mod_points::endpoint_policy::SchemaCatalog {
            mod_points::endpoint_policy::SchemaCatalog {
                layouts: #layouts,
                points: vec![#(#specs),*],
                fields: vec![#(#fields),*],
            }
        }
    };
    // One helper module per crate that owns admitted points, and the game's, which also holds the
    // catalog. Each emits the layouts its own points reach, spelled from its own crate.
    let mut crates = std::collections::BTreeMap::<&str, Vec<usize>>::new();
    crates.entry("horizon_client").or_default();
    for (_, found) in points {
        if let Some(Ok(call)) = calls.get(&found.target()) {
            crates
                .entry(crate::native_crate::owner(&found.module))
                .or_default()
                .extend(std::iter::once(call.head).chain(call.tail));
        }
    }
    for (krate, calls) in crates {
        let emit = crate::native_crate::emitted(rule, graph, &blocked[krate], &calls);
        let windows = crate::native_window::helpers(rule, graph, &emit)?;
        let patches = crate::native_patch::helpers(rule, graph, &emit)?;
        let (catalog, alias) = if krate == "horizon_client" {
            (catalog.clone(), quote::quote!())
        } else {
            // A lower crate names its own layouts by the catalog's paths, as its dependents do.
            let name = quote::format_ident!("{krate}");
            (
                quote::quote!(),
                quote::quote!(#[allow(unused_extern_crates)] extern crate self as #name;),
            )
        };
        let module = quote::quote! {
            #alias
            #[doc(hidden)]
            #[allow(dead_code, clippy::all)]
            pub mod __horizon_points {
                #windows
                #patches
                #catalog
            }
        };
        let (_, lib) = crate_root(root, krate)?;
        let mut source = fs::read_to_string(&lib).map_err(|e| format!("{}: {e}", lib.display()))?;
        if krate == "horizon_client" {
            if !source.contains(INSTALL_STUB) {
                return Err(format!("{}: no `{INSTALL_STUB}` to stage", lib.display()));
            }
            source = source.replacen(
                INSTALL_STUB,
                "pub fn install_points_catalog() { __horizon_points::install() }",
                1,
            );
        }
        source.push('\n');
        source.push_str(&module.to_string());
        fs::write(&lib, source).map_err(|e| format!("{}: {e}", lib.display()))?;
    }
    Ok(())
}

/// The staging pass: puts `#[point]` on every point function of the staged copy at `root`, in
/// place, declares each raw form's record and listener types after the item that holds the
/// function, and gives each lower crate with points the points' dependencies. Run on a copy, never
/// on a working tree: the source tree carries no `#[point]`.
pub fn stage(root: &Path) -> Result<Staged, String> {
    let (scanned, list) = scan(root)?;
    let entries = crate::internals::rule_catalog(root)?;
    let rule = crate::crossing::Rule::new(&entries);
    let points = all_points(&scanned, &list)?;
    let (graph, calls, blocked) = catalog_graph(root, &rule, &points)?;
    let mut staged = Staged::default();
    let mut packages = std::collections::BTreeSet::new();
    for (index, file) in scanned.iter().enumerate() {
        let here: Vec<_> = points
            .iter()
            .filter(|(i, _)| *i == index)
            .map(|(_, found)| found)
            .collect();
        if here.is_empty() {
            continue;
        }
        let source = &file.source;
        let lines: Vec<usize> = std::iter::once(0)
            .chain(source.match_indices('\n').map(|(i, _)| i + 1))
            .collect();
        // Insertions at byte offsets, in order; at one offset, in the order pushed.
        let mut edits: Vec<(usize, String)> = Vec::new();
        for found in &here {
            if let Some(attr) = found.attrs.iter().find(|attr| {
                attr.path()
                    .segments
                    .last()
                    .is_some_and(|segment| segment.ident == "point")
            }) {
                return Err(format!(
                    "{}: {} already has {}: the source carries no #[point]",
                    file.path.display(),
                    found.sig.ident,
                    attr.to_token_stream()
                ));
            }
            let start = match found.vis {
                syn::Visibility::Inherited => found
                    .sig
                    .constness
                    .map(|token| token.span)
                    .unwrap_or(found.sig.fn_token.span)
                    .start(),
                vis => syn::spanned::Spanned::span(vis).start(),
            };
            edits.push((
                offset(source, &lines, start),
                format!(
                    "#[horizon_mod_api_macros::point({})]{} ",
                    found.attribute(),
                    if found.shape()?.raw && !found.shape()?.snapshot_able(found.sig) {
                        calls
                            .get(&found.target())
                            .cloned()
                            .and_then(Result::ok)
                            .and_then(|call| {
                                crate::native_raw::generate_runtime(
                                    &rule,
                                    &found.module,
                                    found.owner.map(|(ty, _)| ty),
                                    found.sig,
                                    &found.shape().ok()?,
                                    &graph,
                                    call,
                                )
                                .ok()
                            })
                            .map(|plan| format!(" {}", plan.attribute()))
                            .unwrap_or_default()
                    } else {
                        String::new()
                    },
                ),
            ));
            let shape = found.shape()?;
            staged.points += 1;
            staged.head_only += usize::from(!shape.has_return());
            if shape.raw {
                staged.raw += 1;
                let items = shape.raw_items(
                    found.sig,
                    found.owner.map(|(ty, _)| ty),
                    found.owner.map(|(_, generics)| generics),
                );
                edits.push((offset(source, &lines, found.end), format!("\n{items}\n")));
            }
        }
        edits.sort_by_key(|(at, _)| *at);
        let mut out = String::with_capacity(
            source.len() + edits.iter().map(|(_, text)| text.len()).sum::<usize>(),
        );
        let mut from = 0;
        for (at, text) in edits {
            out.push_str(&source[from..at]);
            out.push_str(&text);
            from = at;
        }
        out.push_str(&source[from..]);
        fs::write(&file.path, out).map_err(|e| format!("{}: {e}", file.path.display()))?;
        staged.files += 1;
        if let Some(package) = &file.package {
            packages.insert(package.clone());
        }
    }
    emit_runtime_module(root, &rule, &entries, &graph, &points, &calls, &blocked)?;
    dependencies(root, packages)?;
    Ok(staged)
}

/// Give listed lower-crate points and sites their staged dependencies, including the lock file.
pub(crate) fn dependencies(
    root: &Path,
    packages: std::collections::BTreeSet<PathBuf>,
) -> Result<(), String> {
    const ADDED: [&str; 3] = [
        "horizon-mod-points",
        "horizon-mod-api-macros",
        "horizon-mod-api",
    ];
    if packages.is_empty() {
        return Ok(());
    }
    let lock_path = root.join("Cargo.lock");
    // A Windows checkout has CRLF line endings; Cargo reads either, and the entries match on LF.
    let mut lock = fs::read_to_string(&lock_path)
        .map_err(|e| format!("{}: {e}", lock_path.display()))?
        .replace("\r\n", "\n");
    for package in packages {
        let manifest = package.join("Cargo.toml");
        let text =
            fs::read_to_string(&manifest).map_err(|e| format!("{}: {e}", manifest.display()))?;
        let mut cargo: toml::Table = toml::from_str(&text).map_err(|e| e.to_string())?;
        let dependencies = cargo
            .entry("dependencies")
            .or_insert_with(|| toml::Value::Table(toml::Table::new()))
            .as_table_mut()
            .ok_or("[dependencies] is not a table")?;
        // Every package with points sits in `crates/`, beside these two.
        for name in ADDED {
            dependencies.entry(name).or_insert_with(|| {
                toml::Value::Table(toml::Table::from_iter([(
                    "path".to_owned(),
                    format!("../{name}").into(),
                )]))
            });
        }
        let name = cargo
            .get("package")
            .and_then(|package| package.get("name"))
            .and_then(toml::Value::as_str)
            .ok_or_else(|| format!("{}: no package name", manifest.display()))?
            .to_owned();
        lock = locked_with(&lock, &name, &ADDED)
            .ok_or_else(|| format!("Cargo.lock has no entry for {name}"))?;
        fs::write(
            &manifest,
            toml::to_string(&cargo).map_err(|e| e.to_string())?,
        )
        .map_err(|e| format!("{}: {e}", manifest.display()))?;
    }
    fs::write(&lock_path, lock).map_err(|e| format!("{}: {e}", lock_path.display()))?;
    Ok(())
}

/// `lock` with `added` in the dependency list of the package `name`, kept in the lock file's
/// sorted order; none when the lock file has no entry for it.
fn locked_with(lock: &str, name: &str, added: &[&str]) -> Option<String> {
    let header = format!("[[package]]\nname = \"{name}\"\n");
    let start = lock.find(&header)?;
    let end = lock[start + header.len()..]
        .find("\n\n")
        .map_or(lock.len(), |at| start + header.len() + at);
    let entry = &lock[start..end];
    let mut lines: Vec<String> = entry.lines().map(str::to_owned).collect();
    let mut dependencies: Vec<String> = Vec::new();
    let open = lines.iter().position(|line| line == "dependencies = [");
    if let Some(open) = open {
        let close = open + lines[open..].iter().position(|line| line == "]")?;
        dependencies = lines.drain(open + 1..close).collect();
    }
    for name in added {
        let quoted = format!(" \"{name}\",");
        if !dependencies.contains(&quoted) {
            dependencies.push(quoted);
        }
    }
    dependencies.sort();
    let open = match open {
        Some(open) => open,
        None => {
            // `dependencies` follows `version` and `source`, before `checksum`.
            let at = lines
                .iter()
                .position(|line| line.starts_with("checksum"))
                .unwrap_or(lines.len());
            lines.insert(at, "dependencies = [".to_owned());
            lines.insert(at + 1, "]".to_owned());
            at
        }
    };
    for (offset, dependency) in dependencies.into_iter().enumerate() {
        lines.insert(open + 1 + offset, dependency);
    }
    Some(format!(
        "{}{}{}",
        &lock[..start],
        lines.join("\n"),
        &lock[end..]
    ))
}

/// `mod_points`: records, typed function constructors, dispatchers and place ids.
#[cfg(test)]
pub(crate) fn records(points: &[GamePoint], sources: &[source::Point]) -> String {
    records_selected(points, sources, None)
}

pub(crate) fn records_selected(
    points: &[GamePoint],
    sources: &[source::Point],
    selected: Option<&std::collections::BTreeSet<String>>,
) -> String {
    let enabled = |point: &GamePoint| {
        selected.is_none_or(|names| {
            names.contains(&point.name) || names.contains(&format!("{}_return", point.name))
        })
    };
    let parse = |code: &str| {
        code.parse::<proc_macro2::TokenStream>()
            .expect("generated points")
    };
    let records = points.iter().map(|point| {
        if enabled(point) { return parse(&point.record_item); }
        let mut file: syn::File = syn::parse_str(&point.record_item).expect("point record");
        file.items.retain(|item| !matches!(item, syn::Item::Impl(imp) if imp.trait_.as_ref().is_some_and(|(_, path, _)| path.segments.last().is_some_and(|segment| segment.ident == "Wire"))));
        quote!(#file)
    });
    let listeners = points.iter().map(|point| parse(&point.api[0]));
    let variants = points.iter().map(|point| parse(&point.api[1]));
    let dispatch = points.iter().map(|point| parse(&point.api[2]));
    let mut guest_functions = proc_macro2::TokenStream::new();
    let mut guest_entries = proc_macro2::TokenStream::new();
    let mut raw_entries = proc_macro2::TokenStream::new();
    let mut raw_places = proc_macro2::TokenStream::new();
    let mut raw_listeners = proc_macro2::TokenStream::new();
    let mut api_places = proc_macro2::TokenStream::new();
    let mut layout = 0xcbf29ce484222325u64;
    let snapshot = syn::parse_file(include_str!("../../horizon-mod-points/src/snapshot.rs"))
        .expect("snapshot key schema");
    let rows = syn::parse_file(include_str!("../../horizon-mod-api/src/row_pages.rs"))
        .expect("snapshot row schema");
    let patch = syn::parse_file(include_str!("../../horizon-mod-api/src/wire_patch.rs"))
        .expect("snapshot patch schema");
    for item in snapshot
        .items
        .into_iter()
        .chain(rows.items)
        .chain(patch.items)
    {
        let schema = match item {
            syn::Item::Type(alias)
                if matches!(
                    alias.ident.to_string().as_str(),
                    "Row" | "Payload" | "CellBytes"
                ) =>
            {
                alias.ty.to_token_stream().to_string()
            }
            syn::Item::Fn(function) if function.sig.ident == "key" => {
                function.block.to_token_stream().to_string()
            }
            _ => continue,
        };
        for byte in schema.bytes() {
            layout = (layout ^ u64::from(byte)).wrapping_mul(0x100000001b3);
        }
    }
    for point in points {
        for byte in format!(
            "{}:{}:{}:{}:{}:{:?}",
            point.name,
            point.host,
            point.record_item,
            point.at_return,
            point.snapshot,
            point.raw_shape
        )
        .bytes()
        {
            layout = (layout ^ u64::from(byte)).wrapping_mul(0x100000001b3);
        }
        let record = quote::format_ident!("{}", point.record);
        let passed = match point.host {
            "host" => quote!(&host,),
            "server" | "client" => quote!(&mut host,),
            "both" => quote!(if server {
                ::mod_api::HostSide::Server(&mut host)
            } else {
                ::mod_api::HostSide::Client(&mut host)
            },),
            _ => quote!(),
        };
        let function = quote::format_ident!("point_{}", point.name);
        let listener = quote::format_ident!("{}Listener", point_shape::camel(&point.name));
        let side = (point.host == "both").then(|| quote!(let server: bool = input.take()?;));
        if enabled(point) {
            guest_functions.extend(quote! {
            fn #function(place: PointId, input: &mut ::mod_api::wire::Reader<'_>, out: &mut ::mod_api::wire::Writer) -> Result<(), ::mod_api::wire::WireError> {
                let listener: u32 = input.take()?;
                #side
                let mut args: #record = input.take()?;
                let mut host = ::mod_api::wire::guest::Guest;
                crate::guest::fire(place, listener, false, |function| {
                    // SAFETY: head and return have the same checked host and record type.
                    let function = unsafe { ::std::mem::transmute::<*const (), listeners::#listener>(function) };
                    function(#passed &mut args);
                })?;
                out.put(&());
                out.put(&args);
                Ok(())
            }
        });
        }
        for name in std::iter::once(point.name.clone())
            .chain((point.at_return || point.snapshot).then(|| format!("{}_return", point.name)))
        {
            let variant = quote::format_ident!("{}", point_shape::camel(&name));
            let target = &point.target;
            let at_return = name.ends_with("_return") && name != point.name;
            // A borrowed return has a raw return place only: the API has no such listener.
            let raw_only = at_return && !point.at_return;
            if !raw_only {
                api_places.extend(quote! { (PointId::#variant, #target, #at_return), });
            }
            if point.snapshot {
                raw_places.extend(quote! { (PointId::#variant, #target, #at_return), });
                if enabled(point) {
                    let constructor = quote::format_ident!("{name}");
                    raw_listeners.extend(quote! {
                        pub const fn #constructor(injection: &'static str, function: Listener) -> crate::guest::RawListener {
                            // SAFETY: Listener checks the common owned-view and sparse-edit signature.
                            unsafe { crate::guest::RawListener::new(PointId::#variant, injection, function as *const ()) }
                        }
                    });
                }
            }
            if selected.is_some_and(|names| !names.contains(&name)) {
                continue;
            }
            if point.snapshot {
                raw_entries.extend(quote! { table[PointId::#variant as usize] = Some((PointId::#variant, raw_dispatch as Dispatch)); });
            }
            if !raw_only {
                guest_entries.extend(quote! {
                    table[PointId::#variant as usize] = Some((PointId::#variant, #function as Dispatch));
                });
            }
        }
    }
    let private_variants = sources.iter().map(|point| {
        let head = quote::format_ident!("{}", point_shape::camel(&point.name));
        let back = quote::format_ident!("{}Return", point_shape::camel(&point.name));
        quote!(#head, #back,)
    });
    quote! {
        #(#records)*
        /// The generated point export table and record layout, checked separately from the API layout.
        #[doc(hidden)]
        pub const POINT_LAYOUT: u64 = #layout;
        /// API places as (place, target function, return phase), for manifest validation.
        #[doc(hidden)]
        pub const API_PLACES: &[(PointId, &str, bool)] = &[#api_places];
        /// Raw places whose staged slots have snapshot readers.
        #[doc(hidden)]
        pub const RAW_PLACES: &[(PointId, &str, bool)] = &[#raw_places];
        #[cfg(any(target_arch = "wasm32", feature = "wasm-guest"))]
        #[allow(missing_docs)]
        pub mod raw_listeners {
            use super::*;
            pub type Listener = fn(crate::snapshot::Snapshot<'_>, &mut ::mod_api::wire::RecordPatch);
            #raw_listeners
        }
        #[cfg(any(target_arch = "wasm32", feature = "wasm-guest"))]
        #[doc(hidden)]
        pub fn guest_dispatch(id: u32, input: &mut ::mod_api::wire::Reader<'_>, out: &mut ::mod_api::wire::Writer) -> Result<(), ::mod_api::wire::WireError> {
            if id == 0xBFFF_FFFF { out.put(&crate::guest::subscriptions()); return Ok(()); }
            let raw = id & 0xC000_0000 == 0xC000_0000;
            let index = (id & 0x3FFF_FFFF) as usize;
            let table = if raw { &guest_exports::RAW_TABLE } else { &guest_exports::TABLE };
            let (place, function) = table.get(index).copied().flatten().ok_or(::mod_api::wire::WireError::Tag(id))?;
            function(place, input, out)
        }
        #[cfg(any(target_arch = "wasm32", feature = "wasm-guest"))]
        #[allow(unused_mut, unused_variables)]
        mod guest_exports {
            use super::*;
            type Dispatch = fn(PointId, &mut ::mod_api::wire::Reader<'_>, &mut ::mod_api::wire::Writer) -> Result<(), ::mod_api::wire::WireError>;
            pub(super) const TABLE: [Option<(PointId, Dispatch)>; POINT_COUNT] = {
                let mut table = [None; POINT_COUNT];
                #guest_entries
                table
            };
            pub(super) const RAW_TABLE: [Option<(PointId, Dispatch)>; POINT_COUNT] = {
                let mut table = [None; POINT_COUNT]; #raw_entries table
            };
            fn raw_dispatch(place: PointId, input: &mut ::mod_api::wire::Reader<'_>, out: &mut ::mod_api::wire::Writer) -> Result<(), ::mod_api::wire::WireError> {
                let listener: u32 = input.take()?;
                let (rows, pages, mut patch): crate::snapshot::Payload = input.take()?;
                crate::guest::fire(place, listener, true, |function| {
                    // SAFETY: every raw constructor checks this one shared signature.
                    let function = unsafe { ::std::mem::transmute::<*const (), raw_listeners::Listener>(function) };
                    function(crate::snapshot::Snapshot::with_pages(&rows, pages.as_ref()), &mut patch);
                })?;
                out.put(&()); out.put(&patch); Ok(())
            }
            #guest_functions
        }
        /// A point's place: its head and return in catalog order.
        #[doc(hidden)]
        #[repr(usize)]
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #[non_exhaustive]
        pub enum PointId { #(#variants)* #(#private_variants)* __Count }
        /// The number of places, the length of the game's slot table.
        #[doc(hidden)]
        pub const POINT_COUNT: usize = PointId::__Count as usize;
        /// Typed API listener aliases and constructors, generated from the point catalog.
        pub mod listeners { use super::*; #(#listeners)* }
        /// The game's typed dispatch, not part of the API a mod is written against.
        #[doc(hidden)]
        pub mod dispatch { use super::*; #(#dispatch)* }
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_guest_exports_keep_the_full_layout() {
        let make = |name: &str| {
            let sig: syn::Signature = syn::parse_str(&format!("fn {name}()")).unwrap();
            let shape = point_shape::shape(quote!(), &sig).unwrap();
            let api = shape.api_items(name);
            GamePoint {
                runtime_refusal: None,
                name: name.into(),
                target: name.into(),
                side: "none",
                host: "none",
                record: shape.record().to_string(),
                args: vec![],
                returns: "cancel",
                at_return: true,
                raw: false,
                snapshot: false,
                raw_shape: None,
                listener: None,
                return_listener: None,
                record_item: shape.record_item(name).to_string(),
                api: [
                    api.listeners.to_string(),
                    api.variants.to_string(),
                    api.dispatch.to_string(),
                ],
            }
        };
        let points = [make("first"), make("second")];
        let selected = std::collections::BTreeSet::from(["first_return".to_string()]);
        let all = records(&points, &[]);
        let selected = records_selected(&points, &[], Some(&selected));
        let layout = |source: &str| {
            syn::parse_file(source)
                .unwrap()
                .items
                .into_iter()
                .find_map(|item| match item {
                    syn::Item::Const(c) if c.ident == "POINT_LAYOUT" => {
                        Some(c.expr.to_token_stream().to_string())
                    }
                    _ => None,
                })
                .unwrap()
        };
        assert_eq!(layout(&all), layout(&selected));
        let file = syn::parse_file(&selected).unwrap();
        let wires = file.items.iter().filter(|item| matches!(item, syn::Item::Impl(imp) if imp.trait_.as_ref().is_some_and(|(_, path, _)| path.segments.last().unwrap().ident == "Wire"))).count();
        assert_eq!(wires, 1);
        assert!(selected.contains("fn point_first"));
        assert!(!selected.contains("fn point_second"));
        assert!(selected.contains("table [PointId :: FirstReturn as usize]"));
        assert!(!selected.contains("table [PointId :: First as usize]"));
    }

    #[test]
    fn raw_point_inventory() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let points = game_points(&root).unwrap();
        let mut inventory = Vec::new();
        for point in &points {
            assert_eq!(point.raw_shape.is_some(), point.raw, "{}", point.target);
            for at_return in [false, true] {
                if at_return && !point.at_return && !point.raw {
                    continue;
                }
                inventory.push(serde_json::json!({
                    "name": if at_return { format!("{}_return", point.name) } else { point.name.clone() },
                    "target": point.target,
                    "api": !at_return || point.at_return,
                    "raw_shape": point.raw_shape,
                    "raw_status": if point.snapshot { "generated primitive rows" } else if point.raw { "staged nominal reader needed" } else { "no raw place" },
                    "raw_snapshot": point.snapshot,
                    "runtime_refusal": point.runtime_refusal,
                }));
            }
        }
        if let Ok(path) = std::env::var("POINT_INVENTORY") {
            fs::write(path, serde_json::to_vec_pretty(&inventory).unwrap()).unwrap();
        }
        println!(
            "{} functions; {} places; {} raw functions; {} generated raw snapshot functions",
            points.len(),
            inventory.len(),
            points.iter().filter(|point| point.raw).count(),
            points.iter().filter(|point| point.snapshot).count()
        );
        let mut refusals = std::collections::BTreeMap::<String, usize>::new();
        for point in points.iter().filter(|point| point.raw) {
            let reason = point
                .runtime_refusal
                .as_deref()
                .map_or("admitted", |reason| &reason[..reason.len().min(110)]);
            *refusals.entry(reason.to_owned()).or_default() += 1;
        }
        let mut lower = std::collections::BTreeMap::<(&str, &str), usize>::new();
        for point in points
            .iter()
            .filter(|point| point.raw && !point.target.starts_with("horizon_client::"))
        {
            let krate = crate::native_crate::owner(&point.target);
            let reason = point.runtime_refusal.as_deref().unwrap_or("admitted");
            *lower.entry((krate, reason)).or_default() += 1;
        }
        for ((krate, reason), count) in lower {
            println!("lower crate {krate}: {count} {reason}");
        }
        let mut refusals: Vec<_> = refusals.into_iter().collect();
        refusals.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
        for (reason, count) in refusals.iter().take(40) {
            println!("runtime admission {count}: {reason}");
        }
        {
            // Trait-object arguments that cross as method tables, by trait, with how many of
            // the trait's methods cross; the rest stay opaque leaves.
            let rule = crate::crossing::Rule::new(&crate::internals::rule_catalog(&root).unwrap());
            let mut tables = std::collections::BTreeMap::<String, usize>::new();
            let mut leaves = std::collections::BTreeMap::<String, usize>::new();
            for point in &points {
                // `module::Owner<..>::method` or `module::function`.
                let segments: Vec<_> = point.target.split("::").collect();
                let owned = segments
                    .iter()
                    .position(|s| s.contains('<') || s.starts_with(char::is_uppercase));
                let module = segments[..owned.unwrap_or(segments.len() - 1)].join("::");
                for arg in point.raw_shape.iter().flat_map(|shape| &shape.args) {
                    if arg.class != "unclassified" {
                        continue;
                    }
                    let ty: syn::Type = syn::parse_str(&arg.ty).unwrap();
                    match rule.trait_object(&module, &ty) {
                        Some(path) => *tables.entry(path).or_default() += 1,
                        None => *leaves.entry(arg.ty.clone()).or_default() += 1,
                    }
                }
            }
            for (path, count) in &tables {
                let methods = crate::native_callable::methods(&rule, path);
                let crossing = methods
                    .iter()
                    .filter(|m| {
                        crate::native_callable::method_gap(&rule, path, &m.name.to_string())
                            .is_none()
                    })
                    .count();
                println!(
                    "trait table {count} args: {path} ({crossing}/{} methods cross)",
                    methods.len()
                );
            }
            for (ty, count) in &leaves {
                println!("trait object leaf {count} args: {ty}");
            }
        }
        for class in ["plain", "callback", "unclassified"] {
            let functions = points
                .iter()
                .filter_map(|p| p.raw_shape.as_ref())
                .filter(|shape| shape.args.iter().any(|arg| arg.class == class))
                .count();
            let arguments = points
                .iter()
                .filter_map(|p| p.raw_shape.as_ref())
                .flat_map(|shape| &shape.args)
                .filter(|arg| arg.class == class)
                .count();
            println!("argument class {class}: {functions} functions; {arguments} arguments");
            if class != "plain" {
                let mut admitted = 0;
                let mut refused = std::collections::BTreeMap::<&str, usize>::new();
                for point in points.iter().filter(|point| {
                    point
                        .raw_shape
                        .as_ref()
                        .is_some_and(|shape| shape.args.iter().any(|arg| arg.class == class))
                }) {
                    match point.runtime_refusal.as_deref() {
                        None => admitted += 1,
                        Some(reason) => *refused.entry(reason).or_default() += 1,
                    }
                }
                println!("argument class {class} as opaque leaf: {admitted} admitted functions");
                for (reason, count) in refused {
                    println!("argument class {class} refused {count}: {reason}");
                }
            }
        }
        for class in ["plain", "borrowed_return", "callback", "unclassified"] {
            let functions = points
                .iter()
                .filter_map(|p| p.raw_shape.as_ref())
                .filter(|shape| shape.return_class == class)
                .count();
            println!("return class {class}: {functions} functions");
        }
    }

    #[test]
    fn record_mutability_catalog_proof() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let points = game_points(&root).unwrap();
        let sources = source::read(&root).unwrap();
        let full = serde_json::to_vec_pretty(&points).unwrap();
        if let Ok(path) = std::env::var("RETURN_MUTABILITY_FULL") {
            fs::write(path, &full).unwrap();
        }
        let serialized: serde_json::Value = serde_json::from_slice(&full).unwrap();
        assert_eq!(serialized, serde_json::to_value(&points).unwrap());
        for point in serialized.as_array().unwrap() {
            if let Some(raw) = point.get("raw_shape") {
                assert!(raw["return_record_mutable"].is_boolean());
                if raw["return_ty"] != "()" {
                    assert_eq!(raw["return_record_mutable"], true);
                }
            }
        }
        let text = std::str::from_utf8(&full).unwrap();
        for raw in text.split("\"raw_shape\":").skip(1) {
            assert!(
                raw.find("\"return_record_mutable\"").unwrap() < raw.find("\"return_ty\"").unwrap()
            );
            assert!(raw.find("\"return_ty\"").unwrap() < raw.find("\"return_class\"").unwrap());
        }
        let mut existing = serde_json::to_value(&points).unwrap();
        let mut added = 0;
        for point in existing.as_array_mut().unwrap() {
            if let Some(raw) = point.get_mut("raw_shape") {
                assert!(raw.get("return_record_mutable").is_some());
                raw.as_object_mut().unwrap().remove("return_record_mutable");
                for arg in raw["args"].as_array_mut().unwrap() {
                    assert_eq!(arg["record_mutable"].as_bool(), Some(true));
                    added += usize::from(
                        arg.as_object_mut()
                            .unwrap()
                            .remove("record_mutable")
                            .is_some(),
                    );
                }
                assert_eq!(
                    raw.get("outcome_record_mutable"),
                    (raw["outcome"] != "none").then_some(&serde_json::Value::Bool(true)),
                );
                added += usize::from(
                    raw.as_object_mut()
                        .unwrap()
                        .remove("outcome_record_mutable")
                        .is_some(),
                );
            }
        }
        let existing = serde_json::to_vec_pretty(&existing).unwrap();
        let proof = serde_json::to_vec_pretty(&serde_json::json!({
            "existing_fields": String::from_utf8(existing.clone()).unwrap(),
            "records_and_shapes": records(&points, &sources),
        }))
        .unwrap();
        if let Ok(path) = std::env::var("RECORD_MUTABILITY_BASELINE") {
            assert_eq!(
                proof,
                fs::read(path).unwrap(),
                "existing points fields or records/SHAPES changed"
            );
        }
        if let Ok(path) = std::env::var("RECORD_MUTABILITY_PROOF") {
            fs::write(path, proof).unwrap();
        }
        println!(
            "points={} raw={} added_flags={added} bytes={} existing_bytes={} growth={}",
            points.len(),
            points.iter().filter(|p| p.raw).count(),
            full.len(),
            existing.len(),
            full.len() - existing.len()
        );
    }

    #[test]
    fn raw_shape_catalog_proof() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut points = game_points(&root).unwrap();
        let mut existing = serde_json::to_value(&points).unwrap();
        let mut counts = std::collections::BTreeMap::<String, usize>::new();
        for (point, json) in points.iter().zip(existing.as_array_mut().unwrap()) {
            if point.raw {
                assert!(json.get("raw_shape").is_some(), "{}", point.target);
                if let Some(raw) = json.get("raw_shape") {
                    assert!(raw["args"].is_array());
                    for class in ["plain", "callback", "unclassified"] {
                        if raw["args"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .any(|arg| arg["class"] == class)
                        {
                            *counts.entry(class.into()).or_default() += 1;
                            if class == "callback" {
                                println!("callback {}: {}", point.target, raw["args"]);
                            }
                        }
                    }
                    if raw["return_class"] == "borrowed_return" {
                        *counts.entry("borrowed_return".into()).or_default() += 1;
                    }
                }
            } else {
                assert!(json.get("raw_shape").is_none());
            }
            json.as_object_mut().unwrap().remove("raw_shape");
        }
        let sources = source::read(&root).unwrap();
        let proof = serde_json::json!({
            "existing_fields": existing,
            "records": records(&points, &sources),
        });
        let before_records = records(&points, &sources);
        let before_json = serde_json::to_vec(&existing).unwrap();
        for point in &mut points {
            point.raw_shape = None;
        }
        let mut after = serde_json::to_value(&points).unwrap();
        for json in after.as_array_mut().unwrap() {
            json.as_object_mut().unwrap().remove("raw_shape");
        }
        assert_eq!(serde_json::to_vec(&after).unwrap(), before_json);
        // Raw shape metadata participates in the layout fingerprint, but does not change
        // the records or dispatch code emitted for these same points.
        let split_layout = |code: &str| {
            let mut file = syn::parse_file(code).unwrap();
            let index = file
                .items
                .iter()
                .position(|item| {
                    matches!(item,
                        syn::Item::Const(item) if item.ident == "POINT_LAYOUT"
                    )
                })
                .unwrap();
            let layout = file.items.remove(index).to_token_stream().to_string();
            (layout, file.to_token_stream().to_string())
        };
        let (before_layout, before_code) = split_layout(&before_records);
        let (after_layout, after_code) = split_layout(&records(&points, &sources));
        assert_ne!(before_layout, after_layout);
        assert_eq!(before_code, after_code);
        if let Ok(path) = std::env::var("RAW_SHAPE_BASELINE") {
            let baseline = fs::read(path).unwrap();
            assert!(
                serde_json::to_vec_pretty(&proof).unwrap() == baseline,
                "existing fields or generated records/SHAPES changed"
            );
        }
        if let Ok(path) = std::env::var("RAW_SHAPE_PROOF") {
            fs::write(path, serde_json::to_vec_pretty(&proof).unwrap()).unwrap();
        }
        println!(
            "points={} raw={} classes={counts:?}",
            points.len(),
            points.iter().filter(|p| p.raw).count()
        );
    }

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
        // An associated or free function is a point too; off `Game`, with no host.
        assert!(eligible("fn new() -> Self {}"));
        for left_out in [
            "fn native_server_tick(&mut self) {}",
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
    fn the_pass_marks_the_functions_the_generator_lists() {
        let root = std::env::temp_dir().join(format!("horizon-points-{}", std::process::id()));
        let put = |path: &str, text: &str| {
            let path = root.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        };
        put("crates/horizon-mod-api-gen/internals.txt", "");
        put(
            "crates/horizon-client/Cargo.toml",
            "[package]\nname = 'horizon-client'\n",
        );
        put(
            "crates/horizon-client/src/lib.rs",
            "mod game; mod entry;\npub fn install_points_catalog() {}\n",
        );
        put("crates/horizon-client/src/entry.rs", "");
        put(
            "crates/horizon-client/src/game.rs",
            "mod remote;\nmod nested { mod leaf; }\n#[path = \"elsewhere\"]\nmod moved { mod deep; }\n#[cfg(test)]\nmod probes;\nimpl Game<'_> {\n    fn tick(&mut self) {}\n    pub(super) fn slot(&mut self) -> &mut u8 { todo!() }\n    fn native_tick(&mut self) {}\n    fn new() -> Self { todo!() }\n}\n",
        );
        put(
            "crates/horizon-client/src/game/remote.rs",
            "mod lan;\nimpl Game<'_> { fn send(&self, id: u32) {} }\nimpl Other { fn send(&self) {} }\nfn decode<T>(bytes: &[u8]) -> T { todo!() }\n",
        );
        put(
            "crates/horizon-client/src/game/remote/lan.rs",
            "impl Game<'_> { fn join(&mut self) {} }",
        );
        put(
            "crates/horizon-client/src/game/elsewhere/deep.rs",
            "impl Game<'_> { fn dive(&mut self) {} }",
        );
        put(
            "crates/horizon-client/src/game/nested/leaf.rs",
            "impl Game<'_> { fn fall(&mut self) {} }",
        );
        put(
            "crates/horizon-client/src/game/probes.rs",
            "impl Game<'_> { fn probe(&self) {} }",
        );
        put(
            "crates/horizon-mod-points/Cargo.toml",
            "[package]\nname = 'horizon-mod-points'\n[dependencies]\nhorizon-world = { path = '../horizon-world' }\n[build-dependencies]\nhorizon-mod-api-gen = { path = '../horizon-mod-api-gen' }\n",
        );
        put(
            "crates/horizon-world/Cargo.toml",
            "[package]\nname = 'horizon-world'\n",
        );
        put(
            "crates/horizon-mod-api-gen/Cargo.toml",
            "[package]\nname = 'horizon-mod-api-gen'\n",
        );
        put(
            "crates/horizon-client-render/Cargo.toml",
            "[package]\nname = 'horizon-client-render'\n[dependencies]\nlog = '0.4'\n",
        );
        put(
            "Cargo.lock",
            // CRLF, as a Windows checkout writes it.
            &"version = 4\n\n[[package]]\nname = \"horizon-client-render\"\nversion = \"0.1.0\"\ndependencies = [\n \"log\",\n]\n\n[[package]]\nname = \"log\"\nversion = \"0.4.0\"\n"
                .replace('\n', "\r\n"),
        );
        put(
            "crates/horizon-client-render/src/lib.rs",
            "pub mod mesher;\n",
        );
        put(
            "crates/horizon-client-render/src/mesher.rs",
            "pub struct Mesher<'a>(&'a u8);\nimpl Mesher<'_> { pub fn mesh(&mut self, chunk: &Chunk) -> usize { 0 } fn face(&self) {} }\n",
        );
        put(
            "crates/horizon-mod-api-gen/slots/horizon.txt",
            "# lower points\nhorizon_client_render::mesher::Mesher<'_>::mesh\n",
        );
        let points = game_points(&root).unwrap();
        let named: Vec<_> = points
            .iter()
            .map(|p| (p.name.as_str(), p.target.as_str(), p.side, p.raw))
            .collect();
        assert_eq!(
            named,
            [
                (
                    "client_render_mesher_mesher_mesh",
                    "horizon_client_render::mesher::Mesher<'_>::mesh",
                    "none",
                    true
                ),
                (
                    "dive",
                    "horizon_client::game::moved::deep::Game<'_>::dive",
                    "both",
                    true
                ),
                (
                    "fall",
                    "horizon_client::game::nested::leaf::Game<'_>::fall",
                    "both",
                    true
                ),
                (
                    "game_game_new",
                    "horizon_client::game::Game<'_>::new",
                    "none",
                    true
                ),
                (
                    "game_remote_decode",
                    "horizon_client::game::remote::decode",
                    "none",
                    false
                ),
                (
                    "game_remote_other_send",
                    "horizon_client::game::remote::Other::send",
                    "none",
                    true
                ),
                (
                    "join",
                    "horizon_client::game::remote::lan::Game<'_>::join",
                    "both",
                    true
                ),
                (
                    "send",
                    "horizon_client::game::remote::Game<'_>::send",
                    "both",
                    true
                ),
                ("slot", "horizon_client::game::Game<'_>::slot", "both", true),
                ("tick", "horizon_client::game::Game<'_>::tick", "both", true),
            ]
        );
        let api = records(&points, &[]);
        assert!(
            api.contains("fn tick_return")
                && api.contains("fn slot_return")
                && api.contains("SlotReturn")
                && api.contains("fn game_remote_other_send (args"),
            "{api}"
        );
        assert_eq!(
            stage(&root).unwrap(),
            Staged {
                points: 10,
                head_only: 1,
                raw: 9,
                files: 6
            }
        );
        let game = std::fs::read_to_string(root.join("crates/horizon-client/src/game.rs")).unwrap();
        let parsed = syn::parse_file(&game).unwrap();
        for name in ["tick", "slot"] {
            let method = parsed
                .items
                .iter()
                .filter_map(|item| match item {
                    syn::Item::Impl(item) => Some(&item.items),
                    _ => None,
                })
                .flatten()
                .find_map(|item| match item {
                    syn::ImplItem::Fn(method) if method.sig.ident == name => Some(method),
                    _ => None,
                })
                .unwrap();
            assert!(
                method
                    .attrs
                    .iter()
                    .any(|attr| attr.to_token_stream().to_string()
                        == "# [horizon_mod_api_macros :: point (side = \"both\" , raw)]")
            );
            assert!(
                method
                    .attrs
                    .iter()
                    .any(|attr| attr.path().is_ident("horizon_raw_plan"))
            );
        }
        assert!(
            game.contains("fn tick(")
                && game.contains("pub(super) fn slot(")
                && !game.contains("raw)] fn native_tick")
                // `Game`'s associated functions are named and hosted like any other type's.
                && game.contains("point(name = \"game_game_new\"")
                && game.contains("pub type __TickListener")
                && game.contains("pub type __SlotReturnListener"),
            "{game}"
        );
        let remote =
            std::fs::read_to_string(root.join("crates/horizon-client/src/game/remote.rs")).unwrap();
        assert!(
            remote.contains("point(name = \"game_remote_decode\", free)] fn decode")
                && remote.contains("pub type __GameRemoteOtherSendListener"),
            "{remote}"
        );
        let render = root.join("crates/horizon-client-render");
        let mesher = std::fs::read_to_string(render.join("src/mesher.rs")).unwrap();
        assert!(
            mesher.contains("point(name = \"client_render_mesher_mesher_mesh\", raw)]")
                && mesher.contains("pub fn mesh")
                && !mesher.contains("client_render_mesher_mesher_face")
                && mesher.contains("& 'r mut Mesher < '__self0 >"),
            "{mesher}"
        );
        let manifest = std::fs::read_to_string(render.join("Cargo.toml")).unwrap();
        assert!(
            manifest.contains("horizon-mod-points") && manifest.contains("../horizon-mod-points"),
            "{manifest}"
        );
        // The lock file lists them too, in order, so a `--locked` build accepts the manifest.
        let lock = std::fs::read_to_string(root.join("Cargo.lock")).unwrap();
        assert!(
            lock.contains("name = \"horizon-client-render\"\nversion = \"0.1.0\"\ndependencies = [\n \"horizon-mod-api\",\n \"horizon-mod-api-macros\",\n \"horizon-mod-points\",\n \"log\",\n]\n\n[[package]]\nname = \"log\""),
            "{lock}"
        );
        // One helper module per crate that owns points: the game's holds the catalog, a lower
        // crate's names its own layouts by the catalog's paths and has none.
        let lib =
            |krate: &str| std::fs::read_to_string(root.join(krate).join("src/lib.rs")).unwrap();
        let (client, lower) = (
            lib("crates/horizon-client"),
            lib("crates/horizon-client-render"),
        );
        assert_eq!(
            client.matches("pub mod __horizon_points").count(),
            1,
            "{client}"
        );
        assert_eq!(
            lower.matches("pub mod __horizon_points").count(),
            1,
            "{lower}"
        );
        assert!(client.contains("fn catalog") && !lower.contains("fn catalog"));
        assert!(
            lower.contains("extern crate self as horizon_client_render")
                && !client.contains("extern crate self")
        );
        // A second pass finds the attribute and refuses: the source tree never carries one.
        assert!(stage(&root).unwrap_err().contains("already has"));
        // A crate the points' own crate is built on can't be listed.
        put(
            "crates/horizon-mod-api-gen/slots/horizon.txt",
            "horizon_world::level::tick\n",
        );
        assert!(game_points(&root).unwrap_err().contains("built under"));
        // Nor one it builds with (the generator), nor the points' own crate.
        for listed in [
            "horizon_mod_api_gen::points::stage\n",
            "horizon_mod_points::register\n",
        ] {
            put("crates/horizon-mod-api-gen/slots/horizon.txt", listed);
            assert!(
                game_points(&root).unwrap_err().contains("built under"),
                "{listed}"
            );
        }
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// A module whose file each target names with `cfg_attr(…, path = …)` is read from each
    /// target's file under its condition; it has no default file, and none is needed.
    #[test]
    fn per_target_module_paths_are_followed_and_need_no_default_file() {
        let dir = std::env::temp_dir().join(format!("points-cfg-attr-{}", std::process::id()));
        let src = dir.join("src");
        std::fs::create_dir_all(src.join("control")).unwrap();
        std::fs::write(
            src.join("lib.rs"),
            "#[cfg_attr(unix, path = \"control/unix.rs\")]\n#[cfg_attr(windows, path = \"control/windows.rs\")]\nmod platform;\n",
        )
        .unwrap();
        std::fs::write(src.join("control/unix.rs"), "pub fn a() {}").unwrap();
        std::fs::write(src.join("control/windows.rs"), "pub fn b() {}").unwrap();
        let files = module_files("k".into(), src.join("lib.rs"), src.clone(), false).unwrap();
        let mut found: Vec<_> = files
            .iter()
            .map(|f| {
                (
                    f.module.as_str(),
                    f.path.strip_prefix(&src).unwrap().to_owned(),
                    f.cfg.clone(),
                )
            })
            .collect();
        found.sort();
        assert_eq!(
            found,
            [
                ("k", "lib.rs".into(), vec![]),
                (
                    "k::platform",
                    "control/unix.rs".into(),
                    vec!["unix".to_owned()]
                ),
                (
                    "k::platform",
                    "control/windows.rs".into(),
                    vec!["windows".to_owned()]
                ),
            ]
        );
        // Every build compiles neither, so the point walk leaves the module out.
        let every = module_files("k".into(), src.join("lib.rs"), src, true).unwrap();
        assert_eq!(every.len(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
