//! Generate the typed mod interface from native hook signatures and catalog contracts.
mod host;
use heck::ToSnakeCase;
use quote::ToTokens;
use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::Path,
    process::Command,
};
use syn::{FnArg, ImplItem, Item, Pat, Type, visit::Visit, visit_mut::VisitMut};

fn impl_functions(item: syn::ItemImpl) -> impl Iterator<Item = syn::ItemFn> {
    item.items.into_iter().filter_map(|item| match item {
        ImplItem::Fn(method) => Some(syn::ItemFn {
            attrs: method.attrs,
            vis: method.vis,
            sig: method.sig,
            block: Box::new(method.block),
        }),
        _ => None,
    })
}

/// The inherent `Game` methods of the files under `path`, each with its module (`native_hooks`,
/// `lan::guest`…) below `module`.
fn game_methods(
    path: &Path,
    module: &str,
    methods: &mut Vec<(String, syn::ItemFn)>,
) -> Result<(), String> {
    for entry in fs::read_dir(path).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        let inner = format!("{module}::{stem}");
        if path.is_dir() {
            game_methods(&path, &inner, methods)?;
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            let source = fs::read_to_string(&path).map_err(|e| e.to_string())?;
            let file = syn::parse_file(&source).map_err(|e| format!("{}: {e}", path.display()))?;
            for item in file.items {
                if let Item::Impl(item) = item
                    && item.trait_.is_none()
                    && let Type::Path(ty) = &*item.self_ty
                    && ty.path.segments.len() == 1
                    && ty.path.segments[0].ident == "Game"
                {
                    methods.extend(impl_functions(item).map(|method| (inner.clone(), method)));
                }
            }
        }
    }
    Ok(())
}

fn doc_comments(
    attrs: &[syn::Attribute],
    name: &str,
    preserve_empty: bool,
) -> Result<String, String> {
    let mut docs = String::new();
    for attr in attrs.iter().filter(|attr| attr.path().is_ident("doc")) {
        let syn::Meta::NameValue(value) = &attr.meta else {
            return Err(format!("unsupported doc attribute in {name}"));
        };
        let syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(doc),
            ..
        }) = &value.value
        else {
            return Err(format!("unsupported doc attribute in {name}"));
        };
        let value = doc.value();
        for line in value.split('\n') {
            if preserve_empty || !line.is_empty() {
                docs.push_str(&format!("///{line}\n"));
            }
        }
    }
    Ok(docs)
}

struct ApiTypes {
    exports: HashSet<String>,
    handles: BTreeMap<String, String>,
}

