//! Generate ordinary Wire codec closures for a native callable source.
use crate::{
    crossing::{Rule, Shape},
    native_value,
};
use proc_macro2::TokenStream;
use quote::quote;
use syn::Type;

/// Expressions accepted by `FunctionSource::new`: decode an argument tuple and encode its
/// result. The source checks for trailing data before invoking the native callable. Unsupported
/// non-owned or inaccessible shapes are reported by the same codec rules as native values.
pub struct CallableCodecs {
    /// Capture-free argument decoder expression.
    pub decode: TokenStream,
    /// Capture-free result encoder expression.
    pub encode: TokenStream,
}

/// Generate codecs for an ordered list of native parameters and one native result.
pub fn codecs(rule: &Rule, arguments: &[Shape], result: &Shape) -> Result<CallableCodecs, String> {
    let args = native_value::staged_codec(rule, &Shape::Tuple(arguments.to_vec()))?;
    let result = native_value::staged_codec(rule, result)?;
    let decode = args.decode;
    let encode = result.encode;
    Ok(CallableCodecs {
        decode: quote!(|input: &mut mod_api::wire::Reader<'_>| -> ::core::result::Result<_, mod_api::wire::WireError> { Ok(#decode) }),
        encode: quote!(|value, out: &mut mod_api::wire::Writer| { #encode }),
    })
}

/// The signature of a callback argument: `&dyn Fn(A..) -> R`, `&mut dyn FnMut(A..) -> R`, or
/// `fn(A..) -> R`, with any lifetimes. `None` for every other type.
fn signature(ty: &Type) -> Option<(Vec<Type>, Type)> {
    fn tuple(
        inputs: impl Iterator<Item = Type>,
        output: &syn::ReturnType,
    ) -> Option<(Vec<Type>, Type)> {
        let output = match output {
            syn::ReturnType::Default => syn::parse_quote!(()),
            syn::ReturnType::Type(_, ty) => (**ty).clone(),
        };
        Some((inputs.collect(), output))
    }
    match ty {
        Type::Paren(inner) => signature(&inner.elem),
        Type::Reference(reference) => signature(&reference.elem),
        Type::BareFn(function) => tuple(
            function.inputs.iter().map(|input| input.ty.clone()),
            &function.output,
        ),
        Type::TraitObject(object) => object.bounds.iter().find_map(|bound| {
            let syn::TypeParamBound::Trait(bound) = bound else {
                return None;
            };
            let last = bound.path.segments.last()?;
            let syn::PathArguments::Parenthesized(call) = &last.arguments else {
                return None;
            };
            matches!(last.ident.to_string().as_str(), "Fn" | "FnMut" | "FnOnce")
                .then(|| tuple(call.inputs.iter().cloned(), &call.output))?
        }),
        _ => None,
    }
}

/// Whether `ty` is a callback argument (a closure or function pointer).
pub fn is_callback(ty: &Type) -> bool {
    signature(ty).is_some()
}

/// How the callee takes one decoded argument.
#[derive(PartialEq)]
enum Lend {
    Value,
    Shared,
    /// By `&mut`: the decoded value is lent exclusively and its final state returns beside the
    /// result, so the caller sees the callee's writes.
    Exclusive,
}

/// One decoded argument: its owned type, and how the callee takes it.
fn argument(ty: &Type) -> Result<(Type, Lend), String> {
    match ty {
        Type::Reference(reference) => {
            let str_like =
                matches!(&*reference.elem, Type::Path(path) if path.path.is_ident("str"));
            // A shared `&Path` crosses as its text, owned as a `PathBuf`.
            if reference.mutability.is_none()
                && matches!(&*reference.elem, Type::Path(path)
                    if path.path.segments.last().is_some_and(|s| s.ident == "Path"))
            {
                return Ok((syn::parse_quote!(std::path::PathBuf), Lend::Shared));
            }
            if reference.mutability.is_some() {
                if str_like || matches!(&*reference.elem, Type::Slice(_) | Type::TraitObject(_)) {
                    return Err(format!(
                        "`{}` is an unsized mutable reference: it has no owned value to write back",
                        quote!(#ty)
                    ));
                }
                return Ok(((*reference.elem).clone(), Lend::Exclusive));
            }
            Ok((
                if str_like {
                    syn::parse_quote!(String)
                } else {
                    (*reference.elem).clone()
                },
                Lend::Shared,
            ))
        }
        ty => Ok((ty.clone(), Lend::Value)),
    }
}

/// The body of an invocation closure `|__roots, __bytes| -> Result<Vec<u8>, RowError>` for a
/// callback argument, decoding the argument tuple, calling `callee` (a place expression of the
/// callback's own type) and encoding its result, or the reason this signature cannot project.
/// Arguments and result are owned ordinary Wire values: a `&T` argument is decoded as `T` and
/// lent to the call, a `&mut T` argument is decoded as `T`, lent exclusively and returned after
/// the result in a tuple `(result, final T..)`; a layout without a Wire codec is a gap.
///
/// A leading `&mut Self` or `&Self` parameter is bound to `receiver`, the call's own receiver
/// place (`&mut Game` for `fn(&mut Self, PlayerRef)`): the listener supplies only the rest, and
/// the callee's writes land on the real receiver, so nothing is copied or written back.
pub fn invocation(
    rule: &Rule,
    module: &str,
    owner: Option<&str>,
    ty: &Type,
    callee: TokenStream,
    receiver: Option<TokenStream>,
) -> Result<TokenStream, String> {
    let (mut inputs, output) = signature(ty).ok_or("not a callback")?;
    let bound = match (inputs.first(), receiver) {
        (Some(Type::Reference(first)), Some(receiver)) if matches!(&*first.elem, Type::Path(p) if p.path.is_ident("Self")) =>
        {
            let lend = if first.mutability.is_some() {
                quote!(&mut #receiver)
            } else {
                quote!(&#receiver)
            };
            inputs.remove(0);
            Some(lend)
        }
        _ => None,
    };
    body(rule, module, owner, &inputs, &output, |lend| {
        let lend = bound.iter().chain(lend);
        quote!((#callee)(#(#lend),*))
    })
}

/// One method of a listed trait, by declaration order.
pub struct Method {
    pub name: syn::Ident,
    /// Whether the method takes `&mut self`: only an exclusive trait object can call it.
    pub exclusive: bool,
    /// The parameter types (without the receiver) and result, written in the trait's module,
    /// or why this method cannot be a callable at all.
    pub signature: Result<(Vec<Type>, Type), String>,
}

/// The methods of the listed trait at `path`, in declaration order. A method crosses as a
/// callable when it is object-safe, takes `&self` or `&mut self`, has no type or const generics
/// and is not async; whether its argument and result types cross is decided by [`method_call`].
/// Supertrait methods are not part of the table.
pub fn methods(rule: &Rule, path: &str) -> Vec<Method> {
    let Some(decl) = rule.trait_decl(path) else {
        return Vec::new();
    };
    decl.item
        .items
        .iter()
        .filter_map(|item| match item {
            syn::TraitItem::Fn(method) => Some(method),
            _ => None,
        })
        .map(|method| {
            let sig = &method.sig;
            let exclusive = sig
                .receiver()
                .is_some_and(|r| r.reference.is_some() && r.mutability.is_some());
            let signature = (|| {
                let receiver = sig
                    .receiver()
                    .ok_or("an associated function has no receiver")?;
                if receiver.reference.is_none() || receiver.colon_token.is_some() {
                    return Err("the receiver is taken by value or by a typed `self`".to_owned());
                }
                if sig.asyncness.is_some() {
                    return Err("an async method returns a future".to_owned());
                }
                if sig
                    .generics
                    .params
                    .iter()
                    .any(|p| !matches!(p, syn::GenericParam::Lifetime(_)))
                {
                    return Err("a generic method is not object-safe".to_owned());
                }
                if sig.generics.where_clause.as_ref().is_some_and(|w| {
                    w.predicates
                        .iter()
                        .any(|p| quote!(#p).to_string().contains("Sized"))
                }) {
                    return Err("`where Self: Sized` is not callable on an object".to_owned());
                }
                let mut inputs = Vec::new();
                for input in sig.inputs.iter().skip(1) {
                    let syn::FnArg::Typed(input) = input else {
                        return Err("unexpected receiver".to_owned());
                    };
                    if matches!(&*input.ty, Type::ImplTrait(_)) {
                        return Err("an `impl Trait` argument is generic".to_owned());
                    }
                    inputs.push((*input.ty).clone());
                }
                let output = match &sig.output {
                    syn::ReturnType::Default => syn::parse_quote!(()),
                    syn::ReturnType::Type(_, ty) => (**ty).clone(),
                };
                Ok((inputs, output))
            })();
            Method {
                name: sig.ident.clone(),
                exclusive,
                signature,
            }
        })
        .collect()
}

/// The invocation body of method `name` of the listed trait at `path`, called on the trait
/// object place `callee`, or the reason the method cannot cross. Argument and result types are
/// resolved in the trait's own module.
pub fn method_call(
    rule: &Rule,
    path: &str,
    name: &str,
    callee: TokenStream,
) -> Result<TokenStream, String> {
    let module = &rule.trait_decl(path).ok_or("not a listed trait")?.module;
    let method = methods(rule, path)
        .into_iter()
        .find(|method| method.name == name)
        .ok_or("no such method")?;
    let (inputs, output) = method.signature?;
    let ident = method.name;
    body(
        rule,
        module,
        None,
        &inputs,
        &output,
        |lend| quote!((#callee).#ident(#(#lend),*)),
    )
}

/// Why method `name` cannot cross, if it cannot (the table's callable for it is then absent).
pub fn method_gap(rule: &Rule, path: &str, name: &str) -> Option<String> {
    method_call(rule, path, name, quote!(callee)).err()
}

/// The callee-independent part of an invocation: decode, call, encode.
fn body(
    rule: &Rule,
    module: &str,
    owner: Option<&str>,
    inputs: &[Type],
    output: &Type,
    call: impl FnOnce(&[TokenStream]) -> TokenStream,
) -> Result<TokenStream, String> {
    let mut shapes = Vec::new();
    let mut lend = Vec::new();
    let mut owned_types = Vec::new();
    let mut written = Vec::new();
    for (index, input) in inputs.iter().enumerate() {
        let (owned, how) = argument(input)?;
        let shape = rule.shape(module, owner, &owned);
        if how == Lend::Exclusive {
            written.push((index, shape.clone()));
        }
        shapes.push(shape);
        owned_types.push(rule.qualify(module, &owned));
        let member = syn::Index::from(index);
        lend.push(match how {
            Lend::Value => quote!(__arguments.#member),
            Lend::Shared => quote!(&__arguments.#member),
            Lend::Exclusive => quote!(&mut __arguments.#member),
        });
    }
    let args = native_value::staged_codec(rule, &Shape::Tuple(shapes))
        .map_err(|reason| format!("argument tuple: {reason}"))?;
    // A borrowed string result crosses as the owned string it is a view of.
    let borrowed_str = matches!(output, Type::Reference(r) if r.mutability.is_none()
        && matches!(&*r.elem, Type::Path(p) if p.path.is_ident("str")));
    let owned_output: Type = if borrowed_str {
        syn::parse_quote!(String)
    } else {
        output.clone()
    };
    let mut result_shapes = vec![rule.shape(module, owner, &owned_output)];
    result_shapes.extend(written.iter().map(|(_, shape)| shape.clone()));
    let result = if written.is_empty() {
        native_value::staged_codec(rule, &result_shapes[0])
    } else {
        native_value::staged_codec(rule, &Shape::Tuple(result_shapes))
    }
    .map_err(|reason| format!("result `{}`: {reason}", quote!(#output)))?;
    let decode = args.decode;
    let encode = result.encode;
    let call = call(&lend);
    let members = written.iter().map(|(index, _)| syn::Index::from(*index));
    let returned = if written.is_empty() {
        quote!()
    } else {
        quote!(let __result = (__result, #(__arguments.#members,)*);)
    };
    let call = if borrowed_str {
        quote!(#call.to_owned())
    } else {
        call
    };
    Ok(quote! {
        let mut __reader = mod_api::wire::Reader::new(__bytes);
        #[allow(unused_mut)]
        let mut __arguments: (#(#owned_types,)*) = { let input = &mut __reader; #decode };
        if !__reader.rest().is_empty() {
            return Err(mod_api::wire::WireError::Trailing.into());
        }
        let __result = #call;
        #returned
        let mut __writer = mod_api::wire::Writer::new();
        { let out = &mut __writer; let value = &__result; #encode }
        if let Some(error) = __writer.error() {
            return Err(error.into());
        }
        Ok(__writer.into_bytes())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reason(ty: &str) -> Result<(), String> {
        with_receiver(ty, false)
    }

    fn with_receiver(ty: &str, receiver: bool) -> Result<(), String> {
        let rule = Rule::new(&[crate::internals::Internal {
            path: "horizon_client::BlockPos".into(),
            kind: "struct",
            signature: "pub struct BlockPos { pub x: i32, pub y: i32, pub z: i32 }".into(),
            cfg: String::new(),
            hash: String::new(),
            shape_only: false,
            opaque_reason: None,
            refs: Default::default(),
        }]);
        invocation(
            &rule,
            "horizon_client",
            None,
            &syn::parse_str(ty).unwrap(),
            quote!(callee),
            receiver.then(|| quote!(receiver)),
        )
        .map(|_| ())
    }

    #[test]
    fn catalog_callbacks_project_or_name_their_gap() {
        for ty in [
            "&'s dyn Fn(BlockPos) -> bool",
            "&dyn Fn(&str) -> bool",
            "&dyn Fn(&str) -> Option<String>",
            "&mut dyn FnMut(u32, &str)",
        ] {
            assert_eq!(reason(ty), Ok(()), "{ty}");
        }
        let gap = |ty| reason(ty).unwrap_err();
        // `&mut Self` is not a value from bytes, but the call's own receiver can be lent.
        assert!(gap("fn(&mut Self, u32)").contains("argument tuple"));
        assert_eq!(with_receiver("fn(&mut Self, u32)", true), Ok(()));
        // Json and paths cross as text.
        assert_eq!(reason("&dyn Fn(&str) -> Option<serde_json::Value>"), Ok(()));
        assert_eq!(reason("&mut dyn FnMut(u32, &std::path::Path)"), Ok(()));
        assert!(gap("&mut dyn FnMut(UiResources, &std::path::Path)").contains("argument tuple"));
        assert!(!is_callback(&syn::parse_str("&dyn EventLoopCtl").unwrap()));
    }

    #[test]
    fn trait_methods_cross_or_name_their_gap() {
        let entry = |path: &str, kind, signature: &str| crate::internals::Internal {
            path: path.into(),
            kind,
            signature: signature.into(),
            cfg: String::new(),
            hash: String::new(),
            shape_only: false,
            opaque_reason: None,
            refs: Default::default(),
        };
        let rule = Rule::new(&[
            entry(
                "horizon_client::ctl::Ctl",
                "trait",
                "trait Ctl { fn exit(&self); fn flow(&self, flow: Flow); fn bytes(&self) -> &[u8]; fn name(&self) -> &str; \
                 fn each<T>(&self, t: T); fn take(self); fn edit(&mut self, e: &mut u32); \
                 fn wait(&self, at: std::time::Instant) -> std::time::Duration; }",
            ),
            entry(
                "horizon_client::ctl::Flow",
                "enum",
                "pub enum Flow { Poll, Wait, Until(std::time::Instant) }",
            ),
        ]);
        let gaps: Vec<_> = methods(&rule, "horizon_client::ctl::Ctl")
            .iter()
            .map(|m| method_gap(&rule, "horizon_client::ctl::Ctl", &m.name.to_string()))
            .collect();
        assert_eq!(gaps[0], None);
        assert_eq!(gaps[1], None);
        assert!(gaps[2].as_ref().unwrap().contains("result"));
        // A borrowed string result crosses as the owned string.
        assert_eq!(gaps[3], None);
        assert!(gaps[4].as_ref().unwrap().contains("generic"));
        assert!(gaps[5].as_ref().unwrap().contains("by value"));
        // A `&mut u32` argument is decoded, lent and returned beside the result.
        assert_eq!(gaps[6], None);
        assert_eq!(gaps[7], None);
        // The object's shape keeps the trait's identity, through every pointer.
        let object =
            |ty: &str| rule.shape("horizon_client::ctl", None, &syn::parse_str(ty).unwrap());
        for ty in [
            "&dyn Ctl",
            "&mut dyn Ctl",
            "Box<dyn Ctl>",
            "&(dyn Ctl + Send)",
        ] {
            assert!(
                format!("{:?}", object(ty)).contains("dyn horizon_client::ctl::Ctl"),
                "{ty}"
            );
        }
    }
}
