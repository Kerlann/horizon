//! The WebAssembly interface of the mod API: every method of the traits a mod calls (the hosts,
//! the backends, the drawing contexts) and every method it implements (`Hooks`, the parts, the
//! capability handlers), as one WIT world and the glue on both sides of it.
mod glue;
mod index;
mod sides;
mod ty;
mod wit;

use index::{Found, Index};
use std::path::Path;
use syn::{FnArg, Item, Pat, ReturnType, TraitItem};
use ty::{Ty, Types};

/// Traits a mod calls: their methods are the world's imports.
pub(super) const IMPORTS: &[&str] = &[
    "Host",
    "ServerHost",
    "ClientHost",
    "Backend",
    "BackendMut",
    "ClientBackendMut",
    "DrawContext",
    "BlockEntityDrawContext",
    "LevelHost",
    "LevelRandomHost",
    "LevelTicksHost",
    "LevelWriteHost",
    "ConfigFiles",
    "Commands",
    "Levels",
    "Messages",
    "Players",
    "ClientSession",
    "RenderAssets",
    "menu::SlotView",
    "Registrar",
    "CapabilityProviders",
    "TicketValidation",
    "DataLoadContext",
    "LoopContext",
    "GenerationContext",
    "PieceContext",
    "StructureRandom",
    "AttachmentStore",
    "ServerReloadListeners",
    "ClientReloadListeners",
    "ServerReloadResources",
    "CriterionContext",
];

/// Imported traits a call never hands over, each reached through another's method instead.
pub(super) const REACHED: &[(&str, &str, &str)] = &[
    ("ConfigFiles", "host", "Ok(h.config_files())"),
    ("Commands", "server_host_mut", "Ok(h.commands())"),
    ("Levels", "server_host", "Ok(h.levels())"),
    ("Messages", "server_host", "Ok(h.messages())"),
    ("Players", "server_host", "Ok(h.players())"),
    ("ClientSession", "client_host", "Ok(h.client_session())"),
    ("RenderAssets", "client_host", "Ok(h.render_assets())"),
    // A level hook's host first: inside a level write it is the one the call hands over.
    (
        "AttachmentStore",
        "",
        "level_random_host_mut().map(|h| h.attachments()).or_else(|_| host_mut().map(|h| h.attachments()))",
    ),
    (
        "CapabilityProviders",
        "host",
        "Ok(h.capability_providers())",
    ),
    (
        "ServerReloadResources",
        "server_host",
        "Ok(h.reload_resources())",
    ),
    (
        "Backend",
        "host",
        "h.backend().map(crate::BackendRef::open).ok_or_else(|| missing(\"game backend\"))",
    ),
    (
        "BackendMut",
        "server_host_mut",
        "crate::backend_mut_of(h).ok_or_else(|| missing(\"game backend\"))",
    ),
    (
        "ClientBackendMut",
        "client_host_mut",
        "crate::client_backend_mut_of(h).ok_or_else(|| missing(\"client game backend\"))",
    ),
];

/// Traits a mod implements: their methods are the world's exports. The handler traits the
/// capability keys name join them (`exports`).
const EXPORTS: &[&str] = &[
    "Hooks",
    "ReloadApply",
    "StructureType",
    "StructurePiece",
    "BlockPart",
    "BlockEntityPart",
    "ItemPart",
    "MenuPart",
    "ServerReloadListener",
    "ClientReloadListener",
    "PlaceProvider",
    "CriterionTest",
];

/// Every exported trait: [`EXPORTS`], then the handler traits `mod_api`'s capability keys name
/// (`ResourceHandler<ItemResource>`), each instance of a generic one its own export. Set once,
/// by `surface`.
pub(super) fn exports() -> &'static [&'static str] {
    EXPORTED.get().map_or(EXPORTS, Vec::as_slice)
}

/// Whether the game also hands a mod objects of exported trait `owner` (a capability's handler,
/// another mod's provider): the mod calls one through the `held-<interface>` imports, by the
/// index the game keeps it under.
pub(super) fn held(owner: &str) -> bool {
    owner == "PlaceProvider" || (exports().contains(&owner) && !EXPORTS.contains(&owner))
}

/// The contexts of the load phase: the catalog's registration and data-load contexts and a
/// resource reload's. A call given one reads and parses data, so it runs within the load limit
/// (`Budgets::load`), not a hook call's.
const LOAD_CONTEXTS: &[&str] = &[
    "RegistrationContext",
    "DataLoadContext",
    "ServerReloadContext",
    "ClientReloadContext",
];

/// The traits the game calls on its worldgen worker threads: a mod's structure types and
/// their pieces, which keep no mutable state and get no `Host`.
const WORLDGEN: &[&str] = &["StructureType", "StructurePiece"];

