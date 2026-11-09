//! Derive hook subscriptions from a trait implementation.
use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{
    ImplItem, ImplItemFn, ItemImpl, Meta, Token, parse::Parser, parse_macro_input,
    punctuated::Punctuated,
};

struct RawFramePlan {
    roots: usize,
    read: syn::Block,
    apply: syn::Block,
}
struct RawPlan {
    plan: Option<usize>,
    head: RawFramePlan,
    tail: Option<RawFramePlan>,
}
impl syn::parse::Parse for RawPlan {
    fn parse(input: syn::parse::ParseStream<'_>) -> syn::Result<Self> {
        fn frame(input: syn::parse::ParseStream<'_>) -> syn::Result<RawFramePlan> {
            let roots = input.parse::<syn::LitInt>()?.base10_parse()?;
            input.parse::<Token![,]>()?;
            let read = input.parse()?;
            input.parse::<Token![,]>()?;
            let apply = input.parse()?;
            Ok(RawFramePlan { roots, read, apply })
        }
        let plan = if input.peek(syn::Ident) {
            let word: syn::Ident = input.parse()?;
            if word != "plan" {
                return Err(syn::Error::new(word.span(), "expected `plan`"));
            }
            let id = input.parse::<syn::LitInt>()?.base10_parse()?;
            input.parse::<Token![,]>()?;
            Some(id)
        } else {
            None
        };
        let head = frame(input)?;
        let tail = if input.is_empty() {
            None
        } else {
            input.parse::<Token![,]>()?;
            Some(frame(input)?)
        };
        Ok(Self { plan, head, tail })
    }
}

/// The staged call site's typed boundary. Only generated source invokes this macro.
#[proc_macro]
pub fn site(input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(input with Punctuated::<syn::Expr, Token![,]>::parse_terminated);
    let args: Vec<_> = args.iter().collect();
    let [
        wrap,
        place,
        listener,
        recv_ty,
        record,
        output,
        receiver,
        args,
        original,
    ] = args.as_slice()
    else {
        return syn::Error::new(
            proc_macro2::Span::call_site(),
            "expected fresh wrap name, site types, receiver, args and original",
        )
        .to_compile_error()
        .into();
    };
    quote! {{
        fn #wrap<'g>(__at: usize, __receiver: #recv_ty<'g, '_>, __args: &mut #record,
            __original: &mut dyn FnMut(#recv_ty<'g, '_>, &mut #record) -> #output) -> #output {
            ::mod_points::__private::wrap(::mod_points::SiteId::#place, __at, |__entry| {
                match __entry {
                    Some((__pointer, __next)) => {
                        // SAFETY: registration checks this exact site's listener type.
                        let __listener: #listener = unsafe { ::std::mem::transmute(__pointer) };
                        __listener(__receiver, __args, &mut |__recv, __args| #wrap(__next, __recv, __args, __original))
                    }
                    None => __original(__receiver, __args),
                }
            })
        }
        #wrap(0, #receiver, &mut #args, &mut #original)
    }}.into()
}

