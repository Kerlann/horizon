use super::*;
use heck::ToUpperCamelCase;

/// The Backend reads a mob's goal tick answers from its read view (#1077): the
/// ticking mob itself, the player the tick sees, and the tick's snapshot of the
/// other entities. The generated view forwards these to the engine's
/// `GoalCx::r_*` methods; every other backend read of entity data is refused
/// its typed refusal (the handle docs of these carry the snapshot rule), and a
/// read that is not entity data forwards to the real game backend the view
/// carries, which answers it intact through the tick.
pub(super) const GOAL_VIEW_READS: &[&str] = &[
    "entity_position",
    "entity_block_position",
    "entity_velocity",
    "entity_rotation",
    "entity_head_yaw",
    "entity_bounding_box",
    "entity_eye_height",
    "entity_eye_position",
    "entity_distance_to_sqr",
    "entity_on_ground",
    "entity_kind",
    "living_health",
    "living_max_health",
    "living_alive",
];

/// The handle types whose backend reads are entity data (#1077): a read taking
/// one first answers the tick's own view of that entity — the snapshot, the
/// player the tick sees — and is refused where that carries it not. Every
/// other backend read (a level, a chunk, a block entity, a registry key)
/// names data the tick leaves in place, so the view forwards it to the real
/// game backend instead of refusing it.
const GOAL_VIEW_ENTITY_ARGS: &[&str] = &[
    "EntityRef",
    "LivingRef",
    "MobRef",
    "PlayerRef",
    "ItemEntityRef",
];

/// The `Host` methods that find entities (#1077): the goal view answers these
/// by its own rules (the ticking mob, the player the tick sees, the snapshot,
/// else `None`) instead of forwarding them, so nothing asks the game's taken
/// mob list. Everything else the view forwards to the game host it carries.
pub(super) const GOAL_VIEW_LOOKUPS: &[&str] = &[
    "entity",
    "living",
    "mob",
    "player",
    "item_entity",
    "block_entity",
];