/// What a call of `method` is to the runtime (`wasm_host::Call`), for its limits and its wait.
pub(super) fn call_class(types: &ty::Types<'_>, method: &Method) -> &'static str {
    if WORLDGEN.contains(&method.owner) {
        "super::Call::Worldgen"
    } else if loads(types, method) {
        "super::Call::Load"
    } else {
        "super::Call::Hook"
    }
}

/// Whether `method` is a load-phase call: one of its arguments is a load-phase context.
fn loads(types: &ty::Types<'_>, method: &Method) -> bool {
    method.params.iter().any(|(_, ty)| {
        let ty = match ty {
            ty::Ty::Ref(inner, _) => &**inner,
            ty => ty,
        };
        match ty {
            ty::Ty::Dyn(name, _) => LOAD_CONTEXTS.contains(&name.as_str()),
            ty::Ty::Named(def) => LOAD_CONTEXTS
                .iter()
                .any(|name| types.defs[*def].rust.rsplit("::").next() == Some(*name)),
            _ => false,
        }
    })
}

/// A trait object that crosses without methods: a provider of a mod's own handler type,
/// erased (`Arc<dyn Any + Send + Sync>`), which only mods of its group take back.
pub(super) const ERASED: &str = "Any";

static EXPORTED: std::sync::OnceLock<Vec<&'static str>> = std::sync::OnceLock::new();

/// A capability key's shape: what its providers are registered by, its handler trait as
/// written (`ResourceHandler<ItemResource>`), and an entity key's lookup context.
#[derive(Clone, PartialEq)]
pub(super) struct KeyShape {
    pub(super) by: &'static str,
    pub(super) handler: String,
    pub(super) context: String,
}

/// The shapes of `mod_api`'s capability keys (`CapabilityKey<dyn T>` and the item and entity
/// forms), each once, in the order their keys are exported.
fn key_shapes(index: &mut Index<'_>) -> Result<Vec<KeyShape>, String> {
    let api = index.module("mod_api", &[])?;
    let names: Vec<String> = index.modules[api]
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Use(item) if matches!(item.vis, syn::Visibility::Public(_)) => Some(&item.tree),
            _ => None,
        })
        .flat_map(use_names)
        .collect();
    let mut shapes = Vec::new();
    for name in names {
        let Ok(Found::Item(id)) = index.resolve(api, &name) else {
            continue;
        };
        let Item::Const(item) = index.item(id) else {
            continue;
        };
        let syn::Type::Path(path) = &*item.ty else {
            continue;
        };
        let Some(last) = path.path.segments.last() else {
            continue;
        };
        let by = match last.ident.to_string().as_str() {
            "CapabilityKey" => "Block",
            "ItemCapabilityKey" => "Item",
            "EntityCapabilityKey" => "Entity",
            _ => continue,
        };
        let syn::PathArguments::AngleBracketed(args) = &last.arguments else {
            continue;
        };
        let mut args = args.args.iter();
        let Some(syn::GenericArgument::Type(syn::Type::TraitObject(object))) = args.next() else {
            continue;
        };
        let context = match args.next() {
            Some(syn::GenericArgument::Type(ty)) => ty::written(ty),
            _ => "()".to_string(),
        };
        let shape = KeyShape {
            by,
            handler: ty::trait_name(object)?,
            context,
        };
        if !shapes.contains(&shape) {
            shapes.push(shape);
        }
    }
    Ok(shapes)
}

impl KeyShape {
    /// Its provider trait, as an owner (`BlockCapabilityProvider<dyn ResourceHandler<ItemResource>>`).
    pub(super) fn provider(&self) -> String {
        match self.by {
            "Entity" => format!(
                "EntityCapabilityProvider<dyn {}, {}>",
                self.handler, self.context
            ),
            by => format!("{by}CapabilityProvider<dyn {}>", self.handler),
        }
    }
    /// Its case in the provider enum (`ResourceHandlerItemResource`).
    pub(super) fn case(&self) -> String {
        let context = if self.by == "Entity" && self.context != "()" {
            self.context.as_str()
        } else {
            ""
        };
        heck::ToUpperCamelCase::to_upper_camel_case(format!("{} {context}", self.handler).as_str())
    }
}

/// The provider enums (`BlockProviderOf` and the item and entity forms): one case per key shape,
/// and `Own` for a mod's own handler type. `mod_api` includes them.
fn providers_code(shapes: &[KeyShape]) -> String {
    let mut code = String::from(
        "// @generated by horizon-mod-api-gen from mod_api's capability keys; do not edit.\n",
    );
    for (by, ctx) in [("Block", ""), ("Item", ""), ("Entity", ", C")] {
        let of = |shape: &KeyShape| format!("Arc<dyn {}>", shape.provider());
        let mine: Vec<&KeyShape> = shapes.iter().filter(|s| s.by == by).collect();
        let cases: String = mine
            .iter()
            .map(|s| format!("{}({}),\n", s.case(), of(s)))
            .collect();
        let tries: String = mine
            .iter()
            .map(|s| {
                format!(
                    "let any = match any.downcast::<{}>() {{ Ok(p) => return Self::{}(*p), Err(any) => any }};\n",
                    of(s),
                    s.case()
                )
            })
            .collect();
        let names: String = mine
            .iter()
            .map(|s| {
                format!(
                    "Self::{}(_) => std::any::type_name::<dyn {}>(),\n",
                    s.case(),
                    s.handler
                )
            })
            .collect();
        let gets: String = mine
            .iter()
            .map(|s| format!("Self::{}(p) => (p as &dyn Any).downcast_ref(),\n", s.case()))
            .collect();
        let (params, provider) = match by {
            "Entity" => (
                "<T: ?Sized + 'static, C: 'static>",
                "EntityCapabilityProvider<T, C>",
            ),
            _ => ("<T: ?Sized + 'static>", ""),
        };
        let provider = if provider.is_empty() {
            format!("{by}CapabilityProvider<T>")
        } else {
            provider.to_string()
        };
        let _ = ctx;
        code.push_str(&format!(
            "/// A {by} capability's provider, by its handler type.\n#[doc(hidden)]\n#[derive(Clone)]\npub enum {by}ProviderOf {{\n{cases}/// A handler type of a mod's own.\nOwn(OwnProvider),\n}}\nimpl {by}ProviderOf {{\n/// `provider`, by its handler type.\npub(crate) fn of{params}(provider: Arc<dyn {provider}>) -> Self {{\nlet any: Box<dyn Any + Send + Sync> = Box::new(provider);\n{tries}Self::Own(OwnProvider {{ provider: Arc::from(any), handler: std::any::type_name::<T>() }})\n}}\n/// Its handler type's name.\npub(crate) fn handler(&self) -> &'static str {{\nmatch self {{\n{names}Self::Own(own) => own.handler,\n}}\n}}\n/// The provider, if it answers with handler type `T`.\npub(crate) fn get{params}(&self) -> Option<&Arc<dyn {provider}>> {{\nmatch self {{\n{gets}Self::Own(own) => (*own.provider).downcast_ref(),\n}}\n}}\n}}\n"
        ));
    }
    code
}

