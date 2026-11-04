//! The shape of an injection point (#589): what a `#[point]` function hands its listeners. Shared
//! by the `#[point]` attribute (the check at the function's head) and the mod API generator (the
//! slot statics and `points.json`, which `#[path]`s this file in), so the two always agree.
// Each of the two uses its own part of the shape.
#![allow(dead_code)]
use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{FnArg, Pat, Type};

/// Where a point's function runs, as a hook's `side`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Server,
    Client,
    Both,
}

impl Side {
    pub fn name(self) -> &'static str {
        match self {
            Self::Server => "server",
            Self::Client => "client",
            Self::Both => "both",
        }
    }
}

/// One argument a point passes: the parameter's binding and type.
pub struct Arg {
    pub name: syn::Ident,
    pub ty: Type,
}

/// What a point's listeners may do with the function's return value.
pub enum Returns {
    /// `()`: a head listener may `cancel`, and the function returns at once.
    Unit,
    /// `Result<()>`: the same, returning `Ok(())`.
    UnitResult,
    /// An API-typed value: a head listener may set `result`, which the function returns at once;
    /// a return listener sees the value in `result` and may change it.
    Value(Type),
    /// A value the API can't carry: the listeners neither stop the function nor see its value.
    Opaque,
}

/// A `#[point]` function's point.
pub struct Shape {
    /// Unique among the game's points: the function's name, or the attribute's `name`.
    pub name: String,
    pub side: Side,
    /// The function takes `&mut self`: its listeners get the side's writing host.
    pub mutable: bool,
    /// Its listeners run per block, per entity or per frame: the docs and build log say so.
    pub hot: bool,
    /// The parameters with API types, in order. The others aren't passed: an internal type
    /// would become API.
    pub args: Vec<Arg>,
    pub returns: Returns,
}

/// The point of the function `sig`, from `#[point(side = "…", name = "…", hot)]`'s arguments.
pub fn shape(attribute: TokenStream, sig: &syn::Signature) -> syn::Result<Shape> {
    let (mut name, mut side, mut hot) = (sig.ident.to_string(), None, false);
    let parser = syn::meta::parser(|meta| {
        if meta.path.is_ident("hot") {
            hot = true;
        } else if meta.path.is_ident("name") {
            name = meta.value()?.parse::<syn::LitStr>()?.value();
        } else if meta.path.is_ident("side") {
            let value = meta.value()?.parse::<syn::LitStr>()?;
            side = Some(match value.value().as_str() {
                "server" => Side::Server,
                "client" => Side::Client,
                "both" => Side::Both,
                _ => {
                    return Err(syn::Error::new(
                        value.span(),
                        "side is server, client or both",
                    ));
                }
            });
        } else {
            return Err(meta.error("a point takes side = \"…\", name = \"…\" and hot"));
        }
        Ok(())
    });
    syn::parse::Parser::parse2(parser, attribute)?;
    let side = side.ok_or_else(|| {
        syn::Error::new(
            sig.ident.span(),
            "a point needs side = \"server\", \"client\" or \"both\"",
        )
    })?;
    let mutable = match sig.inputs.first() {
        Some(FnArg::Receiver(receiver)) if receiver.reference.is_some() => {
            receiver.mutability.is_some()
        }
        _ => {
            return Err(syn::Error::new(
                sig.ident.span(),
                "a point is a `&self` or `&mut self` method of `Game`",
            ));
        }
    };
    let args = sig
        .inputs
        .iter()
        .filter_map(|input| match input {
            FnArg::Typed(typed) => match &*typed.pat {
                Pat::Ident(pat)
                    if pat.by_ref.is_none() && pat.subpat.is_none() && api_typed(&typed.ty) =>
                {
                    Some(Arg {
                        name: pat.ident.clone(),
                        ty: (*typed.ty).clone(),
                    })
                }
                _ => None,
            },
            FnArg::Receiver(_) => None,
        })
        .collect::<Vec<_>>();
    if let Some(arg) = args
        .iter()
        .find(|arg| arg.name == "cancel" || arg.name == "result")
    {
        return Err(syn::Error::new(
            arg.name.span(),
            "a point's record has its own `cancel` and `result`: rename the parameter",
        ));
    }
    let returns = match &sig.output {
        syn::ReturnType::Default => Returns::Unit,
        syn::ReturnType::Type(_, ty) if unit_result(ty) => Returns::UnitResult,
        syn::ReturnType::Type(_, ty) if scalar(ty) => Returns::Value((**ty).clone()),
        syn::ReturnType::Type(..) => Returns::Opaque,
    };
    Ok(Shape {
        name,
        side,
        mutable,
        hot,
        args,
        returns,
    })
}