fn api_types(source: &str, generated: &str) -> Result<ApiTypes, String> {
    fn use_names(tree: &syn::UseTree, names: &mut HashSet<String>) {
        match tree {
            syn::UseTree::Path(path) => use_names(&path.tree, names),
            syn::UseTree::Name(name) => {
                names.insert(name.ident.to_string());
            }
            syn::UseTree::Rename(rename) => {
                names.insert(rename.rename.to_string());
            }
            syn::UseTree::Group(group) => {
                for item in &group.items {
                    use_names(item, names);
                }
            }
            syn::UseTree::Glob(_) => {}
        }
    }
    let mut names = HashSet::new();
    let mut handles = BTreeMap::new();
    for (source, collect_handles) in [(source, true), (generated, false)] {
        let file = syn::parse_file(source).map_err(|e| e.to_string())?;
        for item in file.items {
            let ident = match item {
                Item::Use(item) if matches!(item.vis, syn::Visibility::Public(_)) => {
                    use_names(&item.tree, &mut names);
                    None
                }
                Item::Struct(item) if matches!(item.vis, syn::Visibility::Public(_)) => {
                    if collect_handles
                        && let Some(base) = item.ident.to_string().strip_suffix("Ref")
                        && !base.is_empty()
                    {
                        let receiver = base.to_snake_case();
                        if handles
                            .insert(receiver.clone(), item.ident.to_string())
                            .is_some()
                        {
                            return Err(format!("duplicate handle receiver {receiver}"));
                        }
                    }
                    Some(item.ident)
                }
                Item::Enum(item) if matches!(item.vis, syn::Visibility::Public(_)) => {
                    Some(item.ident)
                }
                Item::Union(item) if matches!(item.vis, syn::Visibility::Public(_)) => {
                    Some(item.ident)
                }
                Item::Type(item) if matches!(item.vis, syn::Visibility::Public(_)) => {
                    Some(item.ident)
                }
                Item::Trait(item) if matches!(item.vis, syn::Visibility::Public(_)) => {
                    Some(item.ident)
                }
                Item::Mod(item) if matches!(item.vis, syn::Visibility::Public(_)) => {
                    Some(item.ident)
                }
                _ => None,
            };
            if let Some(ident) = ident {
                names.insert(ident.to_string());
            }
        }
    }
    Ok(ApiTypes {
        exports: names,
        handles,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HelperSide {
    Server,
    Client,
    Both,
}

impl HelperSide {
    fn parse(value: &serde_json::Value) -> Result<Option<Self>, String> {
        let side = match value.get("side") {
            None => Ok(None),
            Some(serde_json::Value::String(side)) => match side.as_str() {
                "server" => Ok(Some(Self::Server)),
                "client" => Ok(Some(Self::Client)),
                "both" => Ok(Some(Self::Both)),
                _ => Err(format!("unknown helper side {side}")),
            },
            Some(_) => Err("helper side must be a string".into()),
        }?;
        if value.get("client_target").is_some() && side != Some(Self::Both) {
            return Err("client_target requires side both".into());
        }
        Ok(side)
    }
}

fn helper_target<'a>(
    game: &'a [(String, syn::ItemFn)],
    name: &str,
    target: &str,
) -> Result<&'a syn::ItemFn, String> {
    let native = target
        .rsplit("::")
        .next()
        .ok_or("helper target missing method")?;
    if !native.starts_with("native_") {
        return Err(format!("invalid helper target {target}"));
    }
    let method = one_function(game.iter().map(|(_, method)| method), native)?;
    let (module, _) = game
        .iter()
        .find(|(_, other)| std::ptr::eq(other, method))
        .expect("method found");
    let expected = format!("{module}::Game<'_>::{native}");
    if target != expected {
        return Err(format!(
            "wrong target for helper {name}: {target}, it is {expected}"
        ));
    }
    Ok(method)
}

fn matching_helper_signatures(server: &str, client: &str, name: &str) -> Result<(), String> {
    let normalize = |code: &str| -> Result<String, String> {
        let mut sig = syn::parse_str::<syn::ImplItemFn>(code)
            .map_err(|e| format!("{name}: {e}"))?
            .sig;
        for arg in &mut sig.inputs {
            if let syn::FnArg::Typed(arg) = arg {
                *arg.pat = syn::parse_quote!(_);
            }
        }
        Ok(sig.to_token_stream().to_string())
    };
    if normalize(server)? != normalize(client)? {
        return Err(format!("{name}: client and server signatures differ"));
    }
    Ok(())
}

fn host_method(
    method: &syn::ItemFn,
    name: &str,
    exports: &HashSet<String>,
) -> Result<(String, String), String> {
    struct Types<'a> {
        exports: &'a HashSet<String>,
        error: Option<String>,
    }
    impl Types<'_> {
        fn check_path(&mut self, path: &syn::Path) {
            let first = path.segments.first().unwrap().ident.to_string();
            let unqualified = path.leading_colon.is_none() && path.segments.len() == 1;
            let prelude = unqualified
                && matches!(
                    first.as_str(),
                    "bool"
                        | "char"
                        | "str"
                        | "u8"
                        | "u16"
                        | "u32"
                        | "u64"
                        | "u128"
                        | "usize"
                        | "i8"
                        | "i16"
                        | "i32"
                        | "i64"
                        | "i128"
                        | "isize"
                        | "f32"
                        | "f64"
                        | "String"
                        | "Option"
                        | "Result"
                        | "Vec"
                        | "Box"
                        | "Arc"
                        | "PathBuf"
                        | "AsRef"
                        | "AsMut"
                        | "From"
                        | "Into"
                        | "TryFrom"
                        | "TryInto"
                        | "Iterator"
                        | "IntoIterator"
                        | "DoubleEndedIterator"
                        | "ExactSizeIterator"
                        | "Extend"
                        | "Fn"
                        | "FnMut"
                        | "FnOnce"
                        | "Sized"
                        | "Send"
                        | "Sync"
                        | "Unpin"
                        | "Drop"
                        | "Default"
                        | "Clone"
                        | "Copy"
                        | "PartialEq"
                        | "PartialOrd"
                        | "Eq"
                        | "Ord"
                        | "ToOwned"
                        | "ToString"
                        | "Future"
                        | "IntoFuture"
                );
            if path.leading_colon.is_some() || !(prelude || self.exports.contains(&first)) {
                self.error = Some(format!(
                    "type {} is not available in mod_api",
                    path.to_token_stream()
                ));
            }
        }
    }
    impl<'ast> Visit<'ast> for Types<'_> {
        fn visit_type_impl_trait(&mut self, _: &'ast syn::TypeImplTrait) {
            self.error = Some("impl Trait is not object-safe".into());
        }
        fn visit_type_path(&mut self, ty: &'ast syn::TypePath) {
            if ty.qself.is_some() {
                self.error = Some("qualified associated types are not supported".into());
            } else {
                self.check_path(&ty.path);
            }
            syn::visit::visit_type_path(self, ty);
        }
        fn visit_trait_bound(&mut self, bound: &'ast syn::TraitBound) {
            self.check_path(&bound.path);
            syn::visit::visit_trait_bound(self, bound);
        }
    }
    if method.sig.asyncness.is_some() {
        return Err(format!("{name}: async methods are not object-safe"));
    }
    if method
        .sig
        .generics
        .params
        .iter()
        .any(|param| !matches!(param, syn::GenericParam::Lifetime(_)))
    {
        return Err(format!("{name}: generic methods are not object-safe"));
    }
    let mut types = Types {
        exports,
        error: None,
    };
    for arg in &method.sig.inputs {
        if let FnArg::Typed(arg) = arg {
            types.visit_type(&arg.ty);
        }
    }
    if let syn::ReturnType::Type(_, ty) = &method.sig.output {
        types.visit_type(ty);
    }
    if let Some(reason) = types.error {
        return Err(format!("{name}: {reason}"));
    }
    let mut sig = method.sig.clone();
    let original = sig.ident.clone();
    sig.ident = syn::parse_str(name).map_err(|e| format!("invalid helper name {name}: {e}"))?;
    let mut args = Vec::new();
    let callee = match sig.inputs.first() {
        Some(FnArg::Receiver(r)) if r.reference.is_some() => "self.",
        Some(FnArg::Receiver(_)) => {
            return Err(format!("{name}: expected a borrowed Game receiver"));
        }
        _ => {
            // A receiverless helper stays callable on `&self` in the trait.
            sig.inputs.insert(0, syn::parse_quote!(&self));
            "Self::"
        }
    };
    for arg in sig.inputs.iter_mut().skip(1) {
        let FnArg::Typed(arg) = arg else {
            return Err(format!("{name}: unexpected receiver"));
        };
        let Pat::Ident(pat) = &mut *arg.pat else {
            return Err(format!("{name}: unsupported argument"));
        };
        pat.mutability = None;
        args.push(pat.ident.to_string());
    }
    let docs = doc_comments(&method.attrs, name, true)?;
    let unused = args
        .iter()
        .map(|arg| format!("let _ = {arg};"))
        .collect::<String>();
    Ok((
        format!(
            "{docs}{} {{ {unused} unimplemented!(\"Host::{name}\") }}\n",
            sig.to_token_stream()
        ),
        format!(
            "{} {{ {callee}{original}({}) }}\n",
            sig.to_token_stream(),
            args.join(", ")
        ),
    ))
}

/// A helper's method on its handle (`receiver`): `entity_position(entity)` on
/// `Host` is also `EntityRef::position(self, host)`, which calls it. The
/// helper's first argument is the handle; the host is borrowed as the helper
/// borrows `Game`.
fn handle_method<'a>(
    method: &syn::ItemFn,
    name: &str,
    receiver: &str,
    handles: &'a BTreeMap<String, String>,
) -> Result<(&'a str, String), String> {
    let handle = handles
        .get(receiver)
        .ok_or_else(|| format!("{name}: unknown receiver {receiver}"))?
        .as_str();
    let short = name
        .strip_prefix(receiver)
        .and_then(|rest| rest.strip_prefix('_'))
        .ok_or_else(|| format!("{name}: a {receiver} helper's name starts with {receiver}_"))?;
    let mut sig = method.sig.clone();
    sig.ident =
        syn::parse_str(short).map_err(|_| format!("{name}: {short} is not a method name"))?;
    let mut inputs = sig.inputs.iter();
    let mutable = matches!(inputs.next(), Some(FnArg::Receiver(r)) if r.mutability.is_some());
    let mut inputs = sig.inputs.iter();
    let host: FnArg = match inputs.next() {
        Some(FnArg::Receiver(r)) if r.reference.is_some() && r.mutability.is_some() => {
            syn::parse_quote!(host: &mut dyn Host)
        }
        Some(FnArg::Receiver(r)) if r.reference.is_some() => syn::parse_quote!(host: &dyn Host),
        _ => return Err(format!("{name}: a {receiver} helper borrows Game")),
    };
    let takes_handle = matches!(
        inputs.next(),
        Some(FnArg::Typed(arg)) if matches!(&*arg.ty, Type::Path(ty) if ty.path.is_ident(handle))
    );
    if !takes_handle {
        return Err(format!(
            "{name}: a {receiver} helper takes a {handle} first"
        ));
    }
    let mut args = String::new();
    let mut rest = vec![syn::parse_quote!(self), host];
    for arg in inputs {
        let mut arg = arg.clone();
        let FnArg::Typed(typed) = &mut arg else {
            return Err(format!("{name}: unexpected receiver"));
        };
        let Pat::Ident(pat) = &mut *typed.pat else {
            return Err(format!("{name}: unsupported argument"));
        };
        pat.mutability = None;
        args.push_str(&format!(", {}", pat.ident));
        rest.push(arg);
    }
    sig.inputs = rest.into_iter().collect();
    let docs = doc_comments(&method.attrs, name, true)?;
    let backend = if mutable {
        "backend_mut_of"
    } else {
        "backend_of"
    };
    let returns = |ident: &str| {
        matches!(&sig.output, syn::ReturnType::Type(_, ty) if matches!(&**ty, syn::Type::Path(path)
            if path.path.segments.last().is_some_and(|seg| seg.ident == ident)))
    };
    let call = format!("{name}(self{args})");
    let body = if returns("Option") {
        format!("{backend}(host)?.{call}")
    } else if returns("bool") {
        format!("{backend}(host).map(|backend| backend.{call}).unwrap_or(false)")
    } else {
        return Err(format!(
            "{name}: a {receiver} helper returns Option or bool, the value a host without a backend refuses"
        ));
    };
    Ok((
        handle,
        format!("{docs}pub {} {{ {body} }}\n", sig.to_token_stream()),
    ))
}