/// Every name a `use` tree brings in.
fn use_names(tree: &syn::UseTree) -> Vec<String> {
    match tree {
        syn::UseTree::Path(path) => use_names(&path.tree),
        syn::UseTree::Name(name) => vec![name.ident.to_string()],
        syn::UseTree::Rename(rename) => vec![rename.rename.to_string()],
        syn::UseTree::Group(group) => group.items.iter().flat_map(use_names).collect(),
        syn::UseTree::Glob(_) => Vec::new(),
    }
}

/// An owner as written (`ResourceHandler<ItemResource>`): the trait's path, and its type arguments.
fn owner_parts(owner: &str) -> Result<(Vec<String>, Vec<syn::Type>), String> {
    let path: syn::Path = syn::parse_str(owner).map_err(|e| format!("{owner}: {e}"))?;
    let segments: Vec<String> = path.segments.iter().map(|s| s.ident.to_string()).collect();
    let args = match &path.segments.last().unwrap().arguments {
        syn::PathArguments::AngleBracketed(args) => args
            .args
            .iter()
            .filter_map(|arg| match arg {
                syn::GenericArgument::Type(ty) => Some(ty.clone()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    };
    Ok((segments, args))
}

/// The exported traits the mod itself answers, with no handler index, and the guest's
/// expression for its implementation of each; every other export is a registered handler's.
pub(super) const MOD_EXPORTS: &[(&str, &str)] = &[("Hooks", "super::hooks()")];

/// The guest's receiver of `owner`'s exports, if the mod itself answers them.
pub(super) fn mod_receiver(owner: &str) -> Option<&'static str> {
    MOD_EXPORTS
        .iter()
        .find(|(o, _)| *o == owner)
        .map(|(_, r)| *r)
}

/// The exported traits that list the hooks they subscribe to, and the method listing them.
pub(super) const SUBSCRIBED: &[(&str, &str)] = &[
    ("Hooks", "subscribed"),
    ("BlockPart", "block_subscribed"),
    ("BlockEntityPart", "block_entity_subscribed"),
    ("ItemPart", "item_subscribed"),
    ("MenuPart", "menu_subscribed"),
];

/// One trait method as it crosses.
#[derive(Debug)]
pub(super) struct Method {
    pub(super) owner: &'static str,
    pub(super) name: String,
    /// `Some(mutable)` for a `self` receiver.
    pub(super) receiver: Option<bool>,
    pub(super) params: Vec<(String, Ty)>,
    pub(super) ret: Ty,
}

/// A method that can't cross, and why.
#[derive(Debug)]
pub(super) struct Unbound {
    pub(super) owner: &'static str,
    pub(super) name: String,
    pub(super) why: String,
}

pub(super) struct Surface<'a> {
    pub(super) types: Types<'a>,
    pub(super) methods: Vec<Method>,
    pub(super) unbound: Vec<Unbound>,
    /// The methods whose doc says `Not on wasm: <why>.`
    pub(super) excused: Vec<(&'static str, String)>,
}

/// The line on a method's doc that keeps it off wasm, and says why.
const NOT_ON_WASM: &str = "Not on wasm: ";

/// Whether `attrs`' doc has a [`NOT_ON_WASM`] line with a reason.
fn not_on_wasm(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| match &attr.meta {
        syn::Meta::NameValue(doc) if doc.path.is_ident("doc") => match &doc.value {
            syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(line),
                ..
            }) => line
                .value()
                .trim()
                .strip_prefix(NOT_ON_WASM)
                .is_some_and(|why| why.len() > 1),
            _ => false,
        },
        _ => false,
    })
}