/// `Result<()>` (the game's `anyhow` alias) or `Result<(), E>`.
fn unit_result(ty: &Type) -> bool {
    let Type::Path(path) = ty else { return false };
    let Some(last) = path.path.segments.last() else {
        return false;
    };
    let syn::PathArguments::AngleBracketed(generics) = &last.arguments else {
        return false;
    };
    last.ident == "Result"
        && matches!(generics.args.first(), Some(syn::GenericArgument::Type(Type::Tuple(unit))) if unit.elems.is_empty())
}

/// A scalar the API carries by value.
fn scalar(ty: &Type) -> bool {
    matches!(ty, Type::Path(_)) && api_typed(ty)
}

/// `tick_return` as `TickReturn`.
fn camel(name: &str) -> String {
    name.split('_')
        .map(|word| {
            let mut chars = word.chars();
            chars
                .next()
                .map(|first| first.to_ascii_uppercase().to_string() + chars.as_str())
                .unwrap_or_default()
        })
        .collect()
}

/// Whether the API can carry `ty` to a listener: a scalar, or `&str`.
fn api_typed(ty: &Type) -> bool {
    const SCALARS: [&str; 15] = [
        "bool", "char", "f32", "f64", "i8", "i16", "i32", "i64", "i128", "isize", "u8", "u16",
        "u32", "u64", "usize",
    ];
    match ty {
        Type::Path(path) => {
            path.qself.is_none() && SCALARS.iter().any(|scalar| path.path.is_ident(scalar))
        }
        Type::Reference(reference) => {
            reference.mutability.is_none()
                && matches!(&*reference.elem, Type::Path(path) if path.path.is_ident("str"))
        }
        _ => false,
    }
}

impl Shape {
    /// The slot's static in the game's generated `points` module.
    pub fn slot(&self) -> syn::Ident {
        syn::Ident::new(&self.name.to_uppercase(), Span::call_site())
    }

    /// The slot its return listeners wait in.
    pub fn return_slot(&self) -> syn::Ident {
        syn::Ident::new(&format!("{}_RETURN", self.slot()), Span::call_site())
    }

    /// The record its listeners get, in `mod_api::points`: `TickArgs` for `tick`.
    pub fn record(&self) -> syn::Ident {
        syn::Ident::new(&format!("{}Args", camel(&self.name)), Span::call_site())
    }

    /// Whether the record borrows (a `&str` parameter).
    fn borrows(&self) -> bool {
        self.args
            .iter()
            .any(|arg| matches!(arg.ty, Type::Reference(_)))
    }

