//! The staged internals view (#589). Source copies only: the working tree keeps its visibility.
use proc_macro2::Span;
use quote::{ToTokens, quote};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};
use syn::{Fields, ImplItem, Item, Visibility, spanned::Spanned, visit::Visit};

/// One exposed signature or a reachable lower-crate type shape.
#[derive(Debug, serde::Serialize, PartialEq, Eq)]
pub struct Internal {
    pub path: String,
    pub kind: &'static str,
    /// The `cfg` it exists under (`all(...)` of several), empty where every build has it: a
    /// guarded alternative is listed under its own condition, not in place of the other.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub cfg: String,
    pub signature: String,
    pub hash: String,
    /// A reachable lower-crate type's layout, without exposing it to native mods.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub shape_only: bool,
    /// Type paths resolved through imports, keyed by their spelling in this signature.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub refs: BTreeMap<String, String>,
}

/// The result of widening a source copy, including the deterministic manifest.
#[derive(Debug, Default)]
pub struct Widened {
    pub entries: Vec<Internal>,
    /// Items whose visibility actually changed, grouped by kind.
    pub changed: BTreeMap<&'static str, usize>,
    pub files: usize,
}

struct Source {
    path: PathBuf,
    text: String,
    lines: Vec<usize>,
    parsed: syn::File,
    edits: BTreeMap<usize, (usize, &'static str)>,
    annotations: BTreeMap<usize, (usize, String)>,
}

impl Source {
    fn offset(&self, position: proc_macro2::LineColumn) -> usize {
        let line = self.lines[position.line - 1];
        self.text[line..]
            .char_indices()
            .nth(position.column)
            .map_or(self.text.len(), |(i, _)| line + i)
    }

    fn annotate(&mut self, attrs: &[syn::Attribute], start: Span, kind: &str, path: &str) {
        let note = format!("horizon-internal:{kind}:{path}");
        let attr = format!("#[deprecated(note = {note:?})]\n");
        // Replace an existing deprecation rather than creating a duplicate attribute.
        let existing = attrs.iter().find(|attr| attr.path().is_ident("deprecated"));
        let (start, end) = existing.map_or_else(
            || {
                let offset = self.offset(start.start());
                (offset, offset)
            },
            |attr| {
                (
                    self.offset(attr.span().start()),
                    self.offset(attr.span().end()),
                )
            },
        );
        self.annotations.insert(start, (end, attr));
    }