/// The methods of the bound traits, with their types resolved. `hooks` and `host` are the
/// generated sources `mod_api` includes.
pub(super) fn surface<'a>(
    root: &'a Path,
    included: &'a [(&'a str, &'a str)],
) -> Result<Surface<'a>, String> {
    let mut types = Types::new(Index::new(root, included))?;
    let api = types.index.module("mod_api", &[])?;
    let mut exported: Vec<&'static str> = EXPORTS.to_vec();
    for shape in key_shapes(&mut types.index)? {
        for owner in [shape.handler.clone(), shape.provider()] {
            if !exported.iter().any(|e| *e == owner) {
                exported.push(Box::leak(owner.into_boxed_str()));
            }
        }
    }
    let _ = EXPORTED.set(exported);
    let mut methods = Vec::new();
    let mut unbound = Vec::new();
    let mut excused = Vec::new();
    for owner in IMPORTS.iter().chain(exports()) {
        let (path, args) = owner_parts(owner)?;
        let Found::Item(id) = types.index.resolve_path(api, &path)? else {
            return Err(format!("{owner} is not a mod_api trait"));
        };
        let Item::Trait(item) = types.index.item(id).clone() else {
            return Err(format!("{owner} is not a trait"));
        };
        // A generic trait's instance: its parameters as the owner's arguments.
        let mut subst = Vec::new();
        for (param, arg) in item.generics.type_params().zip(&args) {
            subst.push((param.ident.to_string(), types.ty(api, arg, &[])?));
        }
        for item in &item.items {
            let TraitItem::Fn(function) = item else {
                continue;
            };
            let name = function.sig.ident.to_string();
            if hidden(&function.attrs) || name.ends_with("subscribed") {
                continue;
            }
            if not_on_wasm(&function.attrs) {
                excused.push((*owner, name.clone()));
            }
            match method(&mut types, id.module, owner, function, &subst) {
                Ok(method) => methods.push(method),
                Err(why) => unbound.push(Unbound { owner, name, why }),
            }
        }
    }
    wit::refuse(&mut types);
    // A method whose types can't all cross is unbound, with the first reason.
    let (bound, refused): (Vec<_>, Vec<_>) = methods
        .into_iter()
        .map(|method| match wit::signature(&types, &method) {
            // A borrowed view handed back has nothing on the mod's side to point into.
            Ok(_)
                if IMPORTS.contains(&method.owner)
                    && glue::borrowed_record(&types, &method.ret) =>
            {
                Err(Unbound {
                    owner: method.owner,
                    name: method.name,
                    why: "returns a borrowed view, which crosses only as a resource".into(),
                })
            }
            Ok(_) => Ok(method),
            Err(why) => Err(Unbound {
                owner: method.owner,
                name: method.name,
                why,
            }),
        })
        .partition(Result::is_ok);
    let methods = bound.into_iter().map(Result::unwrap).collect();
    unbound.extend(refused.into_iter().map(|r| r.err().unwrap()));
    Ok(Surface {
        types,
        methods,
        unbound,
        excused,
    })
}

/// The wasm side of the mod API: the WIT package, and the Rust each side compiles.
pub struct Wasm {
    pub wit: String,
    /// The capability provider enums `mod_api` includes.
    pub providers: String,
    pub guest: String,
    pub host: String,
    /// `Owner::method: why` for each method that doesn't cross.
    pub unbound: Vec<String>,
    /// The source files read.
    pub sources: Vec<std::path::PathBuf>,
}

/// Generates the wasm side from the mod API's sources and its generated files (`included`).
pub fn generate(root: &Path, included: &[(&str, &str)]) -> Result<Wasm, String> {
    // The provider enums first: `mod_api` includes them, so its surface reads them too.
    let providers = providers_code(&key_shapes(&mut Index::new(root, included))?);
    let mut with_providers = included.to_vec();
    with_providers.push(("capability_providers_generated.rs", providers.as_str()));
    let mut surface = surface(root, &with_providers)?;
    let hooks = hook_ids(&mut surface.types)?;
    let servers = servers(&mut surface.types)?;
    surface.types.views = views(&mut surface.types)?;
    coverage(&surface)?;
    let methods: Vec<&Method> = surface.methods.iter().collect();
    let imports: Vec<&Method> = methods
        .iter()
        .copied()
        .filter(|m| !frame_accessor(m))
        .collect();
    let wit = wit::package(&surface.types, &imports, &api_version(root)?)?;
    Ok(Wasm {
        providers: providers.clone(),
        guest: sides::guest(&surface.types, &methods, &wit, &hooks)?,
        host: sides::host(&surface.types, &imports, &wit, &hooks, &servers)?,
        wit,
        sources: surface.types.index.read.take(),
        unbound: surface
            .unbound
            .iter()
            .map(|u| format!("{}::{}: {}", u.owner, u.name, u.why))
            .collect(),
    })
}

/// A frame resource already reborrowed through `REACHED`: acquiring it on the
/// guest needs no wire call. Its trait methods are the actual imports.
pub(super) fn frame_accessor(method: &Method) -> bool {
    method.params.is_empty()
        && IMPORTS.contains(&method.owner)
        && matches!(&method.ret, Ty::Dyn(name, _) if REACHED.iter().any(|(owner, ..)| name == owner))
}

/// The coverage check: every method of the bound traits crosses to wasm, or its doc says why
/// not (`/// Not on wasm: <why>.`), and no method that crosses says it doesn't. The game's and
/// the mods' builds both run it, so the wasm side can't fall behind the API.
fn coverage(surface: &Surface<'_>) -> Result<(), String> {
    let excused = |owner: &str, name: &str| {
        surface
            .excused
            .iter()
            .any(|(o, n)| *o == owner && n == name)
    };
    let mut problems: Vec<String> = surface
        .unbound
        .iter()
        .filter(|u| !excused(u.owner, &u.name))
        .map(|u| {
            format!(
                "{}::{} doesn't cross to wasm ({}): fix its types, or add `/// {NOT_ON_WASM}<why>.` to its doc",
                u.owner, u.name, u.why
            )
        })
        .collect();
    problems.extend(
        surface
            .methods
            .iter()
            .filter(|m| excused(m.owner, &m.name) && !frame_accessor(m))
            .map(|m| {
                format!(
                    "{}::{} crosses to wasm: remove `{NOT_ON_WASM}` from its doc",
                    m.owner, m.name
                )
            }),
    );
    match problems.is_empty() {
        true => Ok(()),
        false => Err(problems.join("\n")),
    }
}