pub(super) fn generate(
    root: &Path,
    catalog: &Catalog,
    code: &str,
) -> Result<(String, String, String), String> {
    let contexts = catalog.draw_contexts.as_slice();
    let api_source = fs::read_to_string(root.join("crates/horizon-mod-api/src/lib.rs"))
        .map_err(|e| e.to_string())?;
    let api_source = super::exports::expand(root, &api_source)?;
    let types = api_types(&api_source, code)?;
    let mut game = Vec::new();
    game_methods(
        &root.join("crates/horizon-client/src/game"),
        "horizon_client::game",
        &mut game,
    )?;
    let mut host_traits = String::new();
    let mut host_impl = String::new();
    let mut server_traits = String::new();
    let mut server_impl = String::new();
    let mut client_traits = String::new();
    let mut client_impl = String::new();
    let mut backend_reads = String::new();
    let mut backend_writes = String::new();
    let mut client_backend_writes = String::new();
    let mut backend_read_impl = String::new();
    let mut backend_write_impl = String::new();
    let mut client_backend_write_impl = String::new();
    // Each backend read's name, signature, argument names, typed refusal
    // (None where only a String forwards) and whether it reads entity data,
    // for the goal view.
    let mut backend_read_list: Vec<(&str, String, String, Option<&str>, bool)> = Vec::new();
    // Each `Host` method's name, signature, argument names, return type and
    // mutability, for the goal view's own `Host` impl.
    let mut host_method_list: Vec<(&str, String, String, String, bool)> = Vec::new();
    // Each draw context's trait methods and their forwarding by its game struct.
    let mut drawing: Vec<(String, String)> = contexts.iter().map(|_| Default::default()).collect();
    struct Service {
        side: Side,
        mutable: bool,
        resource_reads_only: bool,
        declarations: String,
        implementations: String,
    }
    let mut services = BTreeMap::<&str, Service>::new();
    let mut handles = std::collections::BTreeMap::<&str, String>::new();
    for helper in &catalog.helpers {
        let (name, side, target) = (helper.name.as_str(), helper.side, helper.target.as_str());
        let method = helper_target(&game, name, target)?;
        if let Some((index, declaration, implementation)) =
            draw_method(method, name, contexts, &types.exports)?
        {
            if helper.service.is_some() || helper.resource_read {
                return Err(format!(
                    "{name}: a service or published resource helper cannot draw"
                ));
            }
            if side != Side::Client {
                return Err(format!("{name}: drawing is client-side"));
            }
            drawing[index].0.push_str(&declaration);
            drawing[index].1.push_str(&implementation);
            continue;
        }
        let receiver = helper.receiver.as_deref();
        let is_backend = helper.backend;
        let service_trait = helper
            .service
            .as_ref()
            .map(|name| name.to_upper_camel_case());
        let trait_name = if let Some(service) = service_trait.as_deref() {
            service
        } else if receiver.is_some() || is_backend {
            if matches!(method.sig.inputs.first(), Some(FnArg::Receiver(r)) if r.mutability.is_some())
            {
                if side == Side::Client {
                    "ClientBackendMut"
                } else {
                    "BackendMut"
                }
            } else {
                "Backend"
            }
        } else {
            match side {
                Side::Both => "Host",
                Side::Server => "ServerHost",
                Side::Client => "ClientHost",
            }
        };
        let (declaration, implementation) = host_method(method, name, trait_name, &types.exports)?;
        // The catalog owns all forwarding boundaries, including services and handle backends.
        let access = if helper.resource_read {
            quote::quote!(mod_api::__private::LoadAccess::PublishedResources)
        } else if side == Side::Client
            && receiver.is_none()
            && !matches!(method.sig.inputs.first(), Some(FnArg::Receiver(r)) if r.mutability.is_some())
        {
            quote::quote!(mod_api::__private::LoadAccess::ClientSetup)
        } else {
            quote::quote!(mod_api::__private::LoadAccess::WorldOnly)
        };
        let guarded = |source: &str| -> Result<String, String> {
            let mut method = syn::parse_str::<syn::TraitItemFn>(source)
                .map_err(|error| format!("{name}: {error}"))?;
            method
                .default
                .as_mut()
                .expect("generated method has a body")
                .stmts
                .insert(
                    0,
                    syn::parse_quote!(mod_api::__private::check_access(#name, #access);),
                );
            Ok(method.to_token_stream().to_string())
        };
        let implementation = guarded(&implementation)?;
        let declaration = guarded(&declaration)?;
        let declaration = if helper.resource_read {
            format!(
                "/// Published asset/data read: available in Setup, Reloading and World.\n{declaration}"
            )
        } else {
            declaration
        };
        if let Some(client_target) = &helper.client_target {
            let client = helper_target(&game, name, client_target)?;
            let (_, client_implementation) = host_method(client, name, trait_name, &types.exports)?;
            matching_helper_signatures(&implementation, &client_implementation, name)?;
        }
        let side_doc = match side {
            Side::Server => "/// Side: server.\n",
            Side::Client => "/// Side: client.\n",
            Side::Both => "/// Side: both.\n",
        };
        if let Some(service) = helper.service.as_deref() {
            let group = services.entry(service).or_insert_with(|| Service {
                side,
                mutable: false,
                resource_reads_only: true,
                declarations: String::new(),
                implementations: String::new(),
            });
            if group.side != side {
                return Err(format!("{name}: all {service} methods must share a side"));
            }
            group.resource_reads_only &= helper.resource_read;
            group.mutable |= matches!(method.sig.inputs.first(), Some(FnArg::Receiver(r)) if r.mutability.is_some());
            group.declarations.push_str(side_doc);
            group.declarations.push_str(&declaration);
            group.implementations.push_str(&implementation);
        } else if receiver.is_some() || is_backend {
            let mutable = matches!(method.sig.inputs.first(), Some(FnArg::Receiver(r)) if r.mutability.is_some());
            if !mutable {
                let mut sig = method.sig.clone();
                sig.ident = syn::parse_str(name).map_err(|e| format!("{name}: {e}"))?;
                let args = sig
                    .inputs
                    .iter()
                    .skip(1)
                    .map(|arg| match arg {
                        FnArg::Typed(arg) => match &*arg.pat {
                            Pat::Ident(pat) => Ok(pat.ident.to_string()),
                            _ => Err(format!("{name}: unsupported argument")),
                        },
                        FnArg::Receiver(_) => Err(format!("{name}: unexpected receiver")),
                    })
                    .collect::<Result<Vec<_>, String>>()?
                    .join(", ");
                let refusal = match &sig.output {
                    syn::ReturnType::Default => {
                        return Err(format!("{name}: a backend read returns a value"));
                    }
                    syn::ReturnType::Type(_, ty) => match &**ty {
                        syn::Type::Path(path)
                            if path
                                .path
                                .segments
                                .last()
                                .is_some_and(|seg| seg.ident == "Option") =>
                        {
                            Some("None")
                        }
                        syn::Type::Path(path)
                            if path
                                .path
                                .segments
                                .last()
                                .is_some_and(|seg| seg.ident == "bool") =>
                        {
                            Some("false")
                        }
                        // A read that answers `Result`: its refusal is the
                        // error a host without a backend answers.
                        syn::Type::Path(path)
                            if path
                                .path
                                .segments
                                .last()
                                .is_some_and(|seg| seg.ident == "Result") =>
                        {
                            Some("Err(mod_api::NoBackend::no_backend())")
                        }
                        syn::Type::Path(path)
                            if path.path.segments.last().is_some_and(|seg| {
                                matches!(
                                    seg.ident.to_string().as_str(),
                                    "u32" | "i32" | "u16" | "u8" | "usize"
                                )
                            }) =>
                        {
                            Some("0")
                        }
                        // A registry key's metadata: a read the goal view only
                        // forwards — a host without a backend answers "".
                        syn::Type::Path(path)
                            if path
                                .path
                                .segments
                                .last()
                                .is_some_and(|seg| seg.ident == "String") =>
                        {
                            None
                        }
                        _ => {
                            return Err(format!(
                                "{name}: the goal view cannot refuse its return type"
                            ));
                        }
                    },
                };
                backend_read_list.push((
                    name,
                    sig.to_token_stream().to_string(),
                    args,
                    refusal,
                    // Entity data answers the tick's view; everything else
                    // forwards to the real backend the view carries.
                    matches!(
                        method.sig.inputs.iter().skip(1).find_map(|arg| match arg {
                            FnArg::Typed(arg) => Some(&*arg.ty),
                            FnArg::Receiver(_) => None,
                        }),
                        Some(syn::Type::Path(ty))
                            if ty.path.segments.len() == 1
                                && GOAL_VIEW_ENTITY_ARGS
                                    .contains(&ty.path.segments[0].ident.to_string().as_str())
                    ),
                ));
            }
            let (trait_methods, impl_methods) = if mutable && side == Side::Client {
                (&mut client_backend_writes, &mut client_backend_write_impl)
            } else if mutable {
                (&mut backend_writes, &mut backend_write_impl)
            } else {
                (&mut backend_reads, &mut backend_read_impl)
            };
            trait_methods.push_str(side_doc);
            trait_methods.push_str(&declaration);
            impl_methods.push_str(&implementation);
            if let Some(receiver) = receiver {
                let (handle, code) = handle_method(method, name, receiver, side, &types.handles)?;
                let methods = handles.entry(handle).or_default();
                methods.push_str(side_doc);
                methods.push_str(&code);
            }
        } else {
            // The goal view implements every `Host` method (#1077): the
            // entity lookups answer the tick's own rules, everything else
            // forwards to the game host the view carries.
            if side == Side::Both {
                let mutable = matches!(method.sig.inputs.first(), Some(FnArg::Receiver(r)) if r.mutability.is_some());
                let mut sig = method.sig.clone();
                sig.ident = syn::parse_str(name).map_err(|e| format!("{name}: {e}"))?;
                // The trait's shape, as `host_method` builds it: a helper
                // without a receiver becomes a `&self` method there.
                if !matches!(sig.inputs.first(), Some(FnArg::Receiver(_))) {
                    sig.inputs.insert(0, syn::parse_quote!(&self));
                }
                let args = sig
                    .inputs
                    .iter()
                    .skip(1)
                    .map(|arg| match arg {
                        FnArg::Typed(arg) => match &*arg.pat {
                            Pat::Ident(pat) => Ok(pat.ident.to_string()),
                            _ => Err(format!("{name}: unsupported argument")),
                        },
                        FnArg::Receiver(_) => Err(format!("{name}: unexpected receiver")),
                    })
                    .collect::<Result<Vec<_>, String>>()?
                    .join(", ");
                let ret = match &sig.output {
                    syn::ReturnType::Default => "()".to_string(),
                    syn::ReturnType::Type(_, ty) => ty.to_token_stream().to_string(),
                };
                host_method_list.push((
                    name,
                    sig.to_token_stream().to_string(),
                    args,
                    ret,
                    mutable,
                ));
            }
            let (traits, implementation_out) = match side {
                Side::Both => (&mut host_traits, &mut host_impl),
                Side::Server => (&mut server_traits, &mut server_impl),
                Side::Client => (&mut client_traits, &mut client_impl),
            };
            traits.push_str(side_doc);
            traits.push_str(&declaration);
            implementation_out.push_str(&implementation);
        }
    }
    let mut service_traits = String::new();
    let mut service_impls = String::new();
    for (name, service) in services {
        let trait_name = name.to_upper_camel_case();
        let borrow = if service.mutable { "&mut" } else { "&" };
        let access = if service.resource_reads_only {
            "PublishedResources"
        } else {
            "WorldOnly"
        };
        let (traits, implementation) = match service.side {
            Side::Both => (&mut host_traits, &mut host_impl),
            Side::Server => (&mut server_traits, &mut server_impl),
            Side::Client => (&mut client_traits, &mut client_impl),
        };
        traits.push_str(&format!(
            "/// The {name} service.\n/// Not on wasm: a frame resource; its methods are the imports.\nfn {name}({borrow} self) -> {borrow} dyn {trait_name} {{ mod_api::__private::check_access(\"{name}\", mod_api::__private::LoadAccess::{access}); unimplemented!(\"{trait_name} service\") }}\n"
        ));
        implementation.push_str(&format!(
            "fn {name}({borrow} self) -> {borrow} dyn {trait_name} {{ mod_api::__private::check_access(\"{name}\", mod_api::__private::LoadAccess::{access}); self }}\n"
        ));
        service_traits.push_str(&format!(
            "/// The {name} service, reached through its host.\npub trait {trait_name} {{\n{} }}\n",
            service.declarations
        ));
        service_impls.push_str(&format!(
            "impl {trait_name} for super::Game<'_> {{\n{} }}\n",
            service.implementations
        ));
    }
    let handles = handles
        .iter()
        .map(|(handle, methods)| format!("impl {handle} {{\n{methods}}}\n"))
        .collect::<String>();
    struct StdTypes {
        path_buf: bool,
        arc: bool,
    }
    impl<'ast> Visit<'ast> for StdTypes {
        fn visit_type_path(&mut self, ty: &'ast syn::TypePath) {
            if ty.path.segments.len() == 1 {
                self.path_buf |= ty.path.segments[0].ident == "PathBuf";
                self.arc |= ty.path.segments[0].ident == "Arc";
            }
            syn::visit::visit_type_path(self, ty);
        }
    }
    let host_trait: syn::ItemTrait = syn::parse_str(&format!(
        "trait Host {{ {host_traits} {server_traits} {client_traits} {backend_reads} {backend_writes} {client_backend_writes} }}"
    ))
    .map_err(|e| format!("generated Host: {e}"))?;
    let mut std_types = StdTypes {
        path_buf: false,
        arc: false,
    };
    std_types.visit_item_trait(&host_trait);
    let mut imports = String::new();
    if std_types.path_buf {
        imports.push_str("use std::path::PathBuf;\n");
    }
    if std_types.arc {
        imports.push_str("use std::sync::Arc;\n");
    }
    let side_enum = |name: &str, server: &str, client: &str| {
        format!(
            "/// The logical side of a hook's host.\npub enum {name}<'a> {{\n/// The hook fired on the server.\nServer({server}),\n/// The hook fired on the client.\nClient({client}) }}\n"
        )
    };
    let sides = format!(
        "{}{}",
        side_enum(
            "HostSide",
            "&'a mut dyn ServerHost",
            "&'a mut dyn ClientHost"
        ),
        side_enum("LevelSide", "&'a dyn LevelHost", "&'a dyn LevelHost")
    );
    let mut draw_traits = String::new();
    let mut draw_impls = String::new();
    for (context, (declarations, implementations)) in contexts.iter().zip(&drawing) {
        let (doc, trait_name, game_struct, surface) = (
            &context.doc,
            &context.trait_name,
            context.game_struct(),
            &context.surface,
        );
        draw_traits.push_str(&format!(
            "/// {doc}\n/// Each method's default panics with its name, so a test double implements only what its test calls.\npub trait {trait_name} {{\n{declarations}}}\n"
        ));
        draw_impls.push_str(&format!(
            "/// [`{trait_name}`] on the game's `{surface}`.\npub(crate) struct {game_struct}<'a, 'g> {{\npub(crate) game: &'a super::Game<'g>,\npub(crate) surface: &'a mut {surface},\n}}\nimpl {trait_name} for {game_struct}<'_, '_> {{\n{implementations}}}\n"
        ));
    }
    let host = format!(
        "// @generated by cargo run -p xtask -- mod-api; do not edit.\nuse super::*;\n{imports}/// Services supplied by the game to trait mods.\n/// Each method's default panics with its name, so a test double implements only what its test calls; the game implements every one.\n/// The backend accessors are the exception: they default to None, so a double without a backend answers every handle's typed refusal.\npub trait Host {{\n#[doc(hidden)] fn backend(&self) -> Option<BackendRef<'_>> {{ None }}\n#[doc(hidden)] fn capability_providers(&self) -> &'static dyn CapabilityProviders {{ &InstalledProviders }}\n{host_traits}}}\n/// What only the server's hooks are given: writes to the world.\npub trait ServerHost: Host {{\n#[doc(hidden)] fn backend_mut(&mut self) -> Option<BackendMutRef<'_>> {{ None }}\n{server_traits}}}\n/// What only the client's hooks are given: the client's own services.\npub trait ClientHost: Host {{\n#[doc(hidden)] fn client_backend_mut(&mut self) -> Option<ClientBackendMutRef<'_>> {{ None }}\n{client_traits}}}\n{sides}#[doc(hidden)] pub trait Backend {{\n{backend_reads}}}\n#[doc(hidden)] pub trait BackendMut {{\n{backend_writes}}}\n#[doc(hidden)] pub trait ClientBackendMut {{\n{client_backend_writes}}}\n{handles}{draw_traits}{service_traits}"
    );
    let implementation = format!(
        "// @generated by cargo run -p xtask -- mod-api; do not edit.\nuse mod_api::*;\nuse mod_api::__private::game::*;\n{imports}impl Host for super::Game<'_> {{\nfn backend(&self) -> Option<BackendRef<'_>> {{ Some(BackendRef::of(self)) }}\n{host_impl}}}\nimpl ServerHost for super::Game<'_> {{\nfn backend_mut(&mut self) -> Option<BackendMutRef<'_>> {{ Some(BackendMutRef::of(self)) }}\n{server_impl}}}\nimpl ClientHost for super::Game<'_> {{\nfn client_backend_mut(&mut self) -> Option<ClientBackendMutRef<'_>> {{ Some(ClientBackendMutRef::of(self)) }}\n{client_impl}}}\nimpl Backend for super::Game<'_> {{\n{backend_read_impl}}}\nimpl BackendMut for super::Game<'_> {{\n{backend_write_impl}}}\nimpl ClientBackendMut for super::Game<'_> {{\n{client_backend_write_impl}}}\n{draw_impls}{service_impls}"
    );
    let goal_view = goal_view(&backend_read_list, &host_method_list)?;
    Ok((host, implementation, goal_view))
}

/// The goal view's generated impls (#1077): the `Backend` one forwards the
/// snapshot-backed reads to the engine context's `r_*` methods, refuses its
/// typed refusal for entity data the tick's view does not carry, and forwards
/// every other backend read — the level's own: blocks, loaded chunks, time,
/// height, weather, block entities — to the real backend of the game host the
/// view carries; the `Host` one answers the entity lookups by the tick's own
/// rules, refuses the mutable methods, and forwards everything else to the
/// game host the view carries. The engine includes both beside its `GoalCx`.
fn goal_view(
    reads: &[(&str, String, String, Option<&str>, bool)],
    host_methods: &[(&str, String, String, String, bool)],
) -> Result<String, String> {
    let mut backend = String::new();
    for (name, sig, args, refusal, entity_data) in reads {
        // How a forwarded read answers a host without a backend: the typed
        // refusal, or the empty string of a metadata read.
        let forwarded = match *refusal {
            Some("None") => ".flatten()".to_string(),
            // A `Result` read's no-backend answer is its error type's own
            // `NoBackend` value, wrapped back in the `Err`.
            Some(refusal) if refusal.starts_with("Err(") => {
                ".unwrap_or_else(|| Err(mod_api::NoBackend::no_backend()))".to_string()
            }
            Some(refusal) => format!(".unwrap_or({refusal})"),
            None => ".unwrap_or_default()".to_string(),
        };
        let body = if GOAL_VIEW_READS.contains(name) {
            format!("self.r_{name}({args})")
        } else if *entity_data {
            let Some(refusal) = refusal else {
                return Err(format!(
                    "{name}: the goal view cannot refuse its return type"
                ));
            };
            if args.is_empty() {
                (*refusal).to_string()
            } else if args.contains(',') {
                format!("let _ = ({args}); {refusal}")
            } else {
                format!("let _ = {args}; {refusal}")
            }
        } else {
            // The read names data the tick leaves in place, so the real
            // backend answers it: `backend_read` holds the `None` a host
            // without one answers.
            format!(
                "mod_api::__private::backend_read(self.host, |backend| backend.{name}({args})){forwarded}"
            )
        };
        backend.push_str(&format!("{sig} {{ {body} }}\n"));
    }
    let mut host = String::from(
        "#[doc(hidden)]\nfn backend(&self) -> Option<BackendRef<'_>> { Some(mod_api::__private::backend_ref(self)) }\n",
    );
    for (name, sig, args, ret, mutable) in host_methods {
        if GOAL_VIEW_LOOKUPS.contains(name) {
            host.push_str(&format!("{sig} {{ self.lookup_{name}({args}) }}\n"));
        } else if *mutable {
            // A `&mut self` Host method cannot be called on the `&dyn Host`
            // the context hands out, so the view leaves it to the trait's
            // panicking default: a goal cannot reach it (that is the point).
            continue;
        } else {
            let _ = ret;
            host.push_str(&format!("{sig} {{ self.host.{name}({args}) }}\n"));
        }
    }
    Ok(format!(
        "// @generated by cargo run -p xtask -- mod-api; do not edit.\nuse mod_api::*;\nuse std::path::PathBuf;\nuse std::sync::Arc;\n/// The mob tick's read view (#1077), as the game generates it: the snapshot-backed\n/// reads forward to the engine context's own methods, entity data the view does\n/// not carry answers its typed refusal, and every other backend read forwards\n/// to the real backend of the game host the view carries.\nimpl Backend for super::GoalCx<'_, '_> {{\n{backend}}}\n/// The same view as a host: the entity lookups answer the tick's own rules\n/// (the ticking mob, the player it sees, the snapshot, else None), the mutable\n/// methods stay out of reach of the shared host the view holds, and everything\n/// else - the level, its blocks, tags, recipes and settings - forwards to the\n/// game host the view carries.\nimpl Host for super::GoalCx<'_, '_> {{\n{host}}}"
    ))
}

/// A draw helper's method on its context's trait, when one of `method`'s inputs is a draw
/// surface: the context's index, the trait's declaration, and the game struct's forwarding.
/// The trait method borrows itself as the helper borrows the surface; the helper borrows
/// `Game` shared, as the struct holds it.
fn draw_method(
    method: &syn::ItemFn,
    name: &str,
    contexts: &[DrawContext],
    exports: &HashSet<String>,
) -> Result<Option<(usize, String, String)>, String> {
    let Some((position, context, mutable)) =
        method
            .sig
            .inputs
            .iter()
            .enumerate()
            .find_map(|(position, arg)| match arg {
                FnArg::Typed(arg) => DrawContext::of(contexts, &arg.ty)
                    .map(|(context, mutable)| (position, context, mutable)),
                FnArg::Receiver(_) => None,
            })
    else {
        return Ok(None);
    };
    if !matches!(method.sig.inputs.first(), Some(FnArg::Receiver(r)) if r.reference.is_some() && r.mutability.is_none())
    {
        return Err(format!("{name}: a draw helper borrows Game shared (&self)"));
    }
    let index = contexts
        .iter()
        .position(|other| std::ptr::eq(other, context))
        .expect("context found");
    let short = name.strip_prefix(context.prefix.as_str()).ok_or_else(|| {
        format!(
            "{name}: a {} helper's name starts with {}",
            context.trait_name, context.prefix
        )
    })?;
    let mut stripped = method.clone();
    stripped.sig.inputs = stripped
        .sig
        .inputs
        .into_iter()
        .enumerate()
        .filter(|(at, _)| *at != position)
        .map(|(at, arg)| match at {
            0 if mutable => syn::parse_quote!(&mut self),
            _ => arg,
        })
        .collect();
    let (declaration, _) = host_method(&stripped, short, &context.trait_name, exports)?;
    let sig = syn::parse_str::<syn::TraitItemFn>(&declaration)
        .map_err(|e| format!("{name}: {e}"))?
        .sig;
    let surface = if mutable {
        "&mut *self.surface"
    } else {
        "&*self.surface"
    };
    let args = method
        .sig
        .inputs
        .iter()
        .enumerate()
        .skip(1)
        .map(|(at, arg)| match arg {
            _ if at == position => Ok(surface.to_owned()),
            FnArg::Typed(arg) => match &*arg.pat {
                Pat::Ident(pat) => Ok(pat.ident.to_string()),
                _ => Err(format!("{name}: unsupported argument")),
            },
            FnArg::Receiver(_) => Err(format!("{name}: unexpected receiver")),
        })
        .collect::<Result<Vec<_>, String>>()?;
    let implementation = format!(
        "{} {{ self.game.{}({}) }}\n",
        sig.to_token_stream(),
        method.sig.ident,
        args.join(", ")
    );
    Ok(Some((index, declaration, implementation)))
}