    fn public(&mut self, vis: &Visibility, start: Span, kind: &'static str) {
        if matches!(vis, Visibility::Public(_)) {
            return;
        }
        let (start, end) = match vis {
            Visibility::Inherited => (self.offset(start.start()), self.offset(start.start())),
            _ => (
                self.offset(vis.span().start()),
                self.offset(vis.span().end()),
            ),
        };
        self.edits.insert(start, (end, kind));
    }
}

type EntryKey = (String, &'static str, String);

#[derive(Clone, Default)]
struct TypeUses {
    paths: BTreeSet<String>,
    parameters: BTreeSet<String>,
}

impl<'ast> Visit<'ast> for TypeUses {
    fn visit_type_path(&mut self, ty: &'ast syn::TypePath) {
        if ty.qself.is_none() {
            let path = ty
                .path
                .segments
                .iter()
                .map(|s| s.ident.to_string())
                .collect::<Vec<_>>()
                .join("::");
            self.paths.insert(if ty.path.leading_colon.is_some() {
                format!("::{path}")
            } else {
                path
            });
        }
        syn::visit::visit_type_path(self, ty);
    }

    fn visit_type_param(&mut self, param: &'ast syn::TypeParam) {
        self.parameters.insert(param.ident.to_string());
        syn::visit::visit_type_param(self, param);
    }

    fn visit_block(&mut self, _: &'ast syn::Block) {}
    fn visit_expr(&mut self, _: &'ast syn::Expr) {}
    fn visit_item_mod(&mut self, _: &'ast syn::ItemMod) {}
}

impl TypeUses {
    fn item(item: &Item) -> Self {
        let mut uses = Self::default();
        uses.visit_item(item);
        uses
    }
}

#[derive(Default)]
struct Pass {
    sources: Vec<Source>,
    /// By path, kind and condition: a field and a method of one name are two entries, and so are
    /// two guarded alternatives of one item.
    entries: BTreeMap<EntryKey, Internal>,
    uses: BTreeMap<EntryKey, (String, TypeUses)>,
    /// The conditions of the module being visited.
    cfg: Vec<String>,
    discovery: bool,
}

// Only test-only declarations are excluded. Other cfgs keep their spelling and visibility, and
// each entry records them: changing visibility doesn't select an OS or feature for the build.
use crate::points::{conditions, test_only};

fn clean_attrs(attrs: &mut Vec<syn::Attribute>) {
    attrs.retain(|attr| attr.path().is_ident("cfg") || attr.path().is_ident("cfg_attr"));
}

fn clean_fields(fields: &Fields) -> Fields {
    let mut fields = fields.clone();
    match &mut fields {
        Fields::Named(fields) => {
            fields.named = fields
                .named
                .iter()
                .filter(|field| !test_only(&field.attrs))
                .cloned()
                .collect()
        }
        Fields::Unnamed(fields) => {
            fields.unnamed = fields
                .unnamed
                .iter()
                .filter(|field| !test_only(&field.attrs))
                .cloned()
                .collect()
        }
        Fields::Unit => {}
    }
    for field in &mut fields {
        clean_attrs(&mut field.attrs);
        field.vis = Visibility::Inherited;
    }
    fields
}

fn shape_signature(item: &Item) -> Option<Item> {
    match item {
        Item::Struct(value) => {
            let mut value = value.clone();
            value.attrs.clear();
            value.vis = Visibility::Inherited;
            value.fields = clean_fields(&value.fields);
            Some(Item::Struct(value))
        }
        Item::Enum(value) => {
            let mut value = value.clone();
            value.attrs.clear();
            value.vis = Visibility::Inherited;
            value.variants = value
                .variants
                .iter()
                .filter(|variant| !test_only(&variant.attrs))
                .cloned()
                .collect();
            for variant in &mut value.variants {
                clean_attrs(&mut variant.attrs);
                variant.fields = clean_fields(&variant.fields);
            }
            Some(Item::Enum(value))
        }
        _ => None,
    }
}

fn configuration(outer: &[String], attrs: &[syn::Attribute]) -> String {
    let all: Vec<String> = outer.iter().cloned().chain(conditions(attrs)).collect();
    match &all[..] {
        [] => String::new(),
        [one] => one.clone(),
        many => format!("all({})", many.join(", ")),
    }
}

#[derive(Clone)]
struct Definition {
    module: String,
    cfg: Vec<String>,
    item: Item,
}

#[derive(Default)]
struct Scope {
    names: BTreeSet<String>,
    imports: BTreeMap<String, Vec<String>>,
    globs: BTreeSet<String>,
}

impl Scope {
    fn import(&mut self, prefix: &str, tree: &syn::UseTree) {
        let join = |name: &str| {
            if prefix.is_empty() {
                name.to_owned()
            } else {
                format!("{prefix}::{name}")
            }
        };
        match tree {
            syn::UseTree::Path(path) => self.import(&join(&path.ident.to_string()), &path.tree),
            syn::UseTree::Name(name) => {
                let (name, path) = if name.ident == "self" {
                    (
                        prefix.rsplit("::").next().unwrap_or(prefix).to_owned(),
                        prefix.to_owned(),
                    )
                } else {
                    (name.ident.to_string(), join(&name.ident.to_string()))
                };
                self.imports.entry(name).or_default().push(path);
            }
            syn::UseTree::Rename(rename) => {
                let path = if rename.ident == "self" {
                    prefix.to_owned()
                } else {
                    join(&rename.ident.to_string())
                };
                self.imports
                    .entry(rename.rename.to_string())
                    .or_default()
                    .push(path);
            }
            syn::UseTree::Glob(_) => {
                self.globs.insert(prefix.to_owned());
            }
            syn::UseTree::Group(group) => {
                for tree in &group.items {
                    self.import(prefix, tree);
                }
            }
        }
    }
}

/// A source-level name index, not a compiler: macros and associated types remain opaque. Crates
/// are loaded only when a signature reaches them; no visibility edits are made through this index.
struct Shapes {
    roots: BTreeMap<String, PathBuf>,
    loaded: BTreeSet<String>,
    scopes: BTreeMap<String, Scope>,
    definitions: BTreeMap<String, Vec<Definition>>,
}

impl Shapes {
    fn new(root: &Path) -> Result<Self, String> {
        let mut roots = BTreeMap::new();
        for package in fs::read_dir(root.join("crates")).map_err(|e| e.to_string())? {
            let package = package.map_err(|e| e.to_string())?.path();
            let Some(name) = package.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if !name.starts_with("horizon-") || !package.join("Cargo.toml").is_file() {
                continue;
            }
            let (_, lib) = crate::points::crate_root(root, name)?;
            if !lib.is_file() {
                continue;
            }
            let text = fs::read_to_string(package.join("Cargo.toml")).map_err(|e| e.to_string())?;
            let manifest: toml::Value = toml::from_str(&text).map_err(|e| e.to_string())?;
            let name = manifest
                .get("lib")
                .and_then(|lib| lib.get("name"))
                .and_then(toml::Value::as_str)
                .map_or_else(|| name.replace('-', "_"), str::to_owned);
            roots.insert(name, lib);
        }
        Ok(Self {
            roots,
            loaded: BTreeSet::new(),
            scopes: BTreeMap::new(),
            definitions: BTreeMap::new(),
        })
    }

    fn load(&mut self, krate: &str) -> Result<(), String> {
        if self.loaded.contains(krate) {
            return Ok(());
        }
        let Some(lib) = self.roots.get(krate) else {
            return Ok(());
        };
        let dir = lib.parent().expect("source parent");
        let files =
            crate::points::module_files(krate.to_owned(), lib.clone(), dir.to_owned(), false)?;
        self.loaded.insert(krate.to_owned());
        for file in files {
            let text = fs::read_to_string(&file.path)
                .map_err(|e| format!("{}: {e}", file.path.display()))?;
            let parsed =
                syn::parse_file(&text).map_err(|e| format!("{}: {e}", file.path.display()))?;
            self.items(&file.module, &file.cfg, &parsed.items);
        }
        Ok(())
    }

    fn items(&mut self, module: &str, cfg: &[String], items: &[Item]) {
        self.scopes.entry(module.to_owned()).or_default();
        for item in items {
            let (ident, attrs) = match item {
                Item::Struct(value) => (&value.ident, &value.attrs),
                Item::Enum(value) => (&value.ident, &value.attrs),
                Item::Type(value) => (&value.ident, &value.attrs),
                Item::Union(value) => (&value.ident, &value.attrs),
                Item::Trait(value) => (&value.ident, &value.attrs),
                Item::Mod(value) => (&value.ident, &value.attrs),
                Item::Use(value) if !test_only(&value.attrs) => {
                    self.scopes
                        .get_mut(module)
                        .expect("scope")
                        .import("", &value.tree);
                    continue;
                }
                Item::ExternCrate(value) if !test_only(&value.attrs) => {
                    let name = value
                        .rename
                        .as_ref()
                        .map_or(&value.ident, |(_, name)| name)
                        .to_string();
                    let path = if value.ident == "self" {
                        "crate".to_owned()
                    } else {
                        value.ident.to_string()
                    };
                    self.scopes
                        .get_mut(module)
                        .expect("scope")
                        .imports
                        .entry(name)
                        .or_default()
                        .push(path);
                    continue;
                }
                _ => continue,
            };
            if test_only(attrs) {
                continue;
            }
            self.scopes
                .get_mut(module)
                .expect("scope")
                .names
                .insert(ident.to_string());
            let path = format!("{module}::{ident}");
            if let Item::Mod(value) = item {
                if let Some((_, inner)) = &value.content {
                    let cfg: Vec<_> = cfg.iter().cloned().chain(conditions(attrs)).collect();
                    self.items(&path, &cfg, inner);
                }
            } else if matches!(item, Item::Struct(_) | Item::Enum(_) | Item::Type(_)) {
                self.definitions.entry(path).or_default().push(Definition {
                    module: module.to_owned(),
                    cfg: cfg.to_vec(),
                    item: item.clone(),
                });
            }
        }
    }

    fn resolve(
        &mut self,
        module: &str,
        path: &str,
        seen: &mut BTreeSet<(String, String)>,
    ) -> Result<BTreeMap<String, bool>, String> {
        if !seen.insert((module.to_owned(), path.to_owned())) {
            return Ok(BTreeMap::new());
        }
        if let Some(path) = path.strip_prefix("::") {
            let parts: Vec<_> = path.split("::").collect();
            if !self.roots.contains_key(parts[0]) {
                return Ok(BTreeMap::new());
            }
            self.load(parts[0])?;
            return self.tail(parts[0], &parts[1..], seen);
        }
        let parts: Vec<_> = path.split("::").collect();
        let first = parts[0];
        let krate = module.split("::").next().expect("crate");
        self.load(krate)?;
        let tail = &parts[1..];
        match first {
            "crate" => return self.tail(krate, tail, seen),
            "self" => return self.tail(module, tail, seen),
            "super" => {
                return match module.rsplit_once("::") {
                    Some((parent, _)) => self.tail(parent, tail, seen),
                    None => Ok(BTreeMap::new()),
                };
            }
            "std" | "core" | "alloc" => return Ok(BTreeMap::from([(path.to_owned(), false)])),
            _ => {}
        }
        if self
            .scopes
            .get(module)
            .is_some_and(|scope| scope.names.contains(first))
        {
            return self.tail(&format!("{module}::{first}"), tail, seen);
        }
        let imports = self
            .scopes
            .get(module)
            .and_then(|scope| scope.imports.get(first))
            .cloned();
        if let Some(imports) = imports {
            let mut found = BTreeMap::new();
            for import in imports {
                let path = if tail.is_empty() {
                    import
                } else {
                    format!("{import}::{}", tail.join("::"))
                };
                for (path, _) in self.resolve(module, &path, seen)? {
                    found.insert(path, true);
                }
            }
            return Ok(found);
        }
        if self.roots.contains_key(first) {
            self.load(first)?;
            return self.tail(first, tail, seen);
        }
        let globs = self
            .scopes
            .get(module)
            .map(|scope| scope.globs.clone())
            .unwrap_or_default();
        let mut found = BTreeMap::new();
        for glob in globs {
            for (path, _) in self.resolve(module, &format!("{glob}::{path}"), seen)? {
                found.insert(path, true);
            }
        }
        if !found.is_empty() {
            return Ok(found);
        }
        if module != krate {
            return self.resolve(krate, path, seen);
        }
        Ok(found)
    }

    fn tail(
        &mut self,
        base: &str,
        tail: &[&str],
        seen: &mut BTreeSet<(String, String)>,
    ) -> Result<BTreeMap<String, bool>, String> {
        if tail.is_empty() {
            return Ok(BTreeMap::from([(base.to_owned(), false)]));
        }
        if self.scopes.contains_key(base) {
            self.resolve(base, &tail.join("::"), seen)
        } else {
            Ok(BTreeMap::new())
        }
    }
}

fn owner(ty: &syn::Type) -> String {
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

fn type_path(module: &str, ident: &syn::Ident, generics: &syn::Generics) -> String {
    let (_, arguments, _) = generics.split_for_impl();
    let ty: syn::Type = syn::parse_quote!(#ident #arguments);
    format!("{module}::{}", owner(&ty))
}

impl Pass {
    fn load(&mut self, path: &Path) -> Result<usize, String> {
        if let Some(index) = self.sources.iter().position(|source| source.path == path) {
            return Ok(index);
        }
        let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let parsed = syn::parse_file(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        let lines = std::iter::once(0)
            .chain(text.match_indices('\n').map(|(i, _)| i + 1))
            .collect();
        let index = self.sources.len();
        self.sources.push(Source {
            path: path.to_owned(),
            text,
            parsed,
            lines,
            edits: BTreeMap::new(),
            annotations: BTreeMap::new(),
        });
        Ok(index)
    }

    fn entry(
        &mut self,
        module: &str,
        path: &str,
        kind: &'static str,
        attrs: &[syn::Attribute],
        tokens: impl ToTokens,
        uses: TypeUses,
    ) {
        let cfg = configuration(&self.cfg, attrs);
        let signature = tokens.to_token_stream().to_string();
        let digest = Sha256::digest(signature.as_bytes());
        let hash = digest[..8]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let key = (path.to_owned(), kind, cfg.clone());
        self.uses.insert(key.clone(), (module.to_owned(), uses));
        self.entries.insert(
            key,
            Internal {
                path: path.to_owned(),
                kind,
                cfg,
                signature,
                hash,
                shape_only: false,
                refs: BTreeMap::new(),
            },
        );
    }

    fn fields(
        &mut self,
        file: usize,
        module: &str,
        path: &str,
        fields: &Fields,
        generics: &syn::Generics,
        widen: bool,
    ) {
        for (index, field) in fields.iter().enumerate() {
            if test_only(&field.attrs) {
                continue;
            }
            let name = field
                .ident
                .as_ref()
                .map_or_else(|| index.to_string(), ToString::to_string);
            if widen {
                let start = field
                    .ident
                    .as_ref()
                    .map_or_else(|| field.ty.span(), |ident| ident.span());
                self.sources[file].public(&field.vis, start, "field");
            }
            if self.discovery {
                self.sources[file].annotate(
                    &field.attrs,
                    field.span(),
                    "field",
                    &format!("{path}::{name}"),
                );
            }
            let mut uses = TypeUses::default();
            uses.parameters
                .extend(generics.type_params().map(|param| param.ident.to_string()));
            uses.visit_type(&field.ty);
            self.entry(
                module,
                &format!("{path}::{name}"),
                "field",
                &field.attrs,
                &field.ty,
                uses,
            );
        }
    }

    /// A member of `block`: its fingerprint includes the `impl`'s generics and bounds, which decide
    /// who may call it as much as its own signature does.
    fn member(
        &mut self,
        file: usize,
        module: &str,
        path: &str,
        block: &syn::ItemImpl,
        item: &ImplItem,
    ) {
        let (impl_generics, self_ty) = (&block.generics, &block.self_ty);
        let where_clause = &impl_generics.where_clause;
        let mut uses = TypeUses::default();
        uses.visit_generics(impl_generics);
        uses.visit_type(self_ty);
        uses.visit_impl_item(item);
        match item {
            ImplItem::Fn(method) if !test_only(&method.attrs) => {
                if self.discovery {
                    self.sources[file].annotate(
                        &method.attrs,
                        method.span(),
                        "method",
                        &format!("{path}::{}", method.sig.ident),
                    );
                }
                self.sources[file].public(&method.vis, method.sig.span(), "method");
                let sig = &method.sig;
                self.entry(
                    module,
                    &format!("{path}::{}", method.sig.ident),
                    "method",
                    &method.attrs,
                    quote!(impl #impl_generics #self_ty #where_clause { #sig }),
                    uses,
                );
            }
            ImplItem::Const(value) if !test_only(&value.attrs) => {
                if self.discovery {
                    self.sources[file].annotate(
                        &value.attrs,
                        value.span(),
                        "const",
                        &format!("{path}::{}", value.ident),
                    );
                }
                self.sources[file].public(&value.vis, value.const_token.span(), "const");
                let (ident, generics, ty) = (&value.ident, &value.generics, &value.ty);
                self.entry(
                    module,
                    &format!("{path}::{ident}"),
                    "const",
                    &value.attrs,
                    quote!(impl #impl_generics #self_ty #where_clause { const #ident #generics: #ty }),
                    uses,
                );
            }
            ImplItem::Type(value) if !test_only(&value.attrs) => {
                if self.discovery {
                    self.sources[file].annotate(
                        &value.attrs,
                        value.span(),
                        "type",
                        &format!("{path}::{}", value.ident),
                    );
                }
                self.sources[file].public(&value.vis, value.type_token.span(), "type");
                let (ident, generics, ty) = (&value.ident, &value.generics, &value.ty);
                self.entry(
                    module,
                    &format!("{path}::{ident}"),
                    "type",
                    &value.attrs,
                    quote!(impl #impl_generics #self_ty #where_clause { type #ident #generics = #ty; }),
                    uses,
                );
            }
            _ => {}
        }
    }

    // Does not visit expressions or blocks: local items, macros, uses and trait impls stay intact.
    fn items(&mut self, file: usize, module: &str, items: &[Item]) {
        for item in items {
            self.item(file, module, item, true);
        }
    }

    fn item(&mut self, file: usize, module: &str, item: &Item, children: bool) {
        macro_rules! expose {
            ($value:expr, $kind:literal, $start:expr, $signature:expr) => {{
                let value = $value;
                if test_only(&value.attrs) {
                    return;
                }
                let path = format!("{module}::{}", value.ident);
                if self.discovery {
                    self.sources[file].annotate(&value.attrs, value.span(), $kind, &path);
                }
                self.sources[file].public(&value.vis, $start, $kind);
                self.entry(
                    module,
                    &path,
                    $kind,
                    &value.attrs,
                    $signature,
                    TypeUses::item(item),
                );
                path
            }};
        }
        match item {
            Item::Fn(value) if !test_only(&value.attrs) => {
                if self.discovery {
                    self.sources[file].annotate(
                        &value.attrs,
                        value.span(),
                        "fn",
                        &format!("{module}::{}", value.sig.ident),
                    );
                }
                self.sources[file].public(&value.vis, value.sig.span(), "fn");
                self.entry(
                    module,
                    &format!("{module}::{}", value.sig.ident),
                    "fn",
                    &value.attrs,
                    &value.sig,
                    TypeUses::item(item),
                );
            }
            Item::Struct(value) => {
                let (ident, generics) = (&value.ident, &value.generics);
                let signature = shape_signature(item).expect("a struct");
                expose!(value, "struct", value.struct_token.span(), signature);
                if children {
                    self.fields(
                        file,
                        module,
                        &type_path(module, ident, generics),
                        &value.fields,
                        &value.generics,
                        true,
                    );
                }
            }
            Item::Union(value) => {
                let (ident, generics) = (&value.ident, &value.generics);
                let mut signature = value.clone();
                signature.attrs.clear();
                signature.vis = Visibility::Inherited;
                let Fields::Named(fields) = clean_fields(&Fields::Named(signature.fields)) else {
                    unreachable!()
                };
                signature.fields = fields;
                expose!(value, "struct", value.union_token.span(), signature);
                if children {
                    self.fields(
                        file,
                        module,
                        &type_path(module, ident, generics),
                        &Fields::Named(value.fields.clone()),
                        &value.generics,
                        true,
                    );
                }
            }
            Item::Enum(value) => {
                let signature = shape_signature(item).expect("an enum");
                let path = expose!(value, "enum", value.enum_token.span(), signature);
                if children {
                    for variant in &value.variants {
                        if !test_only(&variant.attrs) {
                            self.fields(
                                file,
                                module,
                                &format!("{path}::{}", variant.ident),
                                &variant.fields,
                                &value.generics,
                                false,
                            );
                        }
                    }
                }
            }
            Item::Const(value) => {
                let (ident, generics, ty) = (&value.ident, &value.generics, &value.ty);
                expose!(
                    value,
                    "const",
                    value.const_token.span(),
                    quote!(const #ident #generics: #ty)
                );
            }
            Item::Static(value) => {
                let (ident, mutability, ty) = (&value.ident, &value.mutability, &value.ty);
                expose!(
                    value,
                    "static",
                    value.static_token.span(),
                    quote!(static #mutability #ident: #ty)
                );
            }
            Item::Type(value) => {
                let mut signature = value.clone();
                signature.attrs.clear();
                signature.vis = Visibility::Inherited;
                expose!(value, "type", value.type_token.span(), signature);
            }
            Item::Trait(value) => {
                let mut signature = value.clone();
                signature.attrs.clear();
                signature.vis = Visibility::Inherited;
                // Trait method defaults are implementations, and docs aren't contract: neither is
                // part of the signature.
                for member in &mut signature.items {
                    match member {
                        syn::TraitItem::Fn(method) => {
                            method.attrs.retain(|attr| !attr.path().is_ident("doc"));
                            method.default = None;
                            method.semi_token = Some(Default::default());
                        }
                        syn::TraitItem::Const(value) => {
                            value.attrs.retain(|attr| !attr.path().is_ident("doc"))
                        }
                        syn::TraitItem::Type(value) => {
                            value.attrs.retain(|attr| !attr.path().is_ident("doc"))
                        }
                        _ => {}
                    }
                }
                expose!(value, "trait", value.trait_token.span(), signature);
            }
            Item::Mod(value) => {
                let ident = &value.ident;
                let path = expose!(value, "mod", value.mod_token.span(), quote!(mod #ident));
                if children && let Some((_, items)) = &value.content {
                    let outer = self.cfg.len();
                    self.cfg.extend(conditions(&value.attrs));
                    self.items(file, &path, items);
                    self.cfg.truncate(outer);
                }
            }
            Item::Impl(value) if children && value.trait_.is_none() && !test_only(&value.attrs) => {
                let path = format!("{module}::{}", owner(&value.self_ty));
                let outer = self.cfg.len();
                self.cfg.extend(conditions(&value.attrs));
                for member in &value.items {
                    self.member(file, module, &path, value, member);
                }
                self.cfg.truncate(outer);
            }
            _ => {}
        }
    }

    fn lower(&mut self, root: &Path, path: &str) -> Result<(), String> {
        self.cfg.clear();
        let parsed: syn::Path = syn::parse_str(path).map_err(|e| format!("invalid path: {e}"))?;
        let mut segments = parsed.segments.iter();
        let crate_name = segments.next().ok_or("empty path")?.ident.to_string();
        let (_, lib) = crate::points::crate_root(root, &crate_name)?;
        let mut file = self.load(&lib)?;
        let mut dir = self.sources[file]
            .path
            .parent()
            .expect("source parent")
            .to_owned();
        let mut items = self.sources[file].parsed.items.clone();
        let mut module = crate_name;
        let segments: Vec<_> = segments.collect();
        for (index, segment) in segments.iter().enumerate() {
            let ident = &segment.ident;
            let child = items.iter().find_map(|item| match item {
                Item::Mod(value) if value.ident == *ident && !test_only(&value.attrs) => {
                    Some(value)
                }
                _ => None,
            });
            if let Some(child) = child {
                self.item(file, &module, &Item::Mod(child.clone()), false);
                module = format!("{module}::{ident}");
                if index + 1 == segments.len() {
                    return Ok(());
                }
                self.cfg.extend(conditions(&child.attrs));
                if let Some((_, inner)) = &child.content {
                    items = inner.clone();
                    dir = dir.join(ident.to_string());
                } else {
                    let explicit = child.attrs.iter().find_map(|attr| match &attr.meta {
                        syn::Meta::NameValue(value) if attr.path().is_ident("path") => {
                            match &value.value {
                                syn::Expr::Lit(syn::ExprLit {
                                    lit: syn::Lit::Str(path),
                                    ..
                                }) => Some(path.value()),
                                _ => None,
                            }
                        }
                        _ => None,
                    });
                    let child_path = if let Some(relative) = explicit {
                        self.sources[file]
                            .path
                            .parent()
                            .expect("source parent")
                            .join(relative)
                    } else if dir.join(format!("{ident}.rs")).is_file() {
                        dir.join(format!("{ident}.rs"))
                    } else {
                        dir.join(ident.to_string()).join("mod.rs")
                    };
                    dir = if child_path.file_name().is_some_and(|name| name == "mod.rs") {
                        child_path.parent().expect("source parent").to_owned()
                    } else {
                        child_path.with_extension("")
                    };
                    file = self.load(&child_path)?;
                    items = self.sources[file].parsed.items.clone();
                }
                continue;
            }
            let rest = &segments[index..];
            if rest.len() == 1 {
                let selected = items
                    .iter()
                    .find(|item| match item {
                        Item::Fn(value) => value.sig.ident == *ident && !test_only(&value.attrs),
                        Item::Struct(value) => value.ident == *ident && !test_only(&value.attrs),
                        Item::Enum(value) => value.ident == *ident && !test_only(&value.attrs),
                        Item::Union(value) => value.ident == *ident && !test_only(&value.attrs),
                        Item::Const(value) => value.ident == *ident && !test_only(&value.attrs),
                        Item::Static(value) => value.ident == *ident && !test_only(&value.attrs),
                        Item::Type(value) => value.ident == *ident && !test_only(&value.attrs),
                        Item::Trait(value) => value.ident == *ident && !test_only(&value.attrs),
                        _ => false,
                    })
                    .ok_or("item not found")?;
                self.item(file, &module, selected, false);
                return Ok(());
            }
            if rest.len() == 2 {
                let member_name = &rest[1].ident;
                let mut found = false;
                for item in &items {
                    if let Item::Impl(block) = item
                        && block.trait_.is_none()
                        && !test_only(&block.attrs)
                        && let syn::Type::Path(ty) = &*block.self_ty
                        && ty
                            .path
                            .segments
                            .last()
                            .is_some_and(|part| part.ident == *ident)
                    {
                        for member in &block.items {
                            let matches = match member {
                                ImplItem::Fn(value) => {
                                    value.sig.ident == *member_name && !test_only(&value.attrs)
                                }
                                ImplItem::Const(value) => {
                                    value.ident == *member_name && !test_only(&value.attrs)
                                }
                                ImplItem::Type(value) => {
                                    value.ident == *member_name && !test_only(&value.attrs)
                                }
                                _ => false,
                            };
                            if matches {
                                self.member(
                                    file,
                                    &module,
                                    &format!("{module}::{}", owner(&block.self_ty)),
                                    block,
                                    member,
                                );
                                found = true;
                            }
                        }
                    }
                }
                if found {
                    for item in &items {
                        if matches!(item, Item::Struct(value) if value.ident == *ident)
                            || matches!(item, Item::Enum(value) if value.ident == *ident)
                            || matches!(item, Item::Union(value) if value.ident == *ident)
                        {
                            self.item(file, &module, item, false);
                        }
                    }
                    return Ok(());
                }
            }
            return Err("item not found".into());
        }
        Err("path names a crate, not an item".into())
    }

    fn shapes(&mut self, root: &Path) -> Result<(), String> {
        let mut shapes = Shapes::new(root)?;
        let mut aliases = BTreeSet::new();
        while let Some((key, (module, uses))) = self.uses.pop_first() {
            for written in uses.paths {
                if uses
                    .parameters
                    .contains(written.split("::").next().expect("type path"))
                    || written == "Self"
                {
                    continue;
                }
                let resolved = shapes.resolve(&module, &written, &mut BTreeSet::new())?;
                for (path, imported) in resolved {
                    if imported && let Some(entry) = self.entries.get_mut(&key) {
                        entry.refs.insert(written.clone(), path.clone());
                    }
                    let Some(definitions) = shapes.definitions.get(&path).cloned() else {
                        continue;
                    };
                    for definition in definitions {
                        let (kind, attrs) = match &definition.item {
                            Item::Struct(value) => ("struct", &value.attrs),
                            Item::Enum(value) => ("enum", &value.attrs),
                            Item::Type(value) => ("type", &value.attrs),
                            _ => unreachable!("type definition"),
                        };
                        let cfg = configuration(&definition.cfg, attrs);
                        let key = (path.clone(), kind, cfg);
                        if kind == "type" {
                            if aliases.insert(key.clone()) {
                                self.uses.entry(key).or_insert_with(|| {
                                    (definition.module.clone(), TypeUses::item(&definition.item))
                                });
                            }
                        } else if !path.starts_with("horizon_client::")
                            && !self.entries.contains_key(&key)
                        {
                            self.cfg = definition.cfg;
                            self.entry(
                                &definition.module,
                                &path,
                                kind,
                                attrs,
                                shape_signature(&definition.item).expect("shape"),
                                TypeUses::item(&definition.item),
                            );
                            self.entries
                                .get_mut(&key)
                                .expect("inserted shape")
                                .shape_only = true;
                        }
                    }
                }
            }
        }
        self.cfg.clear();
        Ok(())
    }

    fn finish(self) -> Result<Widened, String> {
        let mut widened = Widened {
            entries: self.entries.into_values().collect(),
            ..Widened::default()
        };
        for mut source in self.sources {
            if source.edits.is_empty() && source.annotations.is_empty() {
                continue;
            }
            let mut edits = Vec::new();
            for (start, (end, kind)) in source.edits {
                edits.push((
                    start,
                    1,
                    end,
                    if start == end { "pub " } else { "pub" }.to_owned(),
                ));
                *widened.changed.entry(kind).or_default() += 1;
            }
            for (start, (end, text)) in source.annotations {
                edits.push((start, 0, end, text));
            }
            edits.sort_by_key(|(start, order, _, _)| (*start, *order));
            for (start, _, end, text) in edits.into_iter().rev() {
                source.text.replace_range(start..end, &text);
            }
            fs::write(&source.path, source.text)
                .map_err(|e| format!("{}: {e}", source.path.display()))?;
            widened.files += 1;
        }
        Ok(widened)
    }
}

/// Widen game internals and the declared lower-crate items in a staged source copy.
/// Fails before writing anything if a declaration in `internals.txt` cannot be resolved.
/// A listed lower-crate point is an internal too: its raw listener names the function's types.
pub fn widen(root: &Path) -> Result<Widened, String> {
    stage(root, false)
}

/// Instrument a discovery-only source copy. Its catalog is identical to [`widen`]'s, but
/// compiler deprecation diagnostics identify each use by kind and catalog path.
/// Never use this copy for a normal build or a published prebuilt.
pub fn discover(root: &Path) -> Result<Widened, String> {
    stage(root, true)
}

fn stage(root: &Path, discovery: bool) -> Result<Widened, String> {
    let mut lists = Vec::new();
    for name in ["internals.txt", "points.txt"] {
        let path = root.join("crates/horizon-mod-api-gen").join(name);
        lists.push((
            name,
            fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?,
        ));
    }
    widen_mode(root, &lists, discovery)
}

/// [`widen`] with each list's file name and text, so an error names the file and line it came from.
#[cfg(test)]
fn widen_with(root: &Path, lists: &[(&str, String)]) -> Result<Widened, String> {
    widen_mode(root, lists, false)
}

fn widen_mode(root: &Path, lists: &[(&str, String)], discovery: bool) -> Result<Widened, String> {
    let mut pass = Pass {
        discovery,
        ..Pass::default()
    };
    let src = root.join("crates/horizon-client/src");
    for source in crate::points::game_files(&src, false)?
        .into_iter()
        .chain(std::iter::once(crate::points::GameFile {
            module: "horizon_client::entry".into(),
            path: src.join("entry.rs"),
            cfg: Vec::new(),
        }))
    {
        let file = pass.load(&source.path)?;
        pass.cfg = source.cfg;
        pass.items(
            file,
            &source.module,
            &pass.sources[file].parsed.items.clone(),
        );
    }
    pass.cfg.clear();
    for (name, list) in lists {
        for (line, declaration) in list.lines().enumerate() {
            let path = declaration.split('#').next().unwrap_or_default().trim();
            if !path.is_empty() {
                pass.lower(root, path)
                    .map_err(|e| format!("{name}:{}: {path}: {e}", line + 1))?;
            }
        }
    }
    pass.shapes(root)?;
    if discovery {
        let crates: BTreeSet<_> = pass
            .entries
            .values()
            .filter(|entry| !entry.shape_only)
            .filter_map(|entry| entry.path.split("::").next())
            .map(str::to_owned)
            .collect();
        for krate in crates {
            let (_, lib) = crate::points::crate_root(root, &krate)?;
            let file = pass.load(&lib)?;
            let start = pass.sources[file]
                .parsed
                .shebang
                .as_ref()
                .map_or(0, |s| s.len() + 1);
            pass.sources[file]
                .annotations
                .entry(start)
                .and_modify(|(_, text)| text.insert_str(0, "#![allow(deprecated)]\n"))
                .or_insert_with(|| (start, "#![allow(deprecated)]\n".into()));
        }
    }
    pass.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Fixture(PathBuf);
    impl Fixture {
        fn new(game: &str) -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let root = std::env::temp_dir().join(format!(
                "internals-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let fixture = Self(root);
            fixture.put(
                "crates/horizon-client/Cargo.toml",
                "[package]\nname = 'horizon-client'\nversion = '0.1.0'\n",
            );
            fixture.put("crates/horizon-client/src/lib.rs", "mod game; mod entry;\n");
            fixture.put("crates/horizon-client/src/game.rs", game);
            fixture.put("crates/horizon-client/src/entry.rs", "fn start() {}\n");
            fixture
        }
        fn put(&self, path: &str, text: &str) {
            let path = self.0.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        fn game(&self) -> String {
            fs::read_to_string(self.0.join("crates/horizon-client/src/game.rs")).unwrap()
        }
        fn lower(&self) {
            self.put(
                "crates/lower/Cargo.toml",
                "[package]\nname = 'lower'\nversion = '0.1.0'\n",
            );
            self.put("crates/lower/src/lib.rs", "mod hidden;\n");
            self.put(
                "crates/lower/src/hidden.rs",
                "#[path = \"other.rs\"] mod inner;\nfn sibling() {}\n",
            );
            self.put(
                "crates/lower/src/other.rs",
                "struct Thing(u32); impl Thing { fn run(&self) {} fn unrelated(&self) {} }\n",
            );
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    fn shapes_fixture(game: &str) -> Fixture {
        let fixture = Fixture::new(game);
        fixture.put(
            "crates/horizon-items/Cargo.toml",
            "[package]\nname = 'horizon-items'\nversion = '0.1.0'\n[lib]\npath = 'src/items.rs'\n",
        );
        fixture.put(
            "crates/horizon-items/src/items.rs",
            "mod item_stack; mod kinds; pub use item_stack::ItemStack;\n",
        );
        fixture.put(
            "crates/horizon-items/src/item_stack.rs",
            r#"use super::kinds::Kind as StackKind;
/// Source docs are not part of the shape.
#[derive(Clone)]
pub(crate) struct ItemStack { #[allow(dead_code)] kind: StackKind }
"#,
        );
        fixture.put(
            "crates/horizon-items/src/kinds.rs",
            "#[cfg(feature = \"extra\")]\nenum Kind { Empty, Count(#[allow(dead_code)] u32) }\n",
        );
        fixture
    }

    #[test]
    fn discovery_keeps_the_catalog_and_normal_output_and_reports_compiler_uses() {
        let game = "struct Thing { field: u32 }\nimpl Thing { fn method(&self) -> u32 { self.field } }\nfn free() {}\n";
        let normal = Fixture::new(game);
        let repeat = Fixture::new(game);
        let discovery = Fixture::new(game);
        for fixture in [&normal, &repeat, &discovery] {
            fixture.put(
                "crates/horizon-client/src/lib.rs",
                "pub mod game; mod entry;\n",
            );
            fixture.put(
                "crates/lower/Cargo.toml",
                "[package]\nname='lower'\nversion='0.1.0'\n",
            );
            fixture.put("crates/lower/src/lib.rs", "pub fn direct() {}\n");
        }
        let lists = [("internals.txt", "lower::direct".into())];
        let expected = widen_with(&normal.0, &lists).unwrap();
        let ordinary = widen_mode(&repeat.0, &lists, false).unwrap();
        let marked = widen_mode(&discovery.0, &lists, true).unwrap();
        assert_eq!(expected.entries, marked.entries);
        assert_eq!(
            serde_json::to_vec(&expected.entries).unwrap(),
            serde_json::to_vec(&ordinary.entries).unwrap()
        );
        assert_eq!(normal.game(), repeat.game());
        assert!(!normal.game().contains("horizon-internal:"));
        assert!(
            discovery
                .game()
                .contains("horizon-internal:field:horizon_client::game::Thing::field")
        );
        let out = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/isync-rustc-fixture")
            .join(std::process::id().to_string());
        fs::create_dir_all(&out).unwrap();
        let library = out.join("libhorizon_client.rlib");
        let compiled = std::process::Command::new("rustc")
            .args([
                "--edition=2024",
                "--crate-type=lib",
                "--crate-name=horizon_client",
                "--error-format=json",
            ])
            .arg(discovery.0.join("crates/horizon-client/src/lib.rs"))
            .arg("-o")
            .arg(&library)
            .output()
            .unwrap();
        assert!(
            compiled.status.success(),
            "{}",
            String::from_utf8_lossy(&compiled.stderr)
        );
        assert!(
            !String::from_utf8_lossy(&compiled.stderr)
                .lines()
                .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
                .any(|message| message["code"]["code"] == "deprecated"),
            "game must suppress its own deprecations"
        );
        let lower = out.join("liblower.rlib");
        let compiled = std::process::Command::new("rustc")
            .args(["--edition=2024", "--crate-type=lib", "--crate-name=lower"])
            .arg(discovery.0.join("crates/lower/src/lib.rs"))
            .arg("-o")
            .arg(&lower)
            .output()
            .unwrap();
        assert!(
            compiled.status.success(),
            "{}",
            String::from_utf8_lossy(&compiled.stderr)
        );
        let mod_source = out.join("mod.rs");
        fs::write(&mod_source, "use horizon_client::game::*; macro_rules! run { ($t:expr) => { $t.method() } } pub fn uses(t: &Thing) -> u32 { free(); lower::direct(); run!(t) + t.field }").unwrap();
        let checked = std::process::Command::new("rustc")
            .args([
                "--edition=2024",
                "--crate-type=lib",
                "--crate-name=example",
                "--emit=metadata",
                "--error-format=json",
                "--extern",
            ])
            .arg(format!("horizon_client={}", library.display()))
            .arg("--extern")
            .arg(format!("lower={}", lower.display()))
            .arg(&mod_source)
            .arg("-o")
            .arg(out.join("mod.rmeta"))
            .output()
            .unwrap();
        assert!(
            checked.status.success(),
            "{}",
            String::from_utf8_lossy(&checked.stderr)
        );
        let messages = String::from_utf8(checked.stderr).unwrap();
        for note in [
            "struct:horizon_client::game::Thing",
            "field:horizon_client::game::Thing::field",
            "method:horizon_client::game::Thing::method",
            "fn:horizon_client::game::free",
            "fn:lower::direct",
        ] {
            assert!(
                messages.contains(&format!("horizon-internal:{note}")),
                "{note}: {messages}"
            );
        }
        fs::remove_dir_all(out).unwrap();
    }

    #[test]
    fn signatures_keep_field_and_variant_configuration() {
        let fixture = Fixture::new(
            r#"
struct Game {
    #[cfg(windows)] #[cfg_attr(unix, cfg(feature = "extra"))] #[doc = "hidden"] #[serde(default)] windows: u32,
    #[cfg(test)] test_field: u32,
}
enum Choice {
    #[cfg(windows)] #[doc = "hidden"] #[serde(rename = "x")] Windows,
    #[cfg(test)] Test,
}
union Value { #[cfg(windows)] windows: u32, #[cfg(test)] test_field: u32 }
"#,
        );
        let entries = widen_with(&fixture.0, &[]).unwrap().entries;
        let signature = |name: &str| {
            &entries
                .iter()
                .find(|e| e.path == format!("horizon_client::game::{name}"))
                .unwrap()
                .signature
        };
        assert!(signature("Game").contains("cfg_attr"));
        for name in ["Game", "Choice", "Value"] {
            let signature = signature(name);
            assert!(signature.contains("cfg (windows)"), "{signature}");
            assert!(
                !signature.contains("test_field") && !signature.contains("Test"),
                "{signature}"
            );
            assert!(
                !signature.contains("doc") && !signature.contains("serde"),
                "{signature}"
            );
        }
    }

    #[test]
    fn ordinary_entry_resolves_glob_and_std_imports() {
        let fixture = shapes_fixture(
            "use horizon_items::kinds::*; use std::cell::Cell; struct Game { kind: Kind, cell: Cell<u32> }\n",
        );
        let entries = widen_with(&fixture.0, &[]).unwrap().entries;
        let game = entries
            .iter()
            .find(|e| e.path == "horizon_client::game::Game")
            .unwrap();
        assert!(!game.shape_only);
        assert_eq!(game.refs["Kind"], "horizon_items::kinds::Kind");
        assert_eq!(game.refs["Cell"], "std::cell::Cell");
    }

    #[test]
    fn reachable_lower_shapes_are_transitive_and_do_not_widen_sources() {
        let fixture = shapes_fixture(
            "use std::time::Duration; struct Game { stack: horizon_items::ItemStack, external: Duration }\n",
        );
        let paths = ["items.rs", "item_stack.rs", "kinds.rs"];
        let before: Vec<_> = paths
            .iter()
            .map(|path| fs::read(fixture.0.join("crates/horizon-items/src").join(path)).unwrap())
            .collect();
        let result = widen_with(&fixture.0, &[]).unwrap();
        let shapes: Vec<_> = result
            .entries
            .iter()
            .filter(|entry| entry.shape_only)
            .collect();
        assert_eq!(
            shapes
                .iter()
                .map(|entry| entry.path.as_str())
                .collect::<Vec<_>>(),
            [
                "horizon_items::item_stack::ItemStack",
                "horizon_items::kinds::Kind"
            ]
        );
        assert_eq!(shapes[0].kind, "struct");
        assert_eq!(shapes[0].signature, "struct ItemStack { kind : StackKind }");
        assert_eq!(
            shapes[0].refs,
            BTreeMap::from([("StackKind".into(), "horizon_items::kinds::Kind".into())])
        );
        assert_eq!(shapes[1].kind, "enum");
        assert_eq!(shapes[1].signature, "enum Kind { Empty , Count (u32) }");
        assert_eq!(shapes[1].cfg, "feature = \"extra\"");
        assert!(
            result
                .entries
                .iter()
                .all(|entry| !entry.path.starts_with("std::"))
        );
        let external = result
            .entries
            .iter()
            .find(|entry| entry.path == "horizon_client::game::Game::external")
            .unwrap();
        assert_eq!(external.refs["Duration"], "std::time::Duration");
        for (path, original) in paths.into_iter().zip(before) {
            assert_eq!(
                fs::read(fixture.0.join("crates/horizon-items/src").join(path)).unwrap(),
                original
            );
        }
        assert_eq!(result.files, 2); // Only game.rs and entry.rs were widened.
        let again = widen_with(&fixture.0, &[]).unwrap();
        assert_eq!(result.entries, again.entries);
        assert!(again.changed.is_empty());
    }

    #[test]
    fn listed_types_are_not_duplicated_and_shape_hashes_match_exposed_types() {
        let game = "struct Game { stack: horizon_items::ItemStack }\n";
        let fixture = shapes_fixture(game);
        let shapes = widen_with(&fixture.0, &[]).unwrap();
        let listed = widen_with(
            &shapes_fixture(game).0,
            &[(
                "internals.txt",
                "horizon_items::item_stack::ItemStack\nhorizon_items::kinds::Kind\n".into(),
            )],
        )
        .unwrap();
        for shape in shapes.entries.iter().filter(|entry| entry.shape_only) {
            let entries: Vec<_> = listed
                .entries
                .iter()
                .filter(|entry| entry.path == shape.path)
                .collect();
            assert_eq!(entries.len(), 1);
            assert!(!entries[0].shape_only);
            assert_eq!(entries[0].signature, shape.signature);
            assert_eq!(entries[0].hash, shape.hash);
            assert_eq!(entries[0].cfg, shape.cfg);
        }
    }

    #[test]
    fn resolution_uses_lexical_items_imports_reexports_and_the_crate_root() {
        let fixture = shapes_fixture(
            r#"use horizon_items::ItemStack as RootStack;
struct Game;
mod nested {
    use super::RootStack;
    use horizon_items::kinds::*;
    fn take(_: Option<RootStack>, _: Kind) -> horizon_items::ItemStack { loop {} }
}
mod shadow {
    struct RootStack;
    fn take(_: RootStack) {}
}
mod root_lookup { fn take(_: RootStack, _: RootOnly) {} }
"#,
        );
        fixture.put(
            "crates/horizon-client/src/lib.rs",
            "pub use horizon_items::ItemStack as RootOnly; mod game; mod entry;\n",
        );
        let entries = widen_with(&fixture.0, &[]).unwrap().entries;
        let find = |path: &str| {
            entries
                .iter()
                .find(|entry| entry.path == format!("horizon_client::game::{path}"))
                .unwrap()
        };
        assert_eq!(
            find("nested::take").refs["RootStack"],
            "horizon_items::item_stack::ItemStack"
        );
        assert_eq!(
            find("nested::take").refs["Kind"],
            "horizon_items::kinds::Kind"
        );
        assert!(find("shadow::take").refs.is_empty());
        // An enclosing module's imports are not inherited. Rust only falls back to the crate root.
        assert_eq!(
            find("root_lookup::take").refs,
            BTreeMap::from([(
                "RootOnly".into(),
                "horizon_items::item_stack::ItemStack".into()
            )])
        );
        let json = serde_json::to_value(find("shadow::RootStack")).unwrap();
        assert!(json.get("shape_only").is_none());
        assert!(json.get("refs").is_none());
    }

    #[test]
    fn cyclic_shapes_keep_distinct_imports_and_skip_generic_parameters() {
        let fixture =
            shapes_fixture("struct Game<T> { stack: ::horizon_items::ItemStack, marker: T }\n");
        fixture.put(
            "crates/horizon-client/src/lib.rs",
            "pub use horizon_items::Ignored as T; mod game; mod entry;\n",
        );
        fixture.put(
            "crates/horizon-items/src/items.rs",
            r#"pub mod item_stack;
pub use item_stack::ItemStack;
pub struct Ignored;
#[cfg(feature = "shapes")]
pub mod first {
    use crate::item_stack::ItemStack;
    pub enum Kind { Stack(Box<ItemStack>) }
}
pub mod second { pub enum Kind { Empty } }
"#,
        );
        fixture.put(
            "crates/horizon-items/src/item_stack.rs",
            r#"use crate::{first::Kind as First, second::{self, Kind as Second}};
pub struct ItemStack { a: First, b: Second, c: second::Kind }
"#,
        );
        let entries = widen_with(&fixture.0, &[]).unwrap().entries;
        assert!(!entries.iter().any(|entry| entry.path.ends_with("Ignored")));
        let shapes: Vec<_> = entries.iter().filter(|entry| entry.shape_only).collect();
        assert_eq!(shapes.len(), 3);
        let stack = shapes
            .iter()
            .find(|entry| entry.path.ends_with("ItemStack"))
            .unwrap();
        assert_eq!(
            stack.refs,
            BTreeMap::from([
                ("First".into(), "horizon_items::first::Kind".into()),
                ("Second".into(), "horizon_items::second::Kind".into()),
                ("second::Kind".into(), "horizon_items::second::Kind".into()),
            ])
        );
        let first = shapes
            .iter()
            .find(|entry| entry.path == "horizon_items::first::Kind")
            .unwrap();
        assert_eq!(first.cfg, "feature = \"shapes\"");
        let listed = widen_with(
            &fixture.0,
            &[("internals.txt", "horizon_items::first::Kind".into())],
        )
        .unwrap()
        .entries;
        let declared = listed
            .iter()
            .find(|entry| entry.path == first.path)
            .unwrap();
        assert!(!declared.shape_only);
        assert_eq!(declared.cfg, first.cfg);
        assert_eq!(declared.hash, first.hash);
    }

    #[test]
    fn widens_items_without_touching_uses_traits_macros_or_local_items() {
        let fixture = Fixture::new(
            r#"use std::fmt;
pub(crate) struct Game<'a> { world: u32, marker: &'a str }
struct Tuple(#[allow(dead_code)] u8, pub(crate) u16);
enum Kind { Named { value: u8 }, Tuple(u16) }
union Bits { byte: u8, word: u32 }
const LIMIT: u32 = 4;
static COUNT: u32 = 0;
type Alias = u32;
trait Trait { fn call(&self); }
impl<'a> Game<'a> {
    const SIZE: u32 = 4;
    #[inline] pub(super) async fn call(&self) { struct Local; }
    #[cfg(test)] fn test_method() {}
}
impl Trait for Game<'_> { fn call(&self) {} }
macro_rules! untouched { () => { struct MacroItem; } }
#[cfg(test)] mod tests { struct Private; }
#[cfg(test)] struct TestItem;
mod inline { struct Inner; }
// café: span columns after non-ASCII text are character-based.
const LABEL: &str = "é"; struct AfterUnicode;
"#,
        );
        let result = widen_with(&fixture.0, &[]).unwrap();
        let source = fixture.game();
        for expected in [
            "pub struct Game",
            "pub world: u32",
            "pub marker:",
            "pub struct Tuple(#[allow(dead_code)] pub u8, pub u16)",
            "pub enum Kind { Named { value: u8 }, Tuple(u16) }",
            "pub union Bits { pub byte: u8, pub word: u32 }",
            "pub const LIMIT",
            "pub static COUNT",
            "pub type Alias",
            "pub trait Trait",
            "pub const SIZE",
            "pub async fn call",
            "pub mod inline { pub struct Inner; }",
            "pub struct AfterUnicode;",
        ] {
            assert!(
                source.contains(expected),
                "missing {expected:?} in {source}"
            );
        }
        for unchanged in [
            "use std::fmt;",
            "struct Local;",
            "#[cfg(test)] fn test_method() {}",
            "impl Trait for Game<'_> { fn call(&self) {} }",
            "macro_rules! untouched { () => { struct MacroItem; } }",
            "#[cfg(test)] mod tests { struct Private; }",
            "#[cfg(test)] struct TestItem;",
        ] {
            assert!(source.contains(unchanged), "changed {unchanged:?}");
        }
        syn::parse_file(&source).unwrap();
        let field = result
            .entries
            .iter()
            .find(|entry| entry.path == "horizon_client::game::Game<'_>::world")
            .unwrap();
        assert_eq!(field.signature, "u32");
        assert_eq!(field.hash, "d7649d428b9ff33d");
        assert!(
            result
                .entries
                .iter()
                .any(|entry| entry.path == "horizon_client::game::Game<'_>::call")
        );
        assert!(
            !result
                .entries
                .iter()
                .any(|entry| entry.path.contains("TestItem") || entry.path.contains("Local"))
        );
    }

    #[test]
    fn selected_lower_method_and_its_modules_and_receiver_become_reachable() {
        let fixture = Fixture::new("struct Game;\n");
        fixture.lower();
        let result = widen_with(
            &fixture.0,
            &[(
                "internals.txt",
                "# list\nlower::hidden::inner::Thing::run\n".into(),
            )],
        )
        .unwrap();
        let read = |path| fs::read_to_string(fixture.0.join(path)).unwrap();
        assert_eq!(read("crates/lower/src/lib.rs"), "pub mod hidden;\n");
        assert_eq!(
            read("crates/lower/src/hidden.rs"),
            "#[path = \"other.rs\"] pub mod inner;\nfn sibling() {}\n"
        );
        assert_eq!(
            read("crates/lower/src/other.rs"),
            "pub struct Thing(u32); impl Thing { pub fn run(&self) {} fn unrelated(&self) {} }\n"
        );
        assert!(
            result
                .entries
                .iter()
                .any(|entry| entry.path == "lower::hidden::inner::Thing::run")
        );
        assert!(
            !result
                .entries
                .iter()
                .any(|entry| entry.path.contains("unrelated") || entry.path.contains("sibling"))
        );
    }

    #[test]
    fn unresolved_line_is_an_error_and_does_not_write_sources() {
        let fixture = Fixture::new("struct Game;\n");
        fixture.lower();
        // The error names the list and line it came from, past the lists before it.
        let error = widen_with(
            &fixture.0,
            &[
                ("internals.txt", "# one\n# two\n".into()),
                (
                    "points.txt",
                    "# comment\nlower::hidden::inner::Thing::missing\n".into(),
                ),
            ],
        )
        .unwrap_err();
        assert!(
            error.contains("points.txt:2: lower::hidden::inner::Thing::missing"),
            "{error}"
        );
        assert_eq!(fixture.game(), "struct Game;\n");
    }

    #[test]
    fn signatures_track_bounds_and_types_but_not_bodies_or_docs() {
        let one = Fixture::new(
            "/// one\nstruct Game<T> where T: Copy { value: T }\nfn work() -> u32 { 1 }\n",
        );
        let two = Fixture::new(
            "/// two\nstruct Game<T> where T: Clone { value: T }\nfn work() -> u32 { 2 }\n",
        );
        let first = widen_with(&one.0, &[]).unwrap();
        let second = widen_with(&two.0, &[]).unwrap();
        let lookup = |entries: Vec<Internal>| {
            entries
                .into_iter()
                .map(|entry| (entry.path.clone(), entry))
                .collect::<BTreeMap<_, _>>()
        };
        let (first, second) = (lookup(first.entries), lookup(second.entries));
        assert_ne!(
            first["horizon_client::game::Game"].hash,
            second["horizon_client::game::Game"].hash
        );
        assert_eq!(
            first["horizon_client::game::work"],
            second["horizon_client::game::work"]
        );
    }

    #[test]
    fn manifest_is_sorted_deterministic_and_independent_of_visibility() {
        let game = "pub struct Game;\nimpl Game { pub fn z(&self) {} fn a(&self) {} }\n";
        let one = Fixture::new(game);
        let two = Fixture::new(game);
        let first = widen_with(&one.0, &[]).unwrap();
        let second = widen_with(&two.0, &[]).unwrap();
        assert_eq!(
            serde_json::to_string(&first.entries).unwrap(),
            serde_json::to_string(&second.entries).unwrap()
        );
        assert!(
            first
                .entries
                .windows(2)
                .all(|pair| pair[0].path < pair[1].path)
        );
        let again = widen_with(&one.0, &[]).unwrap();
        assert_eq!(first.entries, again.entries);
        assert!(again.changed.is_empty());
    }

    fn by_key(entries: &[Internal]) -> BTreeMap<(String, &'static str, String), String> {
        entries
            .iter()
            .map(|e| ((e.path.clone(), e.kind, e.cfg.clone()), e.hash.clone()))
            .collect()
    }

    #[test]
    fn a_field_and_a_method_of_one_name_are_both_listed() {
        let fixture = Fixture::new(
            "struct Round { listening: bool }\nimpl Round { fn listening(&self) -> bool { self.listening } }\n",
        );
        let entries = by_key(&widen_with(&fixture.0, &[]).unwrap().entries);
        let path = "horizon_client::game::Round::listening".to_owned();
        assert!(entries.contains_key(&(path.clone(), "field", String::new())));
        assert!(entries.contains_key(&(path, "method", String::new())));
    }

    #[test]
    fn a_method_fingerprint_follows_its_impl_bounds_but_not_trait_docs() {
        let hashes = |game: &str| by_key(&widen_with(&Fixture::new(game).0, &[]).unwrap().entries);
        let key = |path: &str, kind| (format!("horizon_client::game::{path}"), kind, String::new());
        let loose = hashes(
            "struct Slots<K>(K);\nimpl<K: PartialEq> Slots<K> { fn new(k: K) -> Self { Self(k) } }\ntrait Ctl {\n    /// One.\n    fn go(&self);\n}\n",
        );
        let tight = hashes(
            "struct Slots<K>(K);\nimpl<K: PartialEq + Clone> Slots<K> { fn new(k: K) -> Self { Self(k) } }\ntrait Ctl {\n    /// Two.\n    fn go(&self);\n}\n",
        );
        let new = key("Slots<K>::new", "method");
        assert_ne!(loose[&new], tight[&new]);
        let ctl = key("Ctl", "trait");
        assert_eq!(loose[&ctl], tight[&ctl]);
    }

    #[test]
    fn guarded_alternatives_are_listed_under_their_conditions() {
        let fixture = Fixture::new(
            "mod pad {\n    #[cfg(feature = \"controllers\")]\n    mod hardware;\n    #[cfg(not(feature = \"controllers\"))]\n    mod hardware { type PadId = usize; }\n    mod nested { mod leaf; }\n}\n",
        );
        fixture.put(
            "crates/horizon-client/src/game/pad/hardware.rs",
            "type PadId = gilrs::GamepadId;\n",
        );
        fixture.put(
            "crates/horizon-client/src/game/pad/nested/leaf.rs",
            "struct Leaf;\n",
        );
        let entries = widen_with(&fixture.0, &[]).unwrap().entries;
        let pad: Vec<_> = entries
            .iter()
            .filter(|e| e.path == "horizon_client::game::pad::hardware::PadId")
            .map(|e| (e.cfg.as_str(), e.signature.as_str()))
            .collect();
        assert_eq!(
            pad,
            [
                (
                    "feature = \"controllers\"",
                    "type PadId = gilrs :: GamepadId ;"
                ),
                ("not (feature = \"controllers\")", "type PadId = usize ;"),
            ]
        );
        assert!(
            entries
                .iter()
                .any(|e| e.path == "horizon_client::game::pad::nested::leaf::Leaf")
        );
        assert!(
            fs::read_to_string(
                fixture
                    .0
                    .join("crates/horizon-client/src/game/pad/hardware.rs")
            )
            .unwrap()
            .starts_with("pub type PadId")
        );
    }
}