/// `horizon-mod-api`'s Cargo version, the one version of the mod API: the WIT package carries it,
/// so a `mod.wasm` imports the interfaces of the API version it was built against.
fn api_version(root: &Path) -> Result<String, String> {
    let path = root.join("crates/horizon-mod-api/Cargo.toml");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let manifest: toml::Table = text
        .parse()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    manifest
        .get("package")
        .and_then(|p| p.get("version"))
        .and_then(toml::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("{}: package.version missing", path.display()))
}

/// Each imported trait with the imported traits whose objects also serve it: itself first, then
/// its subtraits (a server host serves a call that asks for a host).
fn servers(types: &mut Types<'_>) -> Result<Vec<(&'static str, Vec<&'static str>)>, String> {
    let api = types.index.module("mod_api", &[])?;
    let mut supers: Vec<(&str, Vec<String>)> = Vec::new();
    for owner in IMPORTS {
        let path: Vec<String> = owner.split("::").map(str::to_string).collect();
        let Found::Item(id) = types.index.resolve_path(api, &path)? else {
            return Err(format!("{owner} is not a mod_api trait"));
        };
        let Item::Trait(item) = types.index.item(id) else {
            return Err(format!("{owner} is not a trait"));
        };
        let names = item
            .supertraits
            .iter()
            .filter_map(|bound| match bound {
                syn::TypeParamBound::Trait(t) => Some(t.path.segments.last()?.ident.to_string()),
                _ => None,
            })
            .collect();
        supers.push((owner, names));
    }
    let short = |owner: &str| owner.rsplit("::").next().unwrap().to_string();
    let serves = |sub: &str, of: &str| {
        let mut stack = vec![sub.to_string()];
        while let Some(t) = stack.pop() {
            if t == short(of) {
                return true;
            }
            if let Some((_, s)) = supers.iter().find(|(o, _)| short(o) == t) {
                stack.extend(s.iter().cloned());
            }
        }
        false
    };
    Ok(IMPORTS
        .iter()
        .map(|of| {
            let mut list = vec![*of];
            list.extend(
                IMPORTS
                    .iter()
                    .filter(|sub| *sub != of && serves(&short(sub), of)),
            );
            (*of, list)
        })
        .collect())
}

/// Each exported trait with the exported traits a handler of it also answers as: itself, then its
/// exported supertraits (a block entity part is a block part too).
fn views(types: &mut Types<'_>) -> Result<Vec<(String, Vec<String>)>, String> {
    let api = types.index.module("mod_api", &[])?;
    let mut out = Vec::new();
    for owner in exports() {
        let (path, _) = owner_parts(owner)?;
        let Found::Item(id) = types.index.resolve_path(api, &path)? else {
            return Err(format!("{owner} is not a mod_api trait"));
        };
        let Item::Trait(item) = types.index.item(id) else {
            return Err(format!("{owner} is not a trait"));
        };
        let mut list = vec![owner.to_string()];
        list.extend(item.supertraits.iter().filter_map(|bound| match bound {
            syn::TypeParamBound::Trait(t) => {
                let name = t.path.segments.last()?.ident.to_string();
                exports().contains(&name.as_str()).then_some(name)
            }
            _ => None,
        }));
        out.push((owner.to_string(), list));
    }
    out.push((ERASED.to_string(), vec![ERASED.to_string()]));
    Ok(out)
}

/// The variants of the generated `HookId`, each with its hook's name.
fn hook_ids(types: &mut Types<'_>) -> Result<Vec<(String, String)>, String> {
    let api = types.index.module("mod_api", &[])?;
    let Found::Item(id) = types.index.resolve_path(api, &["HookId".to_string()])? else {
        return Err("HookId is not a mod_api enum".into());
    };
    let Item::Enum(item) = types.index.item(id) else {
        return Err("HookId is not an enum".into());
    };
    Ok(item
        .variants
        .iter()
        .map(|v| {
            (
                v.ident.to_string(),
                heck::ToSnakeCase::to_snake_case(v.ident.to_string().as_str()),
            )
        })
        .collect())
}

fn method(
    types: &mut Types<'_>,
    module: usize,
    owner: &'static str,
    function: &syn::TraitItemFn,
    subst: &[(String, Ty)],
) -> Result<Method, String> {
    let sig = &function.sig;
    if sig.generics.type_params().next().is_some() {
        return Err("generic".into());
    }
    let mut receiver = None;
    let mut params = Vec::new();
    for input in &sig.inputs {
        match input {
            FnArg::Receiver(r) => receiver = Some(r.mutability.is_some()),
            FnArg::Typed(arg) => {
                let name = match &*arg.pat {
                    Pat::Ident(ident) => ident.ident.to_string(),
                    _ => format!("arg{}", params.len()),
                };
                let name = name.trim_start_matches('_').to_string();
                params.push((name, types.ty(module, &arg.ty, subst)?));
            }
        }
    }
    let ret = match &sig.output {
        ReturnType::Default => Ty::Unit,
        ReturnType::Type(_, ty) => types.ty(module, ty, subst)?,
    };
    Ok(Method {
        owner,
        name: sig.ident.to_string(),
        receiver,
        params,
        ret,
    })
}

fn hidden(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        matches!(&attr.meta, syn::Meta::List(list)
            if list.path.is_ident("doc") && list.tokens.to_string() == "hidden")
    })
}