fn has_call(block: &syn::Block, expected: &str) -> bool {
    struct Calls<'a> {
        expected: &'a str,
        found: bool,
    }
    impl<'ast> Visit<'ast> for Calls<'_> {
        fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
            if let syn::Expr::Path(path) = &*call.func
                && path
                    .path
                    .segments
                    .last()
                    .is_some_and(|s| s.ident == self.expected)
            {
                self.found = true;
            }
            syn::visit::visit_expr_call(self, call);
        }
    }
    let mut calls = Calls {
        expected,
        found: false,
    };
    calls.visit_block(block);
    calls.found
}

fn one_function<'a>(
    items: impl Iterator<Item = &'a syn::ItemFn>,
    name: &str,
) -> Result<&'a syn::ItemFn, String> {
    let mut matches = items.filter(|item| item.sig.ident == name);
    let first = matches
        .next()
        .ok_or_else(|| format!("expected one {name} signature"))?;
    if matches.next().is_some() {
        return Err(format!("expected one {name} signature"));
    }
    Ok(first)
}

fn fresh_name(base: &str, used: &mut HashSet<String>) -> String {
    let mut name = base.to_owned();
    while !used.insert(name.clone()) {
        name.push('_');
    }
    name
}

/// Turns a hook argument's type into its event field's. A `&mut` primitive scalar becomes the value
/// itself, written back after every mod has run; any other `&mut` stays a reference to the caller's
/// value. Either way one event goes through every mod, so each sees the changes before it. Every
/// elided reference lifetime becomes the event's `'a`, or `'b` inside a `&mut`, which can't shorten
/// it. The launcher keeps a copy for trait injections (`launcher/src/native_mods/inject.rs`); both
/// test the same types.
#[derive(Default)]
struct EventLifetimes {
    a: bool,
    b: bool,
    in_mut: bool,
}

impl EventLifetimes {
    /// The field's type, and whether it is a scalar copied in and written back.
    fn field(&mut self, ty: &Type) -> (Type, bool) {
        let scalar = match ty {
            Type::Reference(reference) if reference.mutability.is_some() => {
                match &*reference.elem {
                    Type::Path(path)
                        if path.qself.is_none()
                            && path.path.get_ident().is_some_and(|ident| {
                                [
                                    "bool", "char", "i8", "i16", "i32", "i64", "i128", "isize",
                                    "u8", "u16", "u32", "u64", "u128", "usize", "f32", "f64",
                                ]
                                .iter()
                                .any(|scalar| ident == scalar)
                            }) =>
                    {
                        Some(&*reference.elem)
                    }
                    _ => None,
                }
            }
            _ => None,
        };
        if let Some(scalar) = scalar {
            return (scalar.clone(), true);
        }
        let mut field = ty.clone();
        self.visit_type_mut(&mut field);
        (field, false)
    }
}