    /// The record's declaration in `mod_api::points`, documented from the function's `target`.
    pub fn record_item(&self, target: &str) -> TokenStream {
        let record = self.record();
        let lifetime = if self.borrows() {
            quote!(<'a>)
        } else {
            quote!()
        };
        let names: Vec<_> = self.args.iter().map(|arg| &arg.name).collect();
        let types: Vec<_> = self
            .args
            .iter()
            .map(|arg| match &arg.ty {
                Type::Reference(_) => quote!(&'a str),
                ty => quote!(#ty),
            })
            .collect();
        let field_docs = names.iter().map(|name| {
            format!(
                "`{name}`: a head listener may change it, and the function runs with the change."
            )
        });
        let (extra, extra_init) = match &self.returns {
            Returns::Unit | Returns::UnitResult => (
                quote! {
                    /// Set by a head listener: the function returns at once, skipping its body.
                    pub cancel: bool,
                },
                quote!(cancel: false,),
            ),
            Returns::Value(ty) => (
                quote! {
                    /// Set by a head listener: the function returns this at once, skipping its body.
                    /// Return listeners find the value it returns here, and may change it.
                    pub result: Option<#ty>,
                },
                quote!(result: None,),
            ),
            Returns::Opaque => (quote!(), quote!()),
        };
        let doc = format!(
            "The `{}` point's record: what its listeners get from `{target}`, a slot filled at load.",
            self.name
        );
        quote! {
            #[doc = #doc]
            #[non_exhaustive]
            pub struct #record #lifetime {
                #( #[doc = #field_docs] pub #names: #types, )*
                #extra
            }
            impl #lifetime #record #lifetime {
                /// The record of a call, before its listeners run (the game's side of the point).
                #[doc(hidden)]
                pub fn __new(#(#names: #types),*) -> Self {
                    Self { #(#names,)* #extra_init }
                }
            }
        }
    }

    /// The `Points` methods for this point (its head, then its return), their `PointId` variants and
    /// the dispatchers that run every subscribed mod's method: generated into `mod_api::points`.
    pub fn api_items(&self, target: &str) -> (TokenStream, TokenStream, TokenStream) {
        let record = self.record();
        let record = if self.borrows() {
            quote!(#record<'_>)
        } else {
            quote!(#record)
        };
        let host = match (self.mutable, self.side) {
            (false, _) => quote!(&dyn crate::Host),
            (true, Side::Server) => quote!(&mut dyn crate::ServerHost),
            (true, Side::Client) => quote!(&mut dyn crate::ClientHost),
            (true, Side::Both) => quote!(crate::HostSide<'_>),
        };
        let (binding, reborrow) = match (self.mutable, self.side) {
            (false, _) => (quote!(host), quote!(host)),
            (true, Side::Both) => (quote!(mut host), quote!(host.reborrow())),
            (true, _) => (quote!(host), quote!(&mut *host)),
        };
        let (mut methods, mut variants, mut dispatch) = (quote!(), quote!(), quote!());
        for (method, place) in [
            (
                self.name.clone(),
                "head: before its body runs, with the arguments it was called with",
            ),
            (
                format!("{}_return", self.name),
                "return: after its body, with the value it returns",
            ),
        ] {
            let method = syn::Ident::new(&method, Span::call_site());
            let variant = syn::Ident::new(&camel(&method.to_string()), Span::call_site());
            let dispatcher = syn::Ident::new(&format!("dispatch_{method}"), Span::call_site());
            let doc = format!("`{target}`'s {place}.");
            let name = method.to_string();
            methods.extend(quote! {
                #[doc = #doc]
                fn #method(&self, host: #host, args: &mut #record) {
                    let _ = (host, args);
                }
            });
            variants.extend(quote!(#variant,));
            dispatch.extend(quote! {
                #[doc(hidden)]
                pub fn #dispatcher(#binding: #host, args: &mut #record) {
                    for points in super::entered_points(PointId::#variant, #name) {
                        points.#method(#reborrow, args);
                    }
                }
                #[doc(hidden)]
                #[allow(non_upper_case_globals)]
                pub const #method: Option<PointId> = Some(PointId::#variant);
            });
        }
        (methods, variants, dispatch)
    }

    /// The listener's function type.
    pub fn listener(&self) -> TokenStream {
        let record = self.record();
        let record = if self.borrows() {
            quote!(::mod_api::points::#record<'_>)
        } else {
            quote!(::mod_api::points::#record)
        };
        let host = match (self.mutable, self.side) {
            (false, _) => quote!(&dyn ::mod_api::Host),
            (true, Side::Server) => quote!(&mut dyn ::mod_api::ServerHost),
            (true, Side::Client) => quote!(&mut dyn ::mod_api::ClientHost),
            (true, Side::Both) => quote!(::mod_api::HostSide<'_>),
        };
        quote!(fn(#host, &mut #record))
    }
}