#[cfg(test)]
mod tests {
    /// Builds the generated wasm interface and hands its `client-host` interface and the unbound
    /// method names to `check`.
    fn with_client_host(check: impl FnOnce(&wit_parser::Interface, &[String]) + Send + 'static) {
        std::thread::Builder::new()
            .stack_size(64 << 20)
            .spawn(move || {
                let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
                let generated =
                    super::super::sources(&root, super::super::FillMode::Require).unwrap();
                let included = [
                    ("mod_api_generated.rs", generated.hooks.as_str()),
                    ("mod_api_generated_host.rs", generated.host.as_str()),
                    ("native_render_generated.rs", generated.render.as_str()),
                ];
                let wasm = super::generate(&root, &included).unwrap();
                let mut resolve = wit_parser::Resolve::default();
                let package = resolve.push_str("mod-api.wit", &wasm.wit).unwrap();
                let client =
                    &resolve.interfaces[resolve.packages[package].interfaces["client-host"]];
                check(client, &wasm.unbound);
            })
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn client_gui_state_has_generated_wasm_bindings() {
        with_client_host(|client, unbound| {
            for method in ["window-focused", "observed-screen-id"] {
                assert!(client.functions.contains_key(method));
            }
            for name in [
                "ClientHost::window_focused",
                "ClientHost::observed_screen_id",
            ] {
                assert!(!unbound.iter().any(|unbound| unbound == name));
            }
        });
    }

    #[test]
    fn client_input_options_have_a_generated_wasm_binding() {
        with_client_host(|client, unbound| {
            assert!(client.functions.contains_key("input-options"));
            assert!(
                !unbound
                    .iter()
                    .any(|name| name == "ClientHost::input_options")
            );
        });
    }

    #[test]
    fn recipe_rules_are_generated_in_wasm_contract() {
        std::thread::Builder::new()
            .stack_size(64 << 20)
            .spawn(|| {
                let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
                let generated =
                    super::super::sources(&root, super::super::FillMode::Require).unwrap();
                let included = [
                    ("mod_api_generated.rs", generated.hooks.as_str()),
                    ("mod_api_generated_host.rs", generated.host.as_str()),
                    ("native_render_generated.rs", generated.render.as_str()),
                ];
                let wasm = super::generate(&root, &included).unwrap();
                let mut resolve = wit_parser::Resolve::default();
                let package = resolve.push_str("mod-api.wit", &wasm.wit).unwrap();
                let types = &resolve.interfaces[resolve.packages[package].interfaces["types"]];
                let rule = types.types["recipe-rule"];
                let wit_parser::TypeDefKind::Variant(variants) = &resolve.types[rule].kind else {
                    panic!("recipe-rule is not a variant")
                };
                for name in [
                    "stonecutting",
                    "smithing-transform",
                    "smithing-trim",
                    "transmute",
                    "firework-star",
                    "repair-item",
                ] {
                    assert!(
                        variants.cases.iter().any(|case| case.name == name),
                        "missing {name}"
                    );
                }
                assert!(wasm.wit.contains("rule: option<recipe-rule>"));
                assert!(
                    wasm.wit
                        .contains("recipes-of-type: func(kind: string) -> list<recipe>")
                );
                assert!(
                    !wasm
                        .unbound
                        .iter()
                        .any(|name| name.contains("recipes_of_type"))
                );
            })
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn services_use_native_forwards_and_frame_resources() {
        use quote::ToTokens;

        std::thread::Builder::new()
            .stack_size(64 << 20)
            .spawn(|| {
                let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
                let generated =
                    super::super::sources(&root, super::super::FillMode::Require).unwrap();
                let included = [
                    ("mod_api_generated.rs", generated.hooks.as_str()),
                    ("mod_api_generated_host.rs", generated.host.as_str()),
                    ("native_render_generated.rs", generated.render.as_str()),
                ];
                let wasm = super::generate(&root, &included).unwrap();
                let mut resolve = wit_parser::Resolve::default();
                let package = resolve.push_str("mod-api.wit", &wasm.wit).unwrap();
                let package = &resolve.packages[package];
                for (service, methods) in [
                    ("commands", &["run-command", "feedback"][..]),
                    ("messages", &["broadcast", "text-allowed"][..]),
                    ("players", &["acting-player", "server-players"][..]),
                ] {
                    let interface = &resolve.interfaces[package.interfaces[service]];
                    for method in methods {
                        assert!(interface.functions.contains_key(*method));
                    }
                    let host = &resolve.interfaces[package.interfaces["server-host"]];
                    assert!(
                        !host.functions.contains_key(service),
                        "no unit accessor import"
                    );
                    for method in methods {
                        assert!(!host.functions.contains_key(*method));
                    }
                }
                let client = &resolve.interfaces[package.interfaces["client-host"]];
                let session = &resolve.interfaces[package.interfaces["client-session"]];
                assert!(!client.functions.contains_key("client-session"));
                assert!(!client.functions.contains_key("render-assets"));
                let assets = &resolve.interfaces[package.interfaces["render-assets"]];
                assert!(assets.functions.contains_key("model"));
                let draw = &resolve.interfaces[package.interfaces["block-entity-draw-context"]];
                assert!(draw.functions.contains_key("model"));
                for (owner, name) in [("ClientHost", "render_assets"), ("RenderAssets", "model")] {
                    let declarations = syn::parse_file(&generated.host).unwrap();
                    let method = declarations.items.iter().find_map(|item| match item {
                        syn::Item::Trait(item) if item.ident == owner => item.items.iter().find_map(|item| match item {
                            syn::TraitItem::Fn(method) if method.sig.ident == name => Some(method),
                            _ => None,
                        }),
                        _ => None,
                    }).unwrap();
                    assert!(method.to_token_stream().to_string().contains("LoadAccess :: PublishedResources"));
                }
                // A resource read may not accidentally open a mixed service during reload.
                let mut mixed = crate::catalog::Catalog::load(&root).unwrap();
                mixed.helpers.iter_mut().find(|helper| helper.name == "client_server").unwrap().service = Some("render_assets".into());
                let (host, implementation) = crate::host::generate(&root, &mixed, &generated.hooks).unwrap();
                for source in [host, implementation] {
                    assert!(source.contains("check_access(\"render_assets\", mod_api::__private::LoadAccess::WorldOnly)"));
                }
                assert!(wasm.host.contains("Ok(h.render_assets())"));
                assert!(wasm.host.contains("ModelRef::from_parts"));
                assert!(!wasm.unbound.iter().any(|name| name.starts_with("RenderAssets::")));
                for method in ["client-server", "client-language", "client-frame-stats"] {
                    assert!(session.functions.contains_key(method), "missing {method}");
                    assert!(!client.functions.contains_key(method));
                }
                let reads = &resolve.interfaces[package.interfaces["backend"]];
                for method in ["player-block-breaking", "player-ambient-mood"] {
                    assert!(reads.functions.contains_key(method), "missing {method}");
                }
                let backend = &resolve.interfaces[package.interfaces["backend-mut"]];
                for method in [
                    "player-tell",
                    "player-action-bar",
                    "player-tab-list",
                    "player-hide",
                    "player-set-nickname",
                    "player-set-sleep-ignored",
                ] {
                    assert!(
                        backend.functions.contains_key(method),
                        "missing {method}: {:?}",
                        backend.functions.keys()
                    );
                }
                let reads = &resolve.interfaces[package.interfaces["backend"]];
                for name in ["player-address", "player-permission-level"] {
                    assert!(reads.functions.contains_key(name), "missing {name}");
                    assert!(!wasm.unbound.iter().any(|method| {
                        method == &format!("Backend::{}", name.replace('-', "_"))
                    }));
                }
                for interface in package.interfaces.values() {
                    assert!(!resolve.interfaces[*interface].functions.contains_key("player-op-level"));
                }
                let declarations = syn::parse_file(&generated.host).unwrap();
                let player = declarations.items.iter().find_map(|item| match item {
                    syn::Item::Impl(item)
                        if matches!(&*item.self_ty, syn::Type::Path(ty) if ty.path.is_ident("PlayerRef")) => Some(item),
                    _ => None,
                }).unwrap();
                let methods: Vec<_> = player.items.iter().filter_map(|item| match item {
                    syn::ImplItem::Fn(method) => Some(method.sig.ident.to_string()),
                    _ => None,
                }).collect();
                assert!(methods.iter().any(|method| method == "address"));
                assert!(methods.iter().any(|method| method == "permission_level"));
                assert!(!methods.iter().any(|method| method == "op_level"));
                let implementation = syn::parse_file(&generated.implementation).unwrap();
                for (service, methods) in [
                    ("Commands", &["run_command", "feedback"][..]),
                    ("Messages", &["broadcast", "text_allowed"][..]),
                    ("Players", &["acting_player", "server_players"][..]),
                    ("ClientSession", &["client_server", "client_language", "client_frame_stats"][..]),
                    ("RenderAssets", &["model"][..]),
                    ("Backend", &["player_address", "player_permission_level", "player_block_breaking", "player_ambient_mood"][..]),
                ] {
                    let item = implementation
                        .items
                        .iter()
                        .find_map(|i| match i {
                            syn::Item::Impl(i)
                                if i.trait_
                                    .as_ref()
                                    .is_some_and(|(_, p, _)| p.is_ident(service)) =>
                            {
                                Some(i)
                            }
                            _ => None,
                        })
                        .unwrap();
                    for name in methods {
                        let method = item
                            .items
                            .iter()
                            .find_map(|i| match i {
                                syn::ImplItem::Fn(f) if f.sig.ident == name => Some(f),
                                _ => None,
                            })
                            .unwrap();
                        let access = if service == "RenderAssets" {
                            quote::quote!(mod_api::__private::LoadAccess::PublishedResources)
                        } else if service == "ClientSession" {
                            quote::quote!(mod_api::__private::LoadAccess::ClientSetup)
                        } else {
                            quote::quote!(mod_api::__private::LoadAccess::WorldOnly)
                        };
                        let native = quote::format_ident!("native_{name}");
                        let args: Vec<_> = method.sig.inputs.iter().filter_map(|arg| match arg {
                            syn::FnArg::Receiver(_) => None,
                            syn::FnArg::Typed(arg) => match &*arg.pat {
                                syn::Pat::Ident(pat) => Some(&pat.ident),
                                _ => panic!("named forwarding argument"),
                            },
                        }).collect();
                        let expected: syn::Block = syn::parse_quote!({
                            mod_api::__private::check_access(#name, #access);
                            self.#native(#(#args),*)
                        });
                        assert_eq!(
                            method.block.to_token_stream().to_string(),
                            expected.to_token_stream().to_string(),
                            "{service}::{name} guards before forwarding its arguments"
                        );
                    }
                }
                assert!(wasm.host.contains("Ok(h.commands())"));
                assert!(wasm.host.contains("Ok(h.messages())"));
                assert!(wasm.host.contains("Ok(h.players())"));
                assert!(wasm.host.contains("Ok(h.client_session())"));
                assert!(
                    !wasm
                        .unbound
                        .iter()
                        .any(|name| name.starts_with("Commands::")
                            || name.starts_with("Messages::")
                            || name.starts_with("Players::"))
                );
            })
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn client_modifiers_have_a_generated_wasm_binding() {
        std::thread::Builder::new()
            .stack_size(64 << 20)
            .spawn(|| {
                let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
                let generated =
                    super::super::sources(&root, super::super::FillMode::Require).unwrap();
                let included = [
                    ("mod_api_generated.rs", generated.hooks.as_str()),
                    ("mod_api_generated_host.rs", generated.host.as_str()),
                    ("native_render_generated.rs", generated.render.as_str()),
                ];
                let wasm = super::generate(&root, &included).unwrap();
                let mut resolve = wit_parser::Resolve::default();
                let package = resolve.push_str("mod-api.wit", &wasm.wit).unwrap();
                let client =
                    &resolve.interfaces[resolve.packages[package].interfaces["client-host"]];
                assert!(client.functions.contains_key("modifiers"));
                assert!(
                    !wasm
                        .unbound
                        .iter()
                        .any(|name| name == "ClientHost::modifiers")
                );
            })
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn client_handle_writes_have_their_own_wasm_backend() {
        std::thread::Builder::new()
            .stack_size(64 << 20)
            .spawn(|| {
                let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
                let generated =
                    super::super::sources(&root, super::super::FillMode::Require).unwrap();
                let included = [
                    ("mod_api_generated.rs", generated.hooks.as_str()),
                    ("mod_api_generated_host.rs", generated.host.as_str()),
                    ("native_render_generated.rs", generated.render.as_str()),
                ];
                let wasm = super::generate(&root, &included).unwrap();
                let mut resolve = wit_parser::Resolve::default();
                resolve.push_str("mod-api.wit", &wasm.wit).unwrap();
                let client = wasm
                    .wit
                    .split("interface client-backend-mut {")
                    .nth(1)
                    .unwrap()
                    .split("\n}")
                    .next()
                    .unwrap();
                assert!(client.contains("menu-click:"));
                for definition in [
                    "variant menu-click {",
                    "variant swap-with {",
                    "enum mouse-button {",
                    "enum drag-kind {",
                ] {
                    assert!(wasm.wit.contains(definition), "missing {definition}");
                }
                let server = wasm
                    .wit
                    .split("interface backend-mut {")
                    .nth(1)
                    .unwrap()
                    .split("\n}")
                    .next()
                    .unwrap();
                assert!(!server.contains("menu-click:"));
                assert!(wasm.host.contains("crate::client_backend_mut_of(h)"));
                assert!(wasm.guest.contains("ClientBackendMutRef::of(self)"));
                assert!(
                    generated
                        .implementation
                        .contains("impl ClientBackendMut for")
                );
            })
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn criteria_bind_a_mod_test_and_a_scoped_world_context() {
        std::thread::Builder::new()
            .stack_size(64 << 20)
            .spawn(|| {
                let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
                let generated =
                    super::super::sources(&root, super::super::FillMode::Require).unwrap();
                let included = [
                    ("mod_api_generated.rs", generated.hooks.as_str()),
                    ("mod_api_generated_host.rs", generated.host.as_str()),
                    ("native_render_generated.rs", generated.render.as_str()),
                ];
                let wasm = super::generate(&root, &included).unwrap();
                let mut resolve = wit_parser::Resolve::default();
                let package = resolve.push_str("mod-api.wit", &wasm.wit).unwrap();
                let package = &resolve.packages[package];
                let context = &resolve.interfaces[package.interfaces["criterion-context"]];
                assert!(context.functions.contains_key("conditions-json"));
                assert!(context.functions.contains_key("item"));
                let test = &resolve.interfaces[package.interfaces["criterion-test"]];
                assert_eq!(test.functions["test"].params[0].name, "handler");
                let backend = &resolve.interfaces[package.interfaces["backend-mut"]];
                assert!(backend.functions.contains_key("player-trigger-criterion"));
                let host = &resolve.interfaces[package.interfaces["server-host"]];
                assert!(!host.functions.contains_key("trigger-criterion"));
                assert!(!wasm.unbound.iter().any(|name| name.contains("Criterion")));
            })
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    #[ignore]
    fn print_surface() {
        std::thread::Builder::new()
            .stack_size(64 << 20)
            .spawn(print)
            .unwrap()
            .join()
            .unwrap();
    }
    fn print() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let generated = super::super::sources(&root, super::super::FillMode::Ignore).unwrap();
        let included = [
            ("mod_api_generated.rs", generated.hooks.as_str()),
            ("mod_api_generated_host.rs", generated.host.as_str()),
            ("native_render_generated.rs", generated.render.as_str()),
        ];
        let wasm = super::generate(&root, &included).unwrap();
        for u in &wasm.unbound {
            println!("UNBOUND {u}");
        }
        let text = wasm.wit;
        std::fs::write("/box/work/wasm-spike/survey/mod-api.wit", &text).unwrap();
        std::fs::write("/box/work/wasm-spike/survey/guest.rs", &wasm.guest).unwrap();
        std::fs::write("/box/work/wasm-spike/survey/host.rs", &wasm.host).unwrap();
        let mut resolve = wit_parser::Resolve::default();
        resolve.push_str("mod-api.wit", &text).unwrap();
    }
}