/// Keep only attributes that can gate a method, including nested `cfg_attr`s.
fn cfg_meta(meta: &Meta) -> Option<Meta> {
    if meta.path().is_ident("cfg") {
        return Some(meta.clone());
    }
    let Meta::List(list) = meta else { return None };
    if !list.path.is_ident("cfg_attr") {
        return None;
    }
    let args = Punctuated::<Meta, Token![,]>::parse_terminated
        .parse2(list.tokens.clone())
        .ok()?;
    let mut args = args.iter();
    let condition = args.next()?;
    let gated: Vec<_> = args.filter_map(cfg_meta).collect();
    if gated.is_empty() {
        None
    } else {
        Some(syn::parse_quote!(cfg_attr(#condition, #(#gated),*)))
    }
}

fn cfg_attributes(method: &ImplItemFn) -> Vec<syn::Attribute> {
    method
        .attrs
        .iter()
        .filter_map(|attr| cfg_meta(&attr.meta))
        .map(|meta| syn::parse_quote!(#[#meta]))
        .collect()
}

/// A method's `#[listener(priority = …, receive_cancelled)]`, removed from the method.
struct ListenerOptions {
    priority: syn::Expr,
    /// The attribute's span, for the error when the hook has no cancel to receive.
    receive_cancelled: Option<proc_macro2::Span>,
}

/// Takes `method`'s `#[listener]`, if it has one.
fn take_listener(method: &mut ImplItemFn) -> syn::Result<Option<ListenerOptions>> {
    let Some(index) = method
        .attrs
        .iter()
        .position(|attr| attr.path().is_ident("listener"))
    else {
        return Ok(None);
    };
    let attr = method.attrs.remove(index);
    if let Some(again) = method
        .attrs
        .iter()
        .find(|attr| attr.path().is_ident("listener"))
    {
        return Err(syn::Error::new_spanned(
            again,
            "a hook method takes one #[listener]",
        ));
    }
    let mut options = ListenerOptions {
        priority: syn::parse_quote!(0),
        receive_cancelled: None,
    };
    attr.parse_nested_meta(|meta| {
        if meta.path.is_ident("priority") {
            options.priority = meta.value()?.parse()?;
            Ok(())
        } else if meta.path.is_ident("receive_cancelled") {
            options.receive_cancelled = Some(meta.path.get_ident().unwrap().span());
            Ok(())
        } else {
            Err(meta.error("expected `priority = <i32>` or `receive_cancelled`"))
        }
    })?;
    Ok(Some(options))
}

fn uninspectable(item: &ImplItem) -> bool {
    matches!(item, ImplItem::Macro(_) | ImplItem::Verbatim(_))
}

fn expand(mut implementation: ItemImpl, export: bool) -> TokenStream {
    let Some((_, trait_path, _)) = &implementation.trait_ else {
        return syn::Error::new_spanned(
            &implementation,
            "expected an impl of Hooks or a Part trait",
        )
        .into_compile_error()
        .into();
    };
    let name = &trait_path.segments.last().unwrap().ident;
    let part = match name.to_string().as_str() {
        "Hooks" => None,
        "BlockPart" => Some("block_subscribed"),
        "BlockEntityPart" => Some("block_entity_subscribed"),
        "ItemPart" => Some("item_subscribed"),
        "MenuPart" => Some("menu_subscribed"),
        "FluidPart" => Some("fluid_subscribed"),
        // Entity parts carry no scoped hook yet (#1077): `register_goals` is
        // not one, so its subscriptions stay empty whatever it overrides.
        "EntityPart" => Some("entity_subscribed_empty"),
        _ => {
            return syn::Error::new_spanned(name, "expected a hook or part trait")
                .into_compile_error()
                .into();
        }
    };
    if part.is_some()
        && let Some(item) = implementation.items.iter().find(|item| uninspectable(item))
    {
        let ty = &implementation.self_ty;
        return syn::Error::new_spanned(
            item,
            format!("uninspectable items in impl {trait_path} for {ty} are not supported by horizon_part; write hook methods explicitly", trait_path = quote!(#trait_path), ty = quote!(#ty)),
        )
        .into_compile_error()
        .into();
    }
    let mut methods = Vec::new();
    let mut cfgs = Vec::new();
    let mut listeners = Vec::new();
    for item in &mut implementation.items {
        if let ImplItem::Fn(method) = item {
            match take_listener(method) {
                Ok(Some(_)) if part.is_some() => {
                    return syn::Error::new_spanned(
                        &method.sig.ident,
                        "#[listener] orders a trait mod's hooks; a part's are called for its own thing",
                    )
                    .into_compile_error()
                    .into();
                }
                Ok(Some(options)) => {
                    listeners.push((method.sig.ident.clone(), cfg_attributes(method), options))
                }
                Ok(None) => {}
                Err(error) => return error.into_compile_error().into(),
            }
            methods.push(method.sig.ident.clone());
            cfgs.push(cfg_attributes(method));
        }
    }
    let id = format_ident!("HookId");
    let ids = format_ident!("hook_ids");
    let subscribed_name = format_ident!("{}", part.unwrap_or("subscribed"));
    let subscribed = if subscribed_name == "entity_subscribed_empty" {
        // Entity parts (arismoko/horizon#1077): no scoped hook exists for them
        // yet, so `register_goals` cannot be mistaken for one.
        syn::parse_quote! {
            fn entity_subscribed(&self) -> &'static [Option<mod_api::HookId>] {
                &[]
            }
        }
    } else {
        syn::parse_quote! {
            fn #subscribed_name(&self) -> &'static [Option<mod_api::#id>] {
                &[#(#(#cfgs)* mod_api::#ids::#methods),*]
            }
        }
    };
    implementation.items.push(subscribed);
    if !listeners.is_empty() {
        let entries = listeners.iter().map(|(method, cfgs, options)| {
            let priority = &options.priority;
            let receive_cancelled = options.receive_cancelled.is_some();
            quote! { #(#cfgs)* mod_api::Listener { hook: mod_api::#ids::#method, priority: #priority, receive_cancelled: #receive_cancelled } }
        });
        // Only a hook a mod may cancel has a constant here, so the option fails to compile on
        // any other.
        let checks = listeners.iter().filter_map(|(method, cfgs, options)| {
            let at_option = syn::Ident::new(&method.to_string(), options.receive_cancelled?);
            Some(quote! { #(#cfgs)* const _: () = mod_api::cancel_hooks::#at_option; })
        });
        implementation.items.push(syn::parse_quote! {
            fn listeners(&self) -> &'static [mod_api::Listener] {
                #(#checks)*
                &[#(#entries),*]
            }
        });
    }
    let export = if export && part.is_none() {
        let ty = &implementation.self_ty;
        let registrations: proc_macro2::TokenStream =
            match std::env::var("HORIZON_GUEST_REGISTRATIONS") {
                Ok(source) => match source.parse::<proc_macro2::TokenStream>() {
                    Ok(tokens) => tokens,
                    Err(error) => {
                        return syn::Error::new_spanned(
                            &implementation,
                            format!("guest registrations: {error}"),
                        )
                        .to_compile_error()
                        .into();
                    }
                },
                Err(_) => quote!(),
            };
        quote! {
            pub fn hooks() -> Box<dyn mod_api::Hooks> { Box::new(<#ty as Default>::default()) }
            /// The mod's hooks for the guest runtime in `mod_api::wire::guest`, when built to wasm.
            #[cfg(target_arch = "wasm32")]
            #[unsafe(no_mangle)]
            fn __horizon_mod_hooks() -> Box<dyn mod_api::Hooks> {
                #registrations
                hooks()
            }
        }
    } else {
        quote!()
    };
    quote!(#implementation #export).into()
}

/// Generate the panicking defaults of a host area from each method's access declaration.
/// The same declarations drive game forwarders in the build-time API generator.
#[proc_macro_attribute]
pub fn host_area(_attribute: TokenStream, input: TokenStream) -> TokenStream {
    let mut area = parse_macro_input!(input as syn::ItemTrait);
    let name = area.ident.to_string();
    let union = [
        "ClientBackendMut",
        "BackendMut",
        "ClientBackend",
        "ServerHost",
        "ClientHost",
        "Backend",
        "Host",
    ]
    .into_iter()
    .find(|suffix| name.ends_with(suffix))
    .unwrap_or("Host");
    let union = if union == "ClientBackend" {
        "Backend"
    } else {
        union
    };
    for member in &mut area.items {
        let syn::TraitItem::Fn(method) = member else {
            continue;
        };
        let Some(index) = method
            .attrs
            .iter()
            .position(|attr| attr.path().is_ident("access"))
        else {
            return syn::Error::new_spanned(&method.sig, "host method needs #[access(Level)]")
                .to_compile_error()
                .into();
        };
        let attr = method.attrs.remove(index);
        let level = match attr.parse_args::<syn::Ident>() {
            Ok(level) => level,
            Err(error) => return error.to_compile_error().into(),
        };
        method.attrs.retain(|attr| {
            !attr.path().is_ident("static_call") && !attr.path().is_ident("service")
        });
        let operation = method.sig.ident.to_string();
        let label = format!("{union}::{operation}");
        method.default = Some(syn::parse_quote!({
            crate::phase::unprovided(#label, #operation, crate::phase::LoadAccess::#level)
        }));
        method.semi_token = None;
    }
    quote!(#area).into()
}

/// Mark a trait mod and derive its hook subscriptions.
///
/// A hook method may carry `#[listener(priority = <i32>, receive_cancelled)]`, both optional:
/// mods run by descending priority (0 when omitted), then in dependency order, and a mod that
/// receives cancelled events is still called after an earlier mod cancels, with
/// `event.cancelled` to read and clear. `receive_cancelled` compiles only on a hook a mod may
/// cancel.
#[proc_macro_attribute]
pub fn horizon_mod(_attribute: TokenStream, input: TokenStream) -> TokenStream {
    expand(parse_macro_input!(input as ItemImpl), true)
}

/// Mark a part implementation and derive the scoped hooks it overrides.
///
/// Hook methods must be written explicitly, not generated inside the impl:
///
/// ```compile_fail
/// use horizon_mod_api_macros::horizon_part;
/// trait BlockPart { fn tick(&self) {} }
/// struct Block;
/// macro_rules! hook { () => { fn tick(&self) {} }; }
/// #[horizon_part]
/// impl BlockPart for Block { hook!(); }
/// ```
#[proc_macro_attribute]
pub fn horizon_part(_attribute: TokenStream, input: TokenStream) -> TokenStream {
    expand(parse_macro_input!(input as ItemImpl), false)
}

/// `#[point(side = "server" | "client" | "both", name = "…", raw, free)]` on a function: an
/// injection point (#589), a slot the launcher fills at load for the mods that listen there. The
/// staging pass that builds the prebuilt game (and the launcher's source builds) puts it on every
/// function [`point_shape::eligible`] admits; the source tree carries none. Its head checks the slot
/// (one atomic load and a branch while nothing listens), then hands each API function listener the
/// parameters with API types (and, on a `Game` method with a `side`, the side's host), and each raw
/// listener (`raw`: the pass declared its record and listener types beside the function) the
/// receiver and a `&mut` to every parameter; each may change them. Its return does the same with
/// the value returned; a value that borrows reaches only raw listeners, as itself. `free` marks a
/// free function, whose body moves to a sibling function rather than an associated one.
#[proc_macro_attribute]
pub fn point(attribute: TokenStream, input: TokenStream) -> TokenStream {
    let mut function = parse_macro_input!(input as ImplItemFn);
    let raw_plan = match function
        .attrs
        .iter()
        .position(|attr| attr.path().is_ident("horizon_raw_plan"))
    {
        Some(index) => match function.attrs.remove(index).parse_args::<RawPlan>() {
            Ok(plan) => Some(plan),
            Err(error) => return error.to_compile_error().into(),
        },
        None => None,
    };
    if let Some(inline) = function.attrs.iter().find(|attr| {
        attr.path().is_ident("inline")
            && attr
                .parse_args::<syn::Ident>()
                .is_ok_and(|arg| arg == "always")
    }) {
        return syn::Error::new_spanned(
            inline,
            "a point keeps its function out of line: drop #[inline(always)]",
        )
        .to_compile_error()
        .into();
    }
    let shape = match point_shape::shape(attribute.into(), &function.sig) {
        Ok(shape) => shape,
        Err(e) => return e.to_compile_error().into(),
    };
    // The body moves to an inlined inner function, so the return listeners run however it
    // returns; the outer function takes each parameter by a plain, mutable name.
    let inner = syn::Ident::new(
        &format!(
            "__point_{}",
            function.sig.ident.to_string().trim_start_matches("r#")
        ),
        function.sig.ident.span(),
    );
    let mut outer = function.clone();
    let mut call = Vec::new();
    let mut passed = Vec::new();
    // Where each typed parameter lives while the listeners run: in the API record, or a local.
    let mut places = std::collections::HashMap::new();
    // For each native-width parameter: the raw listener's local, and its write-back.
    let mut widened = Vec::new();
    for (i, input) in outer.sig.inputs.iter_mut().enumerate() {
        let syn::FnArg::Typed(typed) = input else {
            continue;
        };
        let original = match &*typed.pat {
            syn::Pat::Ident(pat) => Some(pat.ident.clone()),
            _ => None,
        };
        let plain = format_ident!("__p{i}");
        *typed.pat = syn::parse_quote!(mut #plain);
        match original
            .filter(|_| !shape.raw_only)
            .and_then(|name| shape.args.iter().find(|arg| arg.name == name))
        {
            Some(arg) => {
                let name = &arg.name;
                passed.push(plain);
                match point_shape::wide(&arg.ty) {
                    // A native width crosses the record as 64 bits, and a raw listener gets it
                    // back in its own type, in a local written back after it.
                    Some(wide) => {
                        let (ty, local) = (&arg.ty, format_ident!("__w{i}"));
                        places.insert(i, quote!(#local));
                        widened.push((
                            quote!(let mut #local = __args.#name as #ty;),
                            quote!(__args.#name = #local as #wide;),
                        ));
                        call.push(quote!(__args.#name as #ty));
                    }
                    None => {
                        places.insert(i, quote!(__args.#name));
                        call.push(match arg.ty {
                            syn::Type::Reference(_) => quote!(&*__args.#name),
                            _ => quote!(__args.#name),
                        });
                    }
                }
            }
            None => {
                places.insert(i, quote!(#plain));
                call.push(quote!(#plain));
            }
        }
    }
    // The inner function's type and const parameters, named at the call: one only in the return
    // type can't be inferred through it.
    let generics: Vec<_> = function
        .sig
        .generics
        .params
        .iter()
        .filter_map(|param| match param {
            syn::GenericParam::Type(ty) => Some(&ty.ident),
            syn::GenericParam::Const(c) => Some(&c.ident),
            syn::GenericParam::Lifetime(_) => None,
        })
        .collect();
    let turbofish = if generics.is_empty() {
        quote!()
    } else {
        quote!(::<#(#generics),*>)
    };
    let (receiver, callee) = match (shape.receiver, shape.free) {
        (point_shape::Receiver::None, true) => (quote!(), quote!(#inner)),
        (point_shape::Receiver::None, false) => (quote!(), quote!(Self::#inner)),
        (point_shape::Receiver::Shared, _) => (quote!(self,), quote!(self.#inner)),
        (point_shape::Receiver::Mut, _) => (quote!(&mut *self,), quote!(self.#inner)),
    };
    let erased_receiver = match shape.receiver {
        point_shape::Receiver::None => quote!(::std::ptr::null_mut()),
        point_shape::Receiver::Shared => quote!(self as *const Self as *mut ()),
        point_shape::Receiver::Mut => quote!(&mut *self as *mut Self as *mut ()),
    };
    // What the API listeners get before their record: the shape's host, if it has one.
    let host = match (shape.receiver, shape.side) {
        (_, None) => quote!(),
        (point_shape::Receiver::Mut, Some(point_shape::Side::Both)) => {
            quote!(crate::game::points::host_side(&mut *self),)
        }
        (point_shape::Receiver::Mut, Some(_)) => quote!(&mut *self,),
        (_, Some(_)) => quote!(self,),
    };
    let record = shape.record();
    let outcome = shape.raw_outcome(&function.sig);
    // A raw `result` is the API record's when the API carries the value, else a slot of its own.
    let api_result = !shape.raw_only && matches!(shape.returns, point_shape::Returns::Value(_));
    let cancel = if shape.raw_only {
        quote!(__cancel)
    } else {
        quote!(__args.cancel)
    };
    // A native-width value crosses the record as 64 bits.
    let (widen, narrow) = match &shape.returns {
        point_shape::Returns::Value(ty) => match point_shape::wide(ty) {
            Some(wide) => (quote!(as #wide), quote!(as #ty)),
            None => (quote!(), quote!()),
        },
        _ => (quote!(), quote!()),
    };
    let result_place = match (&shape.returns, api_result) {
        (point_shape::Returns::Value(ty), true) if point_shape::wide(ty).is_some() => {
            let wide = point_shape::wide(ty);
            widened.push((
                quote!(let mut __w_result = __args.result.map(|__value| __value as #ty);),
                quote!(__args.result = __w_result.map(|__value| __value as #wide);),
            ));
            quote!(__w_result)
        }
        (_, true) => quote!(__args.result),
        (_, false) => quote!(__result_slot),
    };
    let (unpack, repack): (Vec<_>, Vec<_>) = widened.into_iter().unzip();
    // The raw record, built afresh for each firing from the places the parameters live.
    let raw_record = {
        let ty = shape.raw_record();
        let fields = point_shape::raw_fields(&function.sig)
            .into_iter()
            .map(|field| {
                let (name, place) = (field.name, &places[&field.input]);
                quote!(#name: &mut #place)
            });
        let outcome = match outcome {
            point_shape::RawOutcome::Cancel => quote!(cancel: &mut #cancel,),
            point_shape::RawOutcome::Result(_) => quote!(result: &mut #result_place,),
            point_shape::RawOutcome::None => quote!(),
        };
        quote!(#ty { #(#fields,)* #outcome })
    };
    let snapshot = |at_return: bool| {
        if let Some(plan) = raw_plan.as_ref().and_then(|plan| {
            if at_return {
                plan.tail.as_ref()
            } else {
                Some(&plan.head)
            }
        }) {
            let roots = plan.roots;
            let read = &plan.read;
            let apply = &plan.apply;
            let mut inputs = Vec::new();
            if !at_return && raw_plan.as_ref().is_some_and(|plan| plan.plan.is_some()) {
                // A registration policy projects the receiver, as the first root.
                match shape.receiver {
                    point_shape::Receiver::Shared => inputs.push(quote!(self,)),
                    point_shape::Receiver::Mut => inputs.push(quote!(&mut *self,)),
                    point_shape::Receiver::None => {}
                }
            }
            if !at_return {
                for field in point_shape::raw_fields(&function.sig) {
                    let place = &places[&field.input];
                    inputs.push(quote!(&mut #place,));
                }
            }
            match (&outcome, at_return) {
                (point_shape::RawOutcome::Cancel, false) => inputs.push(quote!(&mut #cancel,)),
                (point_shape::RawOutcome::Result(_), false) => {
                    inputs.push(quote!(&mut #result_place,))
                }
                _ => {}
            }
            // The return phase's one root is the returned slot, whatever the head's outcome was.
            if at_return && matches!(function.sig.output, syn::ReturnType::Type(..)) {
                inputs.push(quote!(&mut __result,));
            }
            if inputs.len() != roots {
                return syn::Error::new_spanned(
                    &function.sig,
                    "staged raw frame does not match the native root tuple",
                )
                .to_compile_error();
            }
            let rows = (roots + 1) as u32;
            // A registration policy rides in the leased state: readers are plain functions.
            let (bind, extra, read, apply) = match raw_plan.as_ref().and_then(|plan| plan.plan) {
                Some(id) => (
                    quote!(let ::std::option::Option::Some(__raw_policy) = __endpoint.plan(#id) else { return; };),
                    quote!(__raw_policy,),
                    quote!({ let __raw_plan = &__raw_state.2; #read }),
                    quote!({ let __raw_plan = &__raw_state.2; #apply }),
                ),
                None => (quote!(), quote!(), quote!(#read), quote!(#apply)),
            };
            return quote! {
                #bind
                let __raw_state = (
                    ::std::cell::RefCell::new((#(#inputs)*)),
                    ::std::cell::RefCell::new(mod_points::locators::Locators::default()),
                    #extra
                );
                __raw_state.1.borrow_mut().intern(None, mod_points::locators::Step::Root(0))
                    .expect("the first raw root fits the locator arena");
                mod_points::snapshot::with_root(&__raw_state, |__root| {
                    let mut __projection = mod_api::wire::Projected::new(
                        |__out: &mut mod_api::wire::Writer| {
                            // SAFETY: creation runs inside the real marked call's encoder, while
                            // the private value lease belongs to this enclosing callback frame.
                            // The generated reader resolves paths afresh and returns bounded rows.
                            let __pages = match unsafe {
                                mod_points::snapshot::PageSource::new(&__root, |__raw_state, __raw_window| #read)
                            } {
                                Ok(__pages) => __pages,
                                Err(_) => {
                                    __out.fail(mod_api::wire::WireError::Invalid("raw native root is unavailable"));
                                    return;
                                }
                            };
                            match __pages.window(mod_api::RowWindow { node: 0, start: 0, rows: #rows, depth: 1 }) {
                                Ok(__rows) => {
                                    __out.put(&__rows);
                                    __out.put(&Some(__pages.clone()));
                                    __out.put(&mod_api::wire::RecordPatch::default());
                                }
                                Err(_) => __out.fail(mod_api::wire::WireError::Invalid("raw native projection refused")),
                            }
                        },
                        |__raw_bytes: &[u8]| -> ::core::result::Result<(), mod_api::wire::WireError> {
                            __root.close();
                            #apply
                        },
                    );
                    __endpoint.forward(__index, &mut __projection);
                });
            };
        }
        if !shape.snapshot_able(&function.sig) {
            return quote!();
        }
        let raw_fields = point_shape::raw_fields(&function.sig);
        let contract = point_shape::raw_shape(&shape, &function.sig, None);
        let count = raw_fields.len() as u32;
        let control_id = count + 1;
        let nodes = count + 2;
        let slots: Vec<_> = (0..raw_fields.len())
            .map(|index| quote::format_ident!("__snapshot_slot_{index}"))
            .collect();
        let positions: Vec<_> = (0..raw_fields.len()).map(syn::Index::from).collect();
        let control_position = syn::Index::from(raw_fields.len());
        let (prepare, control, apply, control_key) = match (&shape.returns, at_return) {
            (point_shape::Returns::Value(ty), true) => (
                quote!(),
                quote!(__result),
                quote!(),
                format!("raw-return-control({})", quote!(#ty)),
            ),
            (point_shape::Returns::Value(ty), false) => (
                quote!(),
                result_place.clone(),
                quote!(),
                format!("raw-result-control({})", quote!(#ty)),
            ),
            _ => (
                quote!(),
                quote!(#cancel),
                quote!(),
                "raw-cancel-control".to_owned(),
            ),
        };
        let roots = raw_fields.iter().map(|field| {
            if at_return {
                let name = &field.name;
                quote!(&mut __args.#name,)
            } else {
                let place = &places[&field.input];
                quote!(&mut #place,)
            }
        });
        let rows = raw_fields.iter().zip(&positions).enumerate().map(|(index, (field, position))| {
            let index = index as u32;
            let ty = &field.ty;
            let ty = quote!(#ty).to_string();
            quote!(__snapshot.value(Some(__root), #index, const { mod_points::snapshot::key(#ty) }, &*__state.#position)?;)
        });
        let writable_control = if at_return {
            contract.return_record_mutable
                && matches!(&shape.returns, point_shape::Returns::Value(_))
        } else {
            contract.outcome_record_mutable == Some(true)
        };
        let fields = if writable_control {
            quote!((mod_api::wire::PatchCell::new(#control_id, &mut **__snapshot_control), ()))
        } else {
            quote!(())
        };
        let fields = raw_fields
            .iter()
            .zip(&contract.args)
            .zip(&slots)
            .enumerate()
            .rev()
            .fold(fields, |tail, (index, ((_, metadata), slot))| {
                if !at_return && metadata.record_mutable {
                    let id = index as u32 + 1;
                    quote!((mod_api::wire::PatchCell::new(#id, &mut **#slot), #tail))
                } else {
                    tail
                }
            });
        quote! {
            #prepare
            {
                let __snapshot_state = ::std::cell::RefCell::new((#(#roots)* &mut #control,));
                mod_points::snapshot::with_root(&__snapshot_state, |__root_handle| {
                    let mut __record = mod_api::wire::Projected::new(
                        |__out: &mut mod_api::wire::Writer| {
                            let __state = __snapshot_state.borrow();
                            let mut __snapshot = mod_points::snapshot::Builder::default();
                            let __rows = (|| -> ::core::result::Result<_, mod_api::wire::WireError> {
                                let __root = __snapshot.node(None, 0, const { mod_points::snapshot::key("raw-call") })?;
                                #(#rows)*
                                __snapshot.value(Some(__root), #count, const { mod_points::snapshot::key(#control_key) }, &*__state.#control_position)?;
                                Ok(__snapshot.finish())
                            })();
                            match __rows {
                                Ok(__rows) => {
                                    __out.put(&__rows);
                                    __out.put(&None::<::std::sync::Arc<dyn mod_api::RowPages>>);
                                    __out.put(&mod_api::wire::RecordPatch::default());
                                }
                                Err(__error) => __out.fail(__error),
                            }
                        },
                        |__bytes: &[u8]| {
                            __root_handle.close();
                            let mut __state = __snapshot_state.borrow_mut();
                            let (#(#slots,)* __snapshot_control,) = &mut *__state;
                            mod_api::wire::Record::apply(&mut mod_api::wire::PatchFields::new(#fields, #nodes), __bytes)
                        },
                    );
                    __endpoint.forward(__index, &mut __record);
                });
            }
            #apply
        }
    };
    let head_snapshot = snapshot(false);
    let return_snapshot = snapshot(true);
    let fire = |variant: &syn::Ident, method: &syn::Ident, api: bool| {
        let name = method.to_string();
        let api = api.then(|| {
            quote! {
                mod_points::dispatch::#method(#host &mut __args);
            }
        });
        let raw = shape.raw.then(|| {
            let listener = shape.raw_listener();
            quote! {
                #(#unpack)*
                mod_points::__private::fire_raw(
                    mod_points::PointId::#variant,
                    #name,
                    &mut |__kind| match __kind {
                        mod_points::__private::RawKind::Typed(__function) => {
                            // SAFETY: registration guarantees this point's listener type.
                            unsafe {
                                ::std::mem::transmute::<*const (), #listener>(__function)(
                                    #receiver &mut #raw_record,
                                )
                            }
                        }
                        mod_points::__private::RawKind::Snapshot(__endpoint, __index) => { #head_snapshot }
                        mod_points::__private::RawKind::Erased(__function) => {
                            // SAFETY: this slot supplies its exact receiver and raw record types.
                            unsafe {
                                __function(
                                    mod_points::PointId::#variant as usize,
                                    #erased_receiver,
                                    &mut #raw_record as *mut _ as *mut (),
                                )
                            }
                        }
                    },
                );
                #(#repack)*
            }
        });
        quote!(#api #raw)
    };
    let listened = |variant: &syn::Ident| quote!(mod_points::__private::POINTS[mod_points::PointId::#variant as usize].load(::std::sync::atomic::Ordering::Acquire));
    let (head_variant, head_method) = (
        shape.variant(),
        syn::Ident::new(&shape.name, proc_macro2::Span::call_site()),
    );
    let head_listened = listened(&head_variant);
    let head_fire = fire(&head_variant, &head_method, !shape.raw_only);
    let result_slot = match &outcome {
        point_shape::RawOutcome::Result(_) if shape.raw && !api_result => {
            quote!(let mut __result_slot = None;)
        }
        _ => quote!(),
    };
    let stop = match (&shape.returns, &outcome) {
        (point_shape::Returns::Unit, _) => quote!(if #cancel {
            return;
        }),
        (point_shape::Returns::UnitResult, _) => quote!(if #cancel {
            return Ok(());
        }),
        (point_shape::Returns::Value(_), _) if api_result => {
            quote!(if let Some(__result) = __args.result.take() {
                return __result #narrow;
            })
        }
        (_, point_shape::RawOutcome::Result(_)) if shape.raw => {
            quote!(if let Some(__result) = __result_slot.take() {
                return __result;
            })
        }
        _ => quote!(),
    };
    let returned = if shape.at_return() {
        let (variant, method) = (
            shape.return_variant(),
            format_ident!("{}_return", shape.name),
        );
        let return_listened = listened(&variant);
        let name = method.to_string();
        let api = match (&shape.returns, shape.has_return() && !shape.raw_only) {
            (_, false) => quote!(),
            (point_shape::Returns::Value(_), true) => quote! {
                __args.result = Some(__result #widen);
                mod_points::dispatch::#method(#host &mut __args);
                let __result = __args.result.map_or(__result, |__value| __value #narrow);
            },
            (_, true) => quote! {
                mod_points::dispatch::#method(#host &mut __args);
            },
        };
        let raw = shape.raw.then(|| {
            let listener = shape.raw_return_listener();
            let (receiver, erased_receiver) = if shape.borrowed_return {
                (quote!(), quote!(::std::ptr::null_mut()))
            } else {
                (receiver.clone(), erased_receiver.clone())
            };
            let value = match &function.sig.output {
                syn::ReturnType::Type(..) => quote!(&mut __result),
                syn::ReturnType::Default => quote!(),
            };
            quote! {
                #[allow(unused_mut)]
                let mut __result = __result;
                mod_points::__private::fire_raw(
                    mod_points::PointId::#variant,
                    #name,
                    &mut |__kind| match __kind {
                        mod_points::__private::RawKind::Typed(__function) => {
                            // SAFETY: registration guarantees this place's return listener type.
                            unsafe {
                                ::std::mem::transmute::<*const (), #listener>(__function)(
                                    #receiver #value
                                )
                            }
                        }
                        mod_points::__private::RawKind::Snapshot(__endpoint, __index) => { #return_snapshot }
                        mod_points::__private::RawKind::Erased(__function) => {
                            // SAFETY: this slot supplies its exact receiver and return types.
                            unsafe {
                                __function(
                                    mod_points::PointId::#variant as usize,
                                    #erased_receiver,
                                    &mut __result as *mut _ as *mut (),
                                )
                            }
                        }
                    },
                );
            }
        });
        quote! {
            if #return_listened {
                #api
                #raw
                return __result;
            }
        }
    } else {
        quote!()
    };
    let arguments = if shape.raw_only {
        quote!(let mut __cancel = false;)
    } else {
        quote!(let mut __args = mod_points::#record::__new(#(#passed),*);)
    };
    outer.block = syn::parse_quote!({
        #[allow(unused_mut)]
        #arguments
        #result_slot
        if #head_listened {
            #head_fire
            #stop
        }
        let __result = #callee #turbofish(#(#call),*);
        #returned
        __result
    });
    // The outer function keeps the function's own `#[inline]`: a small accessor in a lower crate
    // (`Chunk::get`) stays inlinable into its callers, slot check and all.
    function.sig.ident = inner;
    function.vis = syn::Visibility::Inherited;
    function
        .attrs
        .retain(|attr| !attr.path().is_ident("doc") && !attr.path().is_ident("inline"));
    function.attrs.push(syn::parse_quote!(#[inline(always)]));
    quote!(#outer #function).into()
}

/// Derive subscriptions for internal test fixtures that construct their own mods.
#[doc(hidden)]
#[proc_macro_attribute]
pub fn __subscriptions(_attribute: TokenStream, input: TokenStream) -> TokenStream {
    expand(parse_macro_input!(input as ItemImpl), false)
}

/// `if_installed!(host, other_mod, { other_mod::call(); })`: runs the block only where the optional
/// dependency `other_mod` is installed. A native mod is built with the Cargo feature `other_mod`
/// only when it is; a wasm mod is built with it and asks the game at the call, since an absent
/// library's functions trap. One guard for both builds.
#[proc_macro]
pub fn if_installed(input: TokenStream) -> TokenStream {
    let IfInstalled { host, id, block } = parse_macro_input!(input as IfInstalled);
    let name = id.to_string();
    quote! {
        {
            // A `let` takes the attribute where an expression statement can't.
            #[cfg(feature = #name)]
            let () = if ::mod_api::prelude::LevelsHost::is_loaded(&*#host, #name) #block;
            #[cfg(not(feature = #name))]
            let _ = &#host;
        }
    }
    .into()
}

struct IfInstalled {
    host: syn::Expr,
    id: syn::Ident,
    block: syn::Block,
}

impl syn::parse::Parse for IfInstalled {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let host = input.parse()?;
        input.parse::<syn::Token![,]>()?;
        let id = input.parse()?;
        input.parse::<syn::Token![,]>()?;
        Ok(Self {
            host,
            id,
            block: input.parse()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verbatim_items_are_uninspectable() {
        let item = ImplItem::Verbatim(quote!(future syntax));
        assert!(uninspectable(&item));
    }
}
