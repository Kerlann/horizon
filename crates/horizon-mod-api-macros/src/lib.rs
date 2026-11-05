//! Derive hook subscriptions from a trait implementation.
use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{
    ImplItem, ImplItemFn, ItemImpl, Meta, Token, parse::Parser, parse_macro_input,
    punctuated::Punctuated,
};

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

fn uninspectable(item: &ImplItem) -> bool {
    matches!(item, ImplItem::Macro(_) | ImplItem::Verbatim(_))
}

fn expand(mut implementation: ItemImpl, export: bool) -> TokenStream {
    let Some((_, trait_path, _)) = &implementation.trait_ else {
        return syn::Error::new_spanned(
            &implementation,
            "expected an impl of Hooks or RenderHooks",
        )
        .into_compile_error()
        .into();
    };
    let name = &trait_path.segments.last().unwrap().ident;
    let (render, part) = match name.to_string().as_str() {
        "Hooks" => (false, None),
        "RenderHooks" => (true, None),
        "Points" => return expand_points(implementation),
        "BlockPart" => (false, Some("block_subscribed")),
        "BlockEntityPart" => (false, Some("block_entity_subscribed")),
        "ItemPart" => (false, Some("item_subscribed")),
        "MenuPart" => (false, Some("menu_subscribed")),
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
    for item in &implementation.items {
        if let ImplItem::Fn(method) = item {
            methods.push(&method.sig.ident);
            cfgs.push(cfg_attributes(method));
        }
    }
    let id = if render {
        format_ident!("RenderHookId")
    } else {
        format_ident!("HookId")
    };
    let ids = if render {
        format_ident!("render_hook_ids")
    } else {
        format_ident!("hook_ids")
    };
    let subscribed_name = format_ident!("{}", part.unwrap_or("subscribed"));
    let subscribed = syn::parse_quote! {
        fn #subscribed_name(&self) -> &'static [Option<mod_api::#id>] {
            &[#(#(#cfgs)* mod_api::#ids::#methods),*]
        }
    };
    implementation.items.push(subscribed);
    let export = if export && !render && part.is_none() {
        let ty = &implementation.self_ty;
        quote! {
            pub fn hooks() -> Box<dyn mod_api::Hooks> { Box::new(<#ty as Default>::default()) }
            /// The mod's hooks for the guest glue in `mod_api::wasm`, when built to `mod.wasm`.
            #[cfg(target_arch = "wasm32")]
            #[unsafe(no_mangle)]
            fn __horizon_mod_hooks() -> Box<dyn mod_api::Hooks> { hooks() }
        }
    } else {
        quote!()
    };
    quote!(#implementation #export).into()
}

/// `#[horizon_mod]` on `impl mod_points::Points`: its subscriptions, `__HORIZON_POINTS`, the methods
/// it overrides, which the launcher's generated `main` checks each point injection against, and
/// `__horizon_points`, which the generated `main` registers the mod's listeners with.
fn expand_points(mut implementation: ItemImpl) -> TokenStream {
    let mut methods = Vec::new();
    let mut cfgs = Vec::new();
    let mut records = Vec::new();
    let mut hostless = Vec::new();
    for item in &implementation.items {
        if let ImplItem::Fn(method) = item {
            methods.push(method.sig.ident.clone());
            cfgs.push(cfg_attributes(method));
            // The record a listener takes, `args: &mut XArgs`: what crosses to it in a wasm mod.
            // The last parameter: a point below the game takes no host before it.
            hostless.push(method.sig.inputs.len() == 2);
            let record = match method.sig.inputs.last() {
                Some(syn::FnArg::Typed(typed)) => match &*typed.ty {
                    syn::Type::Reference(reference) => Some((*reference.elem).clone()),
                    _ => None,
                },
                _ => None,
            };
            let Some(record) = record else {
                return syn::Error::new_spanned(
                    &method.sig,
                    "a point listener takes `&self`, its host if it has one, and `args: &mut <Point>Args`",
                )
                .into_compile_error()
                .into();
            };
            records.push(record);
        }
    }
    let guest = wasm_listeners(
        &implementation.self_ty,
        &methods,
        &cfgs,
        &records,
        &hostless,
    );
    implementation.items.push(syn::parse_quote! {
        fn subscribed(&self) -> &'static [Option<mod_points::PointId>] {
            &[#(#(#cfgs)* mod_points::ids::#methods),*]
        }
    });
    let names = methods.iter().map(|method| method.to_string());
    let ty = &implementation.self_ty;
    quote! {
        #implementation
        /// The injection-point methods this mod overrides (its `impl Points`).
        #[doc(hidden)]
        pub const __HORIZON_POINTS: &[&str] = &[#(#(#cfgs)* #names),*];
        /// The mod's `impl Points`: its installed `Hooks`, the same value, seen as its listeners.
        /// The launcher's generated `main` registers it.
        #[doc(hidden)]
        pub fn __horizon_points(hooks: &'static dyn mod_api::Hooks) -> &'static dyn mod_points::Points {
            let hooks: &'static dyn ::std::any::Any = hooks;
            hooks
                .downcast_ref::<#ty>()
                .expect("a mod's Points and Hooks are one type")
        }
        #guest
    }
    .into()
}

/// A wasm mod's listeners at points: the `listeners` export (`mod_points::wire`), each place the
/// mod's `impl Points` overrides at its index in `subscribed`, with its record's shape, which the
/// game checks at load. Only the methods the mod overrides are bound, so the mod carries none of
/// the other points' records.
fn wasm_listeners(
    ty: &syn::Type,
    methods: &[syn::Ident],
    cfgs: &[Vec<syn::Attribute>],
    records: &[syn::Type],
    hostless: &[bool],
) -> proc_macro2::TokenStream {
    let ty_path = ty;
    let calls: Vec<_> = methods
        .iter()
        .zip(hostless)
        .map(|(method, &hostless)| match hostless {
            true => quote!(|_: (), args| <#ty_path as mod_points::Points>::#method(this, args)),
            false => {
                quote!(|host, args| <#ty_path as mod_points::Points>::#method(this, host, args))
            }
        })
        .collect();
    let names = methods.iter().map(|method| method.to_string());
    let records = records.iter().map(|record| {
        // The record's own lifetime, if any, elided: `<XArgs<'_> as Lift<'_>>`.
        let mut record = record.clone();
        if let syn::Type::Path(path) = &mut record
            && let Some(last) = path.path.segments.last_mut()
            && let syn::PathArguments::AngleBracketed(arguments) = &mut last.arguments
        {
            for argument in &mut arguments.args {
                if let syn::GenericArgument::Lifetime(lifetime) = argument {
                    *lifetime = syn::Lifetime::new("'_", lifetime.span());
                }
            }
        }
        record
    });
    quote! {
        #[cfg(target_arch = "wasm32")]
        const _: () = {
            struct HorizonListeners;
            impl mod_points::__private::wire::Guest for HorizonListeners {
                fn subscribed() -> ::std::vec::Vec<(::std::string::String, ::std::string::String)> {
                    ::std::vec![#(#(#cfgs)* (
                        ::std::string::String::from(#names),
                        ::std::string::String::from(
                            <#records as mod_points::__private::wire::Lift<'_>>::SHAPE,
                        ),
                    )),*]
                }
                #[allow(unused_assignments)]
                fn fire(
                    place: u32,
                    host: mod_points::__private::wire::HostKind,
                    fields: ::std::vec::Vec<mod_points::__private::wire::Value>,
                ) -> ::std::vec::Vec<mod_points::__private::wire::Value> {
                    let this = mod_points::__private::wire::listener::<#ty>();
                    let mut index = 0u32;
                    #(#(#cfgs)* {
                        if place == index {
                            return mod_points::__private::wire::fire(host, &fields, #calls);
                        }
                        index += 1;
                    })*
                    panic!("the game fired a place this mod doesn't listen at")
                }
            }
            mod_points::__private::wire::bindings::export_listeners!(
                HorizonListeners with_types_in mod_points::__private::wire::bindings
            );
        };
    }
}

/// Mark a trait mod or its renderer and derive its hook subscriptions.
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

/// `#[point(side = "server" | "client" | "both", name = "…", hot)]` on a `Game` method: an
/// injection point (#589), a slot the launcher fills at load for the mods whose `impl Points` listens
/// there. The staging pass that builds the prebuilt game (and the launcher's source builds) puts it
/// on every method [`point_shape::eligible`] admits; the source tree carries none. Its head checks
/// the slot (one atomic load and a branch while nothing listens) and hands each listener the side's
/// host and the parameters with API types, which it may change; its return does the same with the
/// value returned, unless that value borrows the game.
#[proc_macro_attribute]
pub fn point(attribute: TokenStream, input: TokenStream) -> TokenStream {
    let mut function = parse_macro_input!(input as ImplItemFn);
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
    // returns; the outer function takes each parameter by a plain name.
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
    for (i, input) in outer.sig.inputs.iter_mut().enumerate() {
        let syn::FnArg::Typed(typed) = input else {
            continue;
        };
        let original = match &*typed.pat {
            syn::Pat::Ident(pat) => Some(pat.ident.clone()),
            _ => None,
        };
        let plain = format_ident!("__p{i}");
        *typed.pat = syn::parse_quote!(#plain);
        match original.and_then(|name| shape.args.iter().find(|arg| arg.name == name)) {
            Some(arg) => {
                let name = &arg.name;
                passed.push(plain);
                call.push(match (&arg.ty, point_shape::wide(&arg.ty)) {
                    (syn::Type::Reference(_), _) => quote!(&*__args.#name),
                    // A native width crosses as 64 bits: back to the parameter's own type.
                    (ty, Some(_)) => quote!(__args.#name as #ty),
                    (_, None) => quote!(__args.#name),
                });
            }
            None => call.push(quote!(#plain)),
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
    // What the listeners get for `self`: the shape's host.
    let host = match (shape.mutable, shape.side) {
        (false, _) => quote!(self),
        (true, point_shape::Side::Both) => quote!(crate::game::points::host_side(&mut *self)),
        (true, _) => quote!(&mut *self),
    };
    let record = shape.record();
    let fire = |variant: &syn::Ident, method: &syn::Ident| {
        let name = method.to_string();
        quote! {
            crate::game::points::fire(
                crate::game::points::PointId::#variant,
                #name,
                &mut |__points| __points.#method(#host, &mut __args),
            )
        }
    };
    let listened = |variant: &syn::Ident| quote!(crate::game::points::POINTS[crate::game::points::PointId::#variant as usize].load(::std::sync::atomic::Ordering::Acquire));
    let (head_variant, head_method) = (
        shape.variant(),
        syn::Ident::new(&shape.name, proc_macro2::Span::call_site()),
    );
    let head_listened = listened(&head_variant);
    let head_fire = fire(&head_variant, &head_method);
    // A native-width value crosses the record as 64 bits.
    let (widen, narrow) = match &shape.returns {
        point_shape::Returns::Value(ty) => match point_shape::wide(ty) {
            Some(wide) => (quote!(as #wide), quote!(as #ty)),
            None => (quote!(), quote!()),
        },
        _ => (quote!(), quote!()),
    };
    let stop = match &shape.returns {
        point_shape::Returns::Unit => quote!(if __args.cancel {
            return;
        }),
        point_shape::Returns::UnitResult => quote!(if __args.cancel {
            return Ok(());
        }),
        point_shape::Returns::Value(_) => quote!(if let Some(__result) = __args.result.take() {
            return __result #narrow;
        }),
        point_shape::Returns::Opaque => quote!(),
    };
    let returned = if shape.has_return() {
        let (variant, method) = (
            shape.return_variant(),
            format_ident!("{}_return", shape.name),
        );
        let return_listened = listened(&variant);
        let return_fire = fire(&variant, &method);
        let body = match &shape.returns {
            point_shape::Returns::Value(_) => quote! {
                __args.result = Some(__result #widen);
                #return_fire;
                return __args.result.map_or(__result, |__value| __value #narrow);
            },
            _ => quote!(#return_fire;),
        };
        quote! {
            if #return_listened {
                #body
            }
        }
    } else {
        quote!()
    };
    outer.block = syn::parse_quote!({
        let mut __args = crate::game::points::#record::__new(#(#passed),*);
        if #head_listened {
            #head_fire;
            #stop
        }
        let __result = self.#inner #turbofish(#(#call),*);
        #returned
        __result
    });
    outer.attrs.retain(|attr| !attr.path().is_ident("inline"));
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
            let () = if ::mod_api::Host::is_loaded(&*#host, #name) #block;
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

/// `#[wasm(via = Form)]` on a `mod_api` type, or on the `pub use` that exports another crate's:
/// the type crosses to wasm mods as `Form`, through `From<&Type> for Form` and
/// `TryFrom<Form> for Type`, which live with the type. The bindings generator reads the
/// attribute; this checks the two conversions exist.
#[proc_macro_attribute]
pub fn wasm(attribute: TokenStream, input: TokenStream) -> TokenStream {
    let item = parse_macro_input!(input as syn::Item);
    let via = match syn::parse::<WasmVia>(attribute) {
        Ok(via) => via.form,
        Err(e) => return e.to_compile_error().into(),
    };
    let ty = match &item {
        syn::Item::Struct(item) => item.ident.clone(),
        syn::Item::Enum(item) => item.ident.clone(),
        syn::Item::Use(item) => match last_use_name(&item.tree) {
            Some(name) => name,
            None => {
                return syn::Error::new_spanned(item, "#[wasm] exports one type")
                    .to_compile_error()
                    .into();
            }
        },
        item => {
            return syn::Error::new_spanned(item, "#[wasm] goes on a type or its `pub use`")
                .to_compile_error()
                .into();
        }
    };
    quote! {
        #item
        const _: () = {
            fn crosses_as_its_form()
            where
                #via: for<'a> ::core::convert::From<&'a #ty>,
                #ty: ::core::convert::TryFrom<#via>,
            {
            }
        };
    }
    .into()
}

/// `via = Form`.
struct WasmVia {
    form: syn::Type,
}

impl syn::parse::Parse for WasmVia {
    fn parse(input: syn::parse::ParseStream<'_>) -> syn::Result<Self> {
        let key: syn::Ident = input.parse()?;
        if key != "via" {
            return Err(syn::Error::new(key.span(), "expected `via = Form`"));
        }
        input.parse::<syn::Token![=]>()?;
        Ok(Self {
            form: input.parse()?,
        })
    }
}

/// The name a `use` tree of one path brings in.
fn last_use_name(tree: &syn::UseTree) -> Option<syn::Ident> {
    match tree {
        syn::UseTree::Path(path) => last_use_name(&path.tree),
        syn::UseTree::Name(name) => Some(name.ident.clone()),
        syn::UseTree::Rename(rename) => Some(rename.rename.clone()),
        _ => None,
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