impl VisitMut for EventLifetimes {
    fn visit_type_reference_mut(&mut self, reference: &mut syn::TypeReference) {
        if reference.lifetime.is_none() {
            reference.lifetime = Some(syn::parse_quote!('_));
        }
        let outer = self.in_mut;
        self.visit_lifetime_mut(reference.lifetime.as_mut().unwrap());
        self.in_mut |= reference.mutability.is_some();
        self.visit_type_mut(&mut reference.elem);
        self.in_mut = outer;
    }
    fn visit_lifetime_mut(&mut self, lifetime: &mut syn::Lifetime) {
        if lifetime.ident == "_" {
            *lifetime = if self.in_mut {
                self.b = true;
                syn::parse_quote!('b)
            } else {
                self.a = true;
                syn::parse_quote!('a)
            };
        }
    }
}

#[derive(Debug)]
struct CatalogHook {
    entry: serde_json::Value,
    side: HelperSide,
}

impl std::ops::Deref for CatalogHook {
    type Target = serde_json::Value;

    fn deref(&self) -> &Self::Target {
        &self.entry
    }
}

fn catalog_hooks(root: &Path, catalog: &serde_json::Value) -> Result<Vec<CatalogHook>, String> {
    let mut hooks = catalog["hooks"]
        .as_array()
        .ok_or("catalog hooks must be an array")?
        .clone();
    let mut paths = fs::read_dir(root.join("tools/native-hook-fragments"))
        .map_err(|e| e.to_string())?
        .map(|entry| entry.map(|entry| entry.path()).map_err(|e| e.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    paths.sort();
    for path in paths
        .into_iter()
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
    {
        let fragment: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).map_err(|e| e.to_string())?)
                .map_err(|e| format!("{}: {e}", path.display()))?;
        hooks.extend(
            fragment["hooks"]
                .as_array()
                .ok_or("fragment hooks must be an array")?
                .iter()
                .cloned(),
        );
    }
    let mut seen = HashSet::new();
    let mut validated = Vec::with_capacity(hooks.len());
    for hook in hooks {
        let name = hook["name"].as_str().ok_or("hook name missing")?;
        if !seen.insert(name.to_owned()) {
            return Err(format!("duplicate hook {name}"));
        }
        let side = HelperSide::parse(&hook)
            .map_err(|e| format!("hook {name}: {e}"))?
            .ok_or_else(|| format!("hook {name}: side missing"))?;
        for key in ["neoforge", "fabric", "paper"] {
            if let Some(value) = hook.get(key) {
                let valid = value.is_string()
                    || value
                        .as_array()
                        .is_some_and(|values| values.iter().all(serde_json::Value::is_string));
                if !valid {
                    return Err(format!(
                        "hook {name}: {key} must be a string or list of strings"
                    ));
                }
            }
        }
        validated.push(CatalogHook { entry: hook, side });
    }
    Ok(validated)
}

/// The crate folder and module path of a hook target that is a free function
/// outside `Game`. `horizon-client-lib` builds from `horizon-client/src`
/// (`client_lib.rs`).
fn free_function_source(target: &str) -> Option<(&'static str, String)> {
    let prefixes = [
        ("horizon_world::", "horizon-world"),
        ("horizon_client_render::", "horizon-client-render"),
        ("horizon_client_lib::", "horizon-client"),
    ];
    prefixes
        .into_iter()
        .find_map(|(prefix, krate)| target.strip_prefix(prefix).map(|p| (krate, p.to_owned())))
}

/// Whether a hook function needs its dispatch `call` filled in: false when it
/// makes the call; true when it is a `Game` or `GameLevel` method whose body is
/// only the placeholder a new hook starts with, `let _ = (every, parameter);`
/// (`let _ = parameter;` for one). Any other body is an error, left alone.
fn needs_fill(method: &syn::ItemFn, call: &str, in_game: bool) -> Result<bool, String> {
    if has_call(&method.block, call) {
        return Ok(false);
    }
    let params: Vec<String> = method
        .sig
        .inputs
        .iter()
        .filter_map(|arg| match arg {
            FnArg::Typed(arg) => Some(arg.pat.to_token_stream().to_string()),
            FnArg::Receiver(_) => None,
        })
        .collect();
    let named = match &method.block.stmts[..] {
        [
            syn::Stmt::Local(syn::Local {
                pat: Pat::Wild(_),
                init:
                    Some(syn::LocalInit {
                        expr,
                        diverge: None,
                        ..
                    }),
                ..
            }),
        ] => match &**expr {
            syn::Expr::Tuple(tuple) => Some(
                tuple
                    .elems
                    .iter()
                    .map(|e| e.to_token_stream().to_string())
                    .collect(),
            ),
            single @ syn::Expr::Path(_) => Some(vec![single.to_token_stream().to_string()]),
            _ => None,
        },
        _ => None,
    };
    if in_game && named == Some(params) {
        return Ok(true);
    }
    Err(format!(
        "missing {call} call in {}: add it, or leave a Game hook's body as the let _ = (…); placeholder for cargo run -p xtask -- mod-api to fill",
        method.sig.ident
    ))
}

/// A hook function whose placeholder body becomes `call`.
struct Fill {
    native: String,
    call: String,
}

/// `source` with each fill's placeholder statement replaced by its call,
/// formatted as `cargo fmt` would.
fn filled_hooks(source: &str, fills: &[Fill]) -> Result<Vec<u8>, String> {
    let mut source = source.to_owned();
    for Fill { native, call } in fills {
        let start = source
            .find(&format!("fn {native}("))
            .ok_or_else(|| format!("{native} not found"))?;
        let placeholder = start
            + source[start..]
                .find("let _ =")
                .ok_or_else(|| format!("{native} has no placeholder"))?;
        let end = placeholder
            + source[placeholder..]
                .find(';')
                .ok_or_else(|| format!("{native} has no placeholder"))?
            + 1;
        source.replace_range(placeholder..end, call);
    }
    rustfmt(&source)
}

/// The workspace's edition, which rustfmt formats generated code for.
const EDITION: &str = "2024";

/// `code` as `cargo fmt` formats it.
fn rustfmt(code: &str) -> Result<Vec<u8>, String> {
    let mut child = Command::new("rustfmt")
        .args(["--emit", "stdout", "--edition", EDITION])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    use std::io::Write;
    child
        .stdin
        .take()
        .ok_or("rustfmt stdin")?
        .write_all(code.as_bytes())
        .map_err(|e| e.to_string())?;
    let formatted = child.wait_with_output().map_err(|e| e.to_string())?;
    if !formatted.status.success() {
        return Err("rustfmt failed".into());
    }
    Ok(formatted.stdout)
}

/// The doc of [`BlockPart`], the first part trait in the generated file.
const PART_DOCS: &str = "/// A block's part: code attached through [`Registrar`] to one block, a mod's own or vanilla's.\n/// Its methods are the scoped hooks', asked only for the blocks it is attached to, after\n/// every mod's [`Hooks`] method. They may run on the window's thread or the server thread.\n";

/// The crate whose generated sources are being built.
#[derive(Clone, Copy)]
pub enum Target {
    Client,
    Render,
}

/// Fill new hooks' placeholders in hand-written source. Never writes generated files.
pub fn fill_hooks(root: &Path) -> Result<(), String> {
    sources(root, FillMode::Fill).map(|_| ())
}

#[derive(Debug)]
struct Sources {
    hooks: String,
    host: String,
    implementation: String,
    render: String,
}

/// Parse and validate the catalog and hook/helper signatures.
fn sources(root: &Path, fill: FillMode) -> Result<Sources, String> {
    let catalog: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(root.join("native-hooks.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let source = fs::read_to_string(root.join("crates/horizon-client/src/game/native_hooks.rs"))
        .map_err(|e| e.to_string())?;
    let file = syn::parse_file(&source).map_err(|e| e.to_string())?;
    let mut methods = Vec::new();
    for item in file.items {
        if let Item::Impl(item) = item {
            methods.extend(impl_functions(item));
        }
    }
    let hooks = catalog_hooks(root, &catalog)?;
    let dispatch::HookSources {
        traits,
        dispatch,
        render_traits,
        render_dispatch,
        part_traits,
        fills,
        hook_ids,
        hook_count,
        render_hook_ids,
        render_hook_count,
    } = dispatch::emit(root, &hooks, &methods)?;
    if hooks.is_empty() {
        return Err("no native hooks".into());
    }
    let part = |name: &str| part_traits.get(name).map_or("", String::as_str);
    let code = format!(
        "// @generated by cargo run -p xtask -- mod-api; do not edit.\nuse super::*;\n#[doc(hidden)]\n#[repr(usize)]\n#[derive(Clone, Copy)]\npub enum HookId {{ {hook_ids} }}\npub(crate) const HOOK_COUNT: usize = {hook_count};\npub trait Hooks: std::any::Any + Send + Sync {{\nfn render(&self) -> Option<&dyn RenderHooks> {{ None }}\n/// Once, at startup: attach this mod's parts (`r.block_part(\"mymod:shaft\", Shaft)`).\nfn register(&self, r: &mut Registrar) {{ let _ = r; }}\n{traits}}}\n{PART_DOCS}pub trait BlockPart: Send + Sync + 'static {{\n{}}}\n/// A block's part that also answers for its block entity, and for the block like a [`BlockPart`].\npub trait BlockEntityPart: BlockPart {{\n{}}}\n/// An item's part.\npub trait ItemPart: Send + Sync + 'static {{\n{}}}\n/// A mod menu type's part.\npub trait MenuPart: Send + Sync + 'static {{\n{}}}\n{dispatch}",
        part("BlockPart"),
        part("BlockEntityPart"),
        part("ItemPart"),
        part("MenuPart"),
    );
    let (host, implementation) = host::generate(root, &catalog, &code)?;
    let render_code = format!(
        "// @generated by cargo run -p xtask -- mod-api; do not edit.\nuse super::*;\n#[doc(hidden)]\n#[repr(usize)]\n#[derive(Clone, Copy)]\npub enum RenderHookId {{ {render_hook_ids} }}\npub(crate) const RENDER_HOOK_COUNT: usize = {render_hook_count};\npub trait RenderHooks: std::any::Any + Send + Sync {{\n{render_traits}}}\n{render_dispatch}"
    );
    // A new hook's placeholder body becomes its dispatch call.
    let filled = if fills.is_empty() {
        None
    } else if matches!(fill, FillMode::Require) {
        let natives = fills
            .iter()
            .map(|fill| fill.native.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        return Err(format!(
            "{natives}: the let _ = (…); placeholder needs its dispatch call; run cargo run -p xtask -- mod-api"
        ));
    } else if matches!(fill, FillMode::Fill) {
        Some(filled_hooks(&source, &fills)?)
    } else {
        None
    };
    for code in [&code, &host, &implementation, &render_code] {
        syn::parse_file(code).map_err(|e| format!("generated Rust: {e}"))?;
    }
    if let Some(filled) = filled {
        fs::write(
            root.join("crates/horizon-client/src/game/native_hooks.rs"),
            filled,
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(Sources {
        hooks: code,
        host,
        implementation,
        render: render_code,
    })
}

#[derive(Clone, Copy)]
enum FillMode {
    Fill,
    Require,
    Ignore,
}

mod build;
mod dispatch;
mod host_param;
pub use build::{generate, inject};
mod injection;
pub use injection::{TraitContract, TraitInjection};

#[cfg(test)]
mod injection_tests;

/// Generate the dispatch test fixture without preventing xtask from filling new placeholders.
#[doc(hidden)]
pub fn generate_test_hooks(root: &Path, out_dir: &Path) -> Result<(), String> {
    build::track(root)?;
    let code = sources(root, FillMode::Ignore)?.hooks;
    fs::write(out_dir.join("mod_api_generated.rs"), code).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn helper_side_is_optional_and_typed() {
        use super::HelperSide;
        use serde_json::json;
        assert_eq!(HelperSide::parse(&json!({})), Ok(None));
        assert_eq!(
            HelperSide::parse(&json!({"side":"server"})),
            Ok(Some(HelperSide::Server))
        );
        assert_eq!(
            HelperSide::parse(&json!({"side":"client"})),
            Ok(Some(HelperSide::Client))
        );
        assert_eq!(
            HelperSide::parse(&json!({"side":"both"})),
            Ok(Some(HelperSide::Both))
        );
        assert!(HelperSide::parse(&json!({"side":null})).is_err());
        assert!(HelperSide::parse(&json!({"side":"unknown"})).is_err());
        assert!(HelperSide::parse(&json!({"client_target":"somewhere"})).is_err());
        assert!(HelperSide::parse(&json!({"side":"server", "client_target":"somewhere"})).is_err());
        assert_eq!(
            HelperSide::parse(&json!({"side":"both", "client_target":"somewhere"})),
            Ok(Some(HelperSide::Both))
        );
    }
    #[test]
    fn helper_targets_must_name_the_real_game_method() {
        let game = vec![(
            "horizon_client::game::sample".to_owned(),
            syn::parse_quote!(
                fn native_sample(&self) {}
            ),
        )];
        assert!(
            super::helper_target(
                &game,
                "sample",
                "horizon_client::game::sample::Game<'_>::native_sample"
            )
            .is_ok()
        );
        assert!(
            super::helper_target(
                &game,
                "sample",
                "horizon_client::game::wrong::Game<'_>::native_sample"
            )
            .is_err()
        );
        assert!(
            super::helper_target(
                &game,
                "sample",
                "horizon_client::game::sample::Game<'_>::missing"
            )
            .is_err()
        );
    }
    #[test]
    fn dual_targets_require_matching_host_signatures() {
        let server = "fn value(&self, x: i32) -> Option<i32> { None }";
        let same = "fn value(&self, y: i32) -> Option<i32> { Some(y) }";
        let wrong_type = "fn value(&self, y: u32) -> Option<i32> { None }";
        let wrong_receiver = "fn value(&mut self, y: i32) -> Option<i32> { None }";
        assert!(super::matching_helper_signatures(server, same, "value").is_ok());
        assert!(super::matching_helper_signatures(server, wrong_type, "value").is_err());
        assert!(super::matching_helper_signatures(server, wrong_receiver, "value").is_err());
    }
    use quote::ToTokens;
    use serde_json::Value;
    #[test]
    fn fragment_hooks_generate_and_duplicates_fail() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let catalog: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(root.join("native-hooks.json")).unwrap())
                .unwrap();
        let hooks = super::catalog_hooks(&root, &catalog).unwrap();
        let hook = hooks
            .iter()
            .find(|h| h["name"] == "recipes_received")
            .unwrap();
        assert_eq!(hook["mod_api"]["contract"], "listen");
        let generated = super::sources(&root, super::FillMode::Require)
            .unwrap()
            .hooks;
        assert!(generated.contains("fn recipes_received("));
        assert!(generated.contains("/// Side: client."));

        let dir = root
            .join("target")
            .join(format!("mod-api-fragments-{}", std::process::id()));
        let fragments = dir.join("tools/native-hook-fragments");
        std::fs::create_dir_all(&fragments).unwrap();
        std::fs::write(fragments.join("empty.json"), r#"{"hooks":[]}"#).unwrap();
        std::fs::write(
            fragments.join("duplicate.json"),
            r#"{"hooks":[{"name":"same","side":"server"}]}"#,
        )
        .unwrap();
        let catalog = serde_json::json!({"hooks":[{"name":"same","side":"server"}]});
        assert_eq!(
            super::catalog_hooks(&dir, &catalog).unwrap_err(),
            "duplicate hook same"
        );
        std::fs::remove_file(fragments.join("duplicate.json")).unwrap();
        assert_eq!(super::catalog_hooks(&dir, &catalog).unwrap().len(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn counterpart_metadata_accepts_strings_lists_and_absence() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target")
            .join(format!("mod-api-counterparts-{}", std::process::id()));
        std::fs::create_dir_all(root.join("tools/native-hook-fragments")).unwrap();
        let catalog = serde_json::json!({"hooks": [
            {"name": "one", "side": "server", "fabric": "Event.ONE", "neoforge": "OneEvent"},
            {"name": "two", "side": "both", "fabric": ["Event.TWO", "Event.THREE"], "paper": "SomeEvent"},
            {"name": "none", "side": "client"}
        ]});
        assert_eq!(super::catalog_hooks(&root, &catalog).unwrap().len(), 3);
        for side in [serde_json::json!(null), serde_json::json!("invalid")] {
            let invalid = serde_json::json!({"hooks": [{"name": "bad", "side": side}]});
            assert!(super::catalog_hooks(&root, &invalid).is_err());
        }
        let missing = serde_json::json!({"hooks": [{"name": "bad"}]});
        assert!(
            super::catalog_hooks(&root, &missing)
                .unwrap_err()
                .contains("side missing")
        );
        for key in ["fabric", "neoforge", "paper"] {
            let invalid =
                serde_json::json!({"hooks": [{"name": "bad", "side": "server", key: ["ok", 4]}]});
            assert!(
                super::catalog_hooks(&root, &invalid)
                    .unwrap_err()
                    .contains(key)
            );
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn public_ref_structs_define_receivers_without_registration() {
        let types = super::api_types(
            "pub struct BlockEntityRef; pub struct HTTPRequestRef;
             struct HiddenRef; pub(crate) struct InternalRef;
             pub enum EnumRef {} pub type AliasRef = BlockEntityRef; pub struct Plain;",
            "pub struct GeneratedRef;",
        )
        .unwrap();
        for (receiver, ty) in [
            ("block_entity", "BlockEntityRef"),
            ("http_request", "HTTPRequestRef"),
        ] {
            let name = format!("{receiver}_kind");
            let method = syn::parse_str(&format!(
                "fn native_{name}(&self, value: {ty}) -> Option<()> {{}}"
            ))
            .unwrap();
            let (handle, code) =
                super::handle_method(&method, &name, receiver, &types.handles).unwrap();
            assert_eq!(handle, ty);
            assert!(code.contains(&format!("backend_of(host)?.{name}(self)")));
        }
        let method = syn::parse_quote! { fn native_hidden_kind(&self, value: HiddenRef) {} };
        for receiver in [
            "hidden",
            "internal",
            "enum",
            "alias",
            "plain",
            "generated",
            "missing",
        ] {
            assert!(
                super::handle_method(&method, "hidden_kind", receiver, &types.handles)
                    .unwrap_err()
                    .contains("unknown receiver")
            );
        }
        assert_eq!(
            super::api_types("pub struct URLRef; pub struct UrlRef;", "")
                .err()
                .unwrap(),
            "duplicate handle receiver url",
        );
    }

    #[test]
    fn handle_methods_call_the_host_and_borrow_it_as_the_helper_borrows_game() {
        let types = super::api_types("pub struct EntityRef; pub struct LevelRef;", "").unwrap();
        let read: syn::ItemFn = syn::parse_quote! {
            /// Where it is.
            fn native_entity_position(&self, entity: EntityRef, mut scale: f64) -> Option<[f64; 3]> { todo!() }
        };
        let (handle, code) =
            super::handle_method(&read, "entity_position", "entity", &types.handles).unwrap();
        assert_eq!(handle, "EntityRef");
        assert!(code.starts_with("/// Where it is.\n"));
        assert!(code.contains("pub fn position (self , host : & dyn Host , scale : f64)"));
        assert!(code.contains("backend_of(host)?.entity_position(self, scale)"));
        let write: syn::ItemFn = syn::parse_quote! {
            fn native_level_set(&mut self, level: LevelRef) -> bool { todo!() }
        };
        let (handle, code) =
            super::handle_method(&write, "level_set", "level", &types.handles).unwrap();
        assert_eq!(handle, "LevelRef");
        assert!(code.contains("host : & mut dyn Host"));
        assert!(code.contains(
            "backend_mut_of(host).map(|backend| backend.level_set(self)).unwrap_or(false)"
        ));
        for (method, name, receiver, error) in [
            (
                syn::parse_quote! { fn native_entity_x(&self, entity: EntityRef) {} },
                "entity_x",
                "chunk",
                "unknown receiver",
            ),
            (
                syn::parse_quote! { fn native_mob_x(&self, entity: EntityRef) {} },
                "mob_x",
                "entity",
                "starts with entity_",
            ),
            (
                syn::parse_quote! { fn native_entity_type(&self, entity: EntityRef) {} },
                "entity_type",
                "entity",
                "not a method name",
            ),
            (
                syn::parse_quote! { fn native_entity_x(&self, entity: EntityRef) {} },
                "entity_x",
                "entity",
                "returns Option or bool",
            ),
            (
                syn::parse_quote! { fn native_entity_x(entity: EntityRef) {} },
                "entity_x",
                "entity",
                "borrows Game",
            ),
            (
                syn::parse_quote! { fn native_entity_x(&self, level: LevelRef) {} },
                "entity_x",
                "entity",
                "takes a EntityRef first",
            ),
        ] {
            let method: syn::ItemFn = method;
            assert!(
                super::handle_method(&method, name, receiver, &types.handles)
                    .unwrap_err()
                    .contains(error),
                "{name}"
            );
        }
    }

    #[test]
    fn host_mapping_preserves_access_and_docs_and_rejects_unavailable_types() {
        let exports = super::api_types(
            "pub use external::{Visible, Original as Renamed}; pub struct Defined; struct Hidden;",
            "pub struct Generated;",
        )
        .unwrap()
        .exports;
        for ty in ["Visible", "Renamed", "Defined", "Generated"] {
            let method: syn::ItemFn =
                syn::parse_str(&format!("fn native_test(&self, value: {ty}) {{ }}")).unwrap();
            assert!(super::host_method(&method, "test", &exports).is_ok());
        }
        let foreign: syn::ItemFn =
            syn::parse_quote! { fn native_foreign(&self, value: elsewhere::Visible) {} };
        assert!(super::host_method(&foreign, "foreign", &exports).is_err());
        let hidden_trait: syn::ItemFn =
            syn::parse_quote! { fn native_hidden(&self, value: &dyn Hidden) {} };
        assert!(super::host_method(&hidden_trait, "hidden", &exports).is_err());
        let read: syn::ItemFn = syn::parse_quote! {
            /// Read a value exactly.
            ///
            /// Keep the blank line too.
            fn native_read(&self, pos: [i32; 3]) -> String { todo!() }
        };
        let (declaration, implementation) = super::host_method(&read, "read", &exports).unwrap();
        assert!(
            declaration.contains("/// Read a value exactly.\n///\n/// Keep the blank line too.")
        );
        assert!(declaration.contains("& self"));
        assert!(implementation.contains("self.native_read(pos)"));
        let dir: syn::ItemFn = syn::parse_quote! { fn native_dir() -> PathBuf { todo!() } };
        let (declaration, implementation) = super::host_method(&dir, "dir", &exports).unwrap();
        assert!(declaration.contains("& self"));
        assert!(implementation.contains("Self::native_dir()"));
        let owned: syn::ItemFn = syn::parse_quote! { fn native_owned(self) {} };
        assert!(super::host_method(&owned, "owned", &exports).is_err());
        let write: syn::ItemFn = syn::parse_quote! {
            fn native_write(&mut self, value: i32) { }
        };
        let (declaration, _) = super::host_method(&write, "write", &exports).unwrap();
        assert!(declaration.contains("& mut self"));
        let private: syn::ItemFn = syn::parse_quote! {
            fn native_private(&self, value: BinOnly) { }
        };
        assert!(
            super::host_method(&private, "private", &exports)
                .unwrap_err()
                .contains("BinOnly")
        );
        let generic: syn::ItemFn = syn::parse_quote! {
            fn native_generic<T>(&self, value: T) { }
        };
        assert!(
            super::host_method(&generic, "generic", &exports)
                .unwrap_err()
                .contains("object-safe")
        );
        let callback: syn::ItemFn = syn::parse_quote! {
            fn native_callback(&self, callback: impl FnOnce()) { }
        };
        assert!(
            super::host_method(&callback, "callback", &exports)
                .unwrap_err()
                .contains("object-safe")
        );
        let asynchronous: syn::ItemFn = syn::parse_quote! {
            async fn native_async(&self) { }
        };
        assert!(
            super::host_method(&asynchronous, "async", &exports)
                .unwrap_err()
                .contains("object-safe")
        );
    }

    #[test]
    fn lookup_rejects_missing_and_duplicate_functions() {
        let first: syn::ItemFn = syn::parse_quote! { fn native_example() {} };
        let other: syn::ItemFn = syn::parse_quote! { fn unrelated() {} };
        assert!(super::one_function([&other].into_iter(), "native_example").is_err());
        assert!(super::one_function([&first, &other].into_iter(), "native_example").is_ok());
        assert!(super::one_function([&first, &first].into_iter(), "native_example").is_err());
    }

    #[test]
    fn generated_locals_do_not_shadow_hook_arguments() {
        let mut used = ["mods", "mods_", "mod_hook", "replacement", "host"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        assert_eq!(super::fresh_name("mods", &mut used), "mods__");
        assert_eq!(super::fresh_name("mod_hook", &mut used), "mod_hook_");
        assert_eq!(super::fresh_name("replacement", &mut used), "replacement_");
        assert_eq!(super::fresh_name("host", &mut used), "host_");
    }

    #[test]
    fn level_hosts_match_receiver_mutability() {
        let level = super::CatalogHook {
            entry: serde_json::json!({"target": "horizon_client::game::GameLevel<'_>::native_test"}),
            side: super::HelperSide::Both,
        };
        let game = super::CatalogHook {
            entry: serde_json::json!({"target": "horizon_client::game::Game<'_>::native_test"}),
            side: super::HelperSide::Both,
        };
        use super::host_param::{HostParam, host_param};
        let param = |ty: &str, call| {
            Some(HostParam {
                ty: ty.into(),
                call,
            })
        };
        assert_eq!(
            host_param(&level, Some(false), false),
            param("&dyn LevelHost", "self")
        );
        assert_eq!(
            host_param(&level, Some(true), false),
            param("&mut dyn LevelRandomHost", "self")
        );
        let ticks = "&mut super::mod_api_host::LevelTicks { level: self, ticks }";
        assert_eq!(
            host_param(&level, Some(false), true),
            param("&mut dyn LevelTicksHost", ticks)
        );
        assert_eq!(
            host_param(&level, Some(true), true),
            param("&mut dyn LevelWriteHost", ticks)
        );
        assert_eq!(host_param(&level, None, false), None);
        assert_eq!(
            host_param(&game, Some(true), false),
            param("&mut dyn Host", "self")
        );
    }

    #[test]
    fn only_a_game_hooks_placeholder_body_is_filled() {
        let fill = |body: &str, in_game: bool| {
            let method: syn::ItemFn = syn::parse_str(&format!(
                "fn native_h(&mut self, player: u128, cancel: &mut bool) {{ {body} }}"
            ))
            .unwrap();
            super::needs_fill(&method, "dispatch_h", in_game)
        };
        assert_eq!(fill("let _ = (player, cancel);", true), Ok(true));
        assert_eq!(
            fill(
                "horizon_client::mod_api::dispatch_h(self, mods(), player, cancel);",
                true
            ),
            Ok(false)
        );
        for (body, in_game) in [
            ("let _ = (player, cancel);", false),
            ("let _ = (cancel, player);", true),
            ("let _ = player;", true),
            ("let _ = (player, cancel); *cancel = true;", true),
            ("", true),
        ] {
            assert!(
                fill(body, in_game)
                    .unwrap_err()
                    .starts_with("missing dispatch_h call in native_h: "),
                "{body}"
            );
        }
        let one: syn::ItemFn =
            syn::parse_quote! { fn native_one(&self, from: &str) { let _ = from; } };
        assert_eq!(super::needs_fill(&one, "dispatch_one", true), Ok(true));
        let source = "impl Game<'_> {\n    fn native_other(&self, a: u32) {\n        let _ = a;\n    }\n    fn native_h(&mut self, player: u128, name: &str, cancel: &mut bool) {\n        let _ = (player, name, cancel);\n    }\n}\n";
        let filled = super::filled_hooks(
            source,
            &[super::Fill {
                native: "native_h".into(),
                call: "mod_api::dispatch_h(self, mod_api::mods(), player, name, cancel);".into(),
            }],
        )
        .unwrap();
        assert_eq!(
            String::from_utf8(filled).unwrap(),
            "impl Game<'_> {\n    fn native_other(&self, a: u32) {\n        let _ = a;\n    }\n    fn native_h(&mut self, player: u128, name: &str, cancel: &mut bool) {\n        mod_api::dispatch_h(self, mod_api::mods(), player, name, cancel);\n    }\n}\n"
        );
    }

    /// `item_damaged` taken back to how a new hook arrives (its body the
    /// placeholder) in a copy of the sources: the check reports it, any other
    /// body is left alone, a missing `mod_api` entry fails by name, and the
    /// generator brings back the shipped file byte for byte with the generated
    /// code unchanged.
    #[test]
    fn new_hook_placeholder_is_reported_then_filled() {
        fn copy(from: &std::path::Path, to: &std::path::Path) {
            if from.is_dir() {
                for entry in std::fs::read_dir(from).unwrap() {
                    let entry = entry.unwrap();
                    copy(&entry.path(), &to.join(entry.file_name()));
                }
            } else {
                std::fs::create_dir_all(to.parent().unwrap()).unwrap();
                std::fs::copy(from, to).unwrap();
            }
        }
        let real = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let root = real
            .join("target")
            .join(format!("mod-api-fill-{}", std::process::id()));
        let catalog: Value =
            serde_json::from_str(&std::fs::read_to_string(real.join("native-hooks.json")).unwrap())
                .unwrap();
        let mut inputs: Vec<String> = [
            "native-hooks.json",
            "tools/native-hook-fragments",
            "crates/horizon-client/src/game",
            "crates/horizon-client/src/mod_api.rs",
        ]
        .map(str::to_owned)
        .into();
        for hook in super::catalog_hooks(&real, &catalog).unwrap() {
            if let Some((krate, path)) =
                super::free_function_source(hook["target"].as_str().unwrap())
            {
                let (module, _) = path.rsplit_once("::").unwrap();
                inputs.push(format!(
                    "crates/{krate}/src/{}.rs",
                    module.replace("::", "/")
                ));
            }
        }
        for input in &inputs {
            copy(&real.join(input), &root.join(input));
        }
        let catalog_path = root.join("native-hooks.json");
        let original_catalog = std::fs::read_to_string(&catalog_path).unwrap();
        for group in ["helpers", "hooks"] {
            let mut missing: Value = serde_json::from_str(&original_catalog).unwrap();
            let entry = &mut missing[group][0];
            let name = entry["name"].as_str().unwrap().to_owned();
            entry.as_object_mut().unwrap().remove("side").unwrap();
            std::fs::write(&catalog_path, missing.to_string()).unwrap();
            let kind = if group == "hooks" { "hook" } else { "helper" };
            assert_eq!(
                super::sources(&root, super::FillMode::Require).unwrap_err(),
                format!("{kind} {name}: side missing")
            );
        }
        std::fs::write(&catalog_path, &original_catalog).unwrap();
        let without_counterparts = super::sources(&root, super::FillMode::Require)
            .unwrap()
            .hooks;
        let mut with_counterparts: Value = serde_json::from_str(&original_catalog).unwrap();
        let first = &mut with_counterparts["hooks"][0];
        first["fabric"] = serde_json::json!(["Example.ONE", "Example.TWO"]);
        first["paper"] = serde_json::json!("ExampleEvent");
        std::fs::write(&catalog_path, with_counterparts.to_string()).unwrap();
        assert_eq!(
            super::sources(&root, super::FillMode::Require)
                .unwrap()
                .hooks,
            without_counterparts
        );
        std::fs::write(&catalog_path, original_catalog).unwrap();
        let hooks = root.join("crates/horizon-client/src/game/native_hooks.rs");
        let shipped = std::fs::read_to_string(&hooks).unwrap();
        let call = "mod_api::dispatch_item_damaged(self, player, name, item, amount, cancel);";
        assert_eq!(shipped.matches(call).count(), 1);
        let unfinished = shipped.replace(call, "let _ = (player, name, item, amount);");
        std::fs::write(&hooks, &unfinished).unwrap();
        assert!(
            super::fill_hooks(&root)
                .unwrap_err()
                .starts_with("missing dispatch_item_damaged call in native_item_damaged: ")
        );
        assert_eq!(std::fs::read_to_string(&hooks).unwrap(), unfinished);
        let placeholder = shipped.replace(call, "let _ = (player, name, item, amount, cancel);");
        std::fs::write(&hooks, &placeholder).unwrap();
        assert_eq!(
            super::sources(&root, super::FillMode::Require).unwrap_err(),
            "native_item_damaged: the let _ = (…); placeholder needs its dispatch call; run cargo run -p xtask -- mod-api"
        );
        assert_eq!(std::fs::read_to_string(&hooks).unwrap(), placeholder);
        // A stamped hook lives in the catalog, not in a fragment.
        let catalog_path = root.join("native-hooks.json");
        let shipped_catalog = std::fs::read_to_string(&catalog_path).unwrap();
        let mut json: Value = serde_json::from_str(&shipped_catalog).unwrap();
        let hook = json["hooks"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|hook| hook["name"] == "item_damaged")
            .unwrap();
        hook.as_object_mut().unwrap().remove("mod_api").unwrap();
        std::fs::write(&catalog_path, json.to_string()).unwrap();
        assert_eq!(
            super::fill_hooks(&root).unwrap_err(),
            "hook item_damaged has no mod_api entry: add one"
        );
        std::fs::write(&catalog_path, shipped_catalog).unwrap();
        super::fill_hooks(&root).unwrap();
        // The generator writes LF; a Windows checkout (core.autocrlf) has CRLF.
        let lf = |s: &str| s.replace("\r\n", "\n");
        assert_eq!(lf(&std::fs::read_to_string(&hooks).unwrap()), lf(&shipped));
        assert_eq!(
            super::sources(&root, super::FillMode::Require)
                .unwrap()
                .hooks,
            super::sources(&real, super::FillMode::Require)
                .unwrap()
                .hooks,
        );
        super::sources(&root, super::FillMode::Require).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn event_fields_name_their_lifetimes() {
        for (ty, field, scalar, a, b) in [
            (
                "& mut WorldDrawn < '_ >",
                "& 'a mut WorldDrawn < 'b >",
                false,
                true,
                true,
            ),
            ("& str", "& 'a str", false, true, false),
            (
                "& mut Vec < (& 'static str , u32) >",
                "& 'a mut Vec < (& 'static str , u32) >",
                false,
                true,
                false,
            ),
            (
                "Option < & ItemStack >",
                "Option < & 'a ItemStack >",
                false,
                true,
                false,
            ),
            ("u32", "u32", false, false, false),
            ("& mut f32", "f32", true, false, false),
            (
                "& mut Option < bool >",
                "& 'a mut Option < bool >",
                false,
                true,
                false,
            ),
        ] {
            let mut lifetimes = super::EventLifetimes::default();
            let (field_type, is_scalar) = lifetimes.field(&syn::parse_str(ty).unwrap());
            assert_eq!(field_type.to_token_stream().to_string(), field);
            assert_eq!((is_scalar, lifetimes.a, lifetimes.b), (scalar, a, b));
        }
    }

    #[test]
    fn library_crate_targets_resolve_to_their_source() {
        assert_eq!(
            super::free_function_source("horizon_client_lib::menu::native_menu_slot_rules"),
            Some(("horizon-client", "menu::native_menu_slot_rules".to_owned()))
        );
        assert_eq!(
            super::free_function_source("horizon_client_lib::breathing::native_living_breathe"),
            Some((
                "horizon-client",
                "breathing::native_living_breathe".to_owned()
            ))
        );
        for target in [
            "horizon_client::menu::native_menu_slot_rules",
            "horizon_client::breathing::native_living_breathe",
        ] {
            assert_eq!(super::free_function_source(target), None);
        }
        assert_eq!(
            super::free_function_source("horizon_client_lib::crafting::native_ingredient"),
            Some(("horizon-client", "crafting::native_ingredient".to_owned()))
        );
        assert_eq!(
            super::free_function_source(
                "horizon_client::game::native_hooks::Game<'_>::native_zoom"
            ),
            None
        );
    }

    #[test]
    fn contracted_hook_requires_dispatch_call() {
        let missing: syn::ImplItemFn = syn::parse_quote! { fn native_zoom(&self) { unrelated(); } };
        let present: syn::ImplItemFn = syn::parse_quote! { fn native_zoom(&self) { horizon_client::mod_api::dispatch_zoom(); } };
        assert!(!super::has_call(&missing.block, "dispatch_zoom"));
        assert!(super::has_call(&present.block, "dispatch_zoom"));
    }
}
