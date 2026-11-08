//! Generation of owned native values from ordinary Wire values. Unsupported layouts are
//! reported rather than guessed; generated decoders never borrow the packet.
use crate::crossing::{Collection, Rule, Scalar, Shape, Wrapper};
use proc_macro2::TokenStream;
use quote::quote;

/// A codec expression pair. `decode` expects `input: &mut mod_api::wire::Reader`;
/// `encode` expects `value` to be a shared reference and `out: &mut Writer`.
pub struct ValueCodec {
    pub decode: TokenStream,
    pub encode: TokenStream,
}

/// Generate a codec for one complete owned value. Recursive and inaccessible layouts are gaps.
pub fn codec(rule: &Rule, shape: &Shape) -> Result<ValueCodec, String> {
    build(rule, shape, &mut Vec::new(), Mode::Inline)
}

/// [`codec`] for a value built and read inline at a staged point, where the fields of the
/// layouts of the workspace crates below the game are public (the staged copy widens them).
pub fn staged_codec(rule: &Rule, shape: &Shape) -> Result<ValueCodec, String> {
    rule.widened.set(true);
    let codec = codec(rule, shape);
    rule.widened.set(false);
    codec
}

/// How a named layout below the requested shape is spelled.
#[derive(Clone, Copy, PartialEq)]
enum Mode {
    /// Every layout expands in place.
    Inline,
    /// A named layout is a call to its once-per-source decoder function.
    Calls,
    /// Expand this named layout now; its members are calls.
    Body,
}

/// Decode expression whose named layouts are calls to [`decoders`] functions, so generated
/// code grows with the layout graph once, not with each use.
pub fn decode_call(rule: &Rule, shape: &Shape) -> Result<ValueCodec, String> {
    build(rule, shape, &mut Vec::new(), Mode::Calls)
}

fn decoder_name(path: &str) -> proc_macro2::Ident {
    let mangled: String = path
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect();
    quote::format_ident!("__horizon_decode_{mangled}")
}

/// One decoder function per decodable named layout, emitted once per source.
pub fn decoders<'a>(rule: &Rule, paths: impl Iterator<Item = &'a str>) -> TokenStream {
    let mut out = Vec::new();
    for path in paths {
        let Ok(codec) = build(
            rule,
            &Shape::Named(path.to_owned(), 0),
            &mut Vec::new(),
            Mode::Body,
        ) else {
            continue;
        };
        let name = decoder_name(path);
        let native: syn::Path = syn::parse_str(
            &path
                .strip_prefix("horizon_client::")
                .map_or_else(|| path.to_owned(), |p| format!("crate::{p}")),
        )
        .expect("a decodable layout has a path");
        let decode = codec.decode;
        out.push(quote!(pub fn #name(input: &mut mod_api::wire::Reader<'_>) -> Result<#native, mod_api::wire::WireError> { Ok(#decode) }));
    }
    quote!(#(#out)*)
}

fn build(
    rule: &Rule,
    shape: &Shape,
    stack: &mut Vec<String>,
    mode: Mode,
) -> Result<ValueCodec, String> {
    let pair = match shape {
        // An instant crosses as the nanoseconds from the moment it is encoded to it (none if
        // past), and is rebuilt from the moment it is decoded; a duration as seconds and nanos.
        Shape::Opaque(Some(name)) if name == "std::time::Instant" => ValueCodec {
            decode: quote!(
                std::time::Instant::now() + std::time::Duration::from_nanos(input.take::<u64>()?)
            ),
            encode: quote!(out.put(&u64::try_from(value.saturating_duration_since(std::time::Instant::now()).as_nanos()).unwrap_or(u64::MAX));),
        },
        // JSON crosses as its text, a path as its (lossy) UTF-8 text.
        Shape::Opaque(Some(name)) if name == "serde_json::Value" => ValueCodec {
            decode: quote!({
                let text: String = input.take()?;
                serde_json::from_str::<serde_json::Value>(&text)
                    .map_err(|_| mod_api::wire::WireError::Invalid("json text"))?
            }),
            encode: quote!({
                let text = value.to_string();
                out.put(&text);
            }),
        },
        Shape::Opaque(Some(name)) if name == "std::path::PathBuf" => ValueCodec {
            decode: quote!({
                let text: String = input.take()?;
                std::path::PathBuf::from(text)
            }),
            encode: quote!({
                let text = value.to_string_lossy().into_owned();
                out.put(&text);
            }),
        },
        Shape::Opaque(Some(name)) if name == "std::time::Duration" => ValueCodec {
            decode: quote!({
                let secs = input.take::<u64>()?;
                let nanos = input.take::<u32>()?;
                if nanos >= 1_000_000_000 {
                    return Err(mod_api::wire::WireError::Invalid("duration nanoseconds"));
                }
                std::time::Duration::new(secs, nanos)
            }),
            encode: quote!({
                out.put(&value.as_secs());
                out.put(&value.subsec_nanos());
            }),
        },
        Shape::Scalar(Scalar::Str) | Shape::Ref { .. } | Shape::Slice(_) | Shape::Opaque(_) => {
            return Err(format!("non-owned or opaque shape: {shape:?}"));
        }
        Shape::Scalar(Scalar::Signed("isize")) => {
            return Err("isize has no Wire encoding".into());
        }
        Shape::Scalar(_) | Shape::Unit => ValueCodec {
            decode: quote!(input.take()?),
            encode: quote!(out.put(value);),
        },
        Shape::Wrapped(Wrapper::Box, inner) if **inner == Shape::DynError => ValueCodec {
            decode: quote!(::std::convert::From::<::std::string::String>::from(
                input.take::<::std::string::String>()?
            )),
            encode: quote!({
                let value = &value.to_string();
                out.put(value);
            }),
        },
        Shape::Result(ok, err) => {
            let ok = build(rule, ok, stack, mode)?;
            let err = build(rule, err, stack, mode)?;
            let (od, oe, ed, ee) = (ok.decode, ok.encode, err.decode, err.encode);
            ValueCodec {
                decode: quote!(match input.take::<u8>()? { 0 => Ok(input.nested(|input| Ok(#od))?), 1 => Err(input.nested(|input| Ok(#ed))?), tag => return Err(mod_api::wire::WireError::Tag(tag.into())) }),
                encode: quote!(match value { Ok(value) => { out.put(&0u8); #oe } Err(value) => { out.put(&1u8); #ee } }),
            }
        }
        Shape::Wrapped(Wrapper::Box, inner) => {
            let c = build(rule, inner, stack, mode)?;
            let d = c.decode;
            let e = c.encode;
            ValueCodec {
                decode: quote!(Box::new(input.nested(|input| Ok(#d))?)),
                encode: quote!({ let value = &**value; #e }),
            }
        }
        // A shared pointer owns a copy on this side of the boundary; `Arc<str>` is a string.
        Shape::Wrapped(wrapper @ (Wrapper::Arc | Wrapper::Rc), inner) => {
            let pointer = if *wrapper == Wrapper::Arc {
                quote!(std::sync::Arc)
            } else {
                quote!(std::rc::Rc)
            };
            if matches!(**inner, Shape::Scalar(Scalar::Str)) {
                let c = build(rule, &Shape::Scalar(Scalar::String), stack, mode)?;
                let d = c.decode;
                ValueCodec {
                    decode: quote!(#pointer::<str>::from(#d)),
                    encode: quote!({
                        let value = &String::from(&**value);
                        out.put(value);
                    }),
                }
            } else {
                let c = build(rule, inner, stack, mode)?;
                let d = c.decode;
                let e = c.encode;
                ValueCodec {
                    decode: quote!(#pointer::new(input.nested(|input| Ok(#d))?)),
                    encode: quote!({ let value = &**value; #e }),
                }
            }
        }
        Shape::Option(inner) => {
            let c = build(rule, inner, stack, mode)?;
            let d = c.decode;
            let e = c.encode;
            ValueCodec {
                decode: quote!(match input.take::<u8>()? { 0 => None, 1 => Some(input.nested(|input| Ok(#d))?), tag => return Err(mod_api::wire::WireError::Tag(tag.into())) }),
                encode: quote!(match value { None => out.put(&0u8), Some(value) => { out.put(&1u8); #e } }),
            }
        }
        Shape::Tuple(parts) => {
            let cs = parts
                .iter()
                .map(|s| build(rule, s, stack, mode))
                .collect::<Result<Vec<_>, _>>()?;
            let ds = cs.iter().map(|c| &c.decode);
            let es = cs.iter().enumerate().map(|(i, c)| {
                let i = syn::Index::from(i);
                let e = &c.encode;
                quote!({ let value = &value.#i; #e })
            });
            ValueCodec {
                decode: quote!(input.nested(|input| Ok((#(#ds,)*)))?),
                encode: quote!(#(#es)*),
            }
        }
        Shape::Array(inner, len) => {
            let c = build(rule, inner, stack, mode)?;
            let d = c.decode;
            let e = c.encode;
            ValueCodec {
                decode: quote!(input.nested(|input| { let mut values = Vec::new(); for _ in 0..#len { values.push(#d); } values.try_into().map_err(|_| mod_api::wire::WireError::Length) })?),
                encode: quote!(for value in value.iter() { #e }),
            }
        }
        Shape::Collection(kind, parts)
            if parts.len() == 1
                && matches!(
                    kind,
                    Collection::Vec
                        | Collection::VecDeque
                        | Collection::HashSet
                        | Collection::BTreeSet
                ) =>
        {
            let c = build(rule, &parts[0], stack, mode)?;
            let d = c.decode;
            let e = c.encode;
            ValueCodec {
                decode: quote!(input.nested(|input| { let len = input.take_len(1)?; let mut values = Vec::with_capacity(len); for _ in 0..len { values.push(#d); } Ok(values.into_iter().collect()) })?),
                encode: quote!({ out.put_len(value.len()); for value in value.iter() { #e } }),
            }
        }
        Shape::Collection(kind, parts)
            if parts.len() == 2 && matches!(kind, Collection::HashMap | Collection::BTreeMap) =>
        {
            let key = build(rule, &parts[0], stack, mode)?;
            let val = build(rule, &parts[1], stack, mode)?;
            let kd = key.decode;
            let vd = val.decode;
            let ke = key.encode;
            let ve = val.encode;
            ValueCodec {
                decode: quote!(input.nested(|input| { let len = input.take_len(1)?; let mut values = Vec::with_capacity(len); for _ in 0..len { values.push((#kd, #vd)); } Ok(values.into_iter().collect()) })?),
                encode: quote!({ out.put_len(value.len()); for (key, val) in value.iter() { { let value = key; #ke } { let value = val; #ve } } }),
            }
        }
        Shape::Named(path, _) => {
            if stack.contains(path) {
                return Err(format!("recursive layout: {path}"));
            }
            if mode == Mode::Calls {
                let known = rule.decodable.borrow().get(path).copied();
                let ok = match known {
                    Some(ok) => ok,
                    None => {
                        let ok = build(rule, shape, stack, Mode::Body).is_ok();
                        rule.decodable.borrow_mut().insert(path.clone(), ok);
                        ok
                    }
                };
                if !ok {
                    return Err(format!("undecodable layout: {path}"));
                }
                let name = decoder_name(path);
                return Ok(ValueCodec {
                    decode: quote!(crate::__horizon_points::#name(input)?),
                    encode: quote!(),
                });
            }
            let mode = if mode == Mode::Body {
                Mode::Calls
            } else {
                mode
            };
            let named = rule
                .named(path)
                .ok_or_else(|| format!("unknown layout: {path}"))?;
            if named.lifetimes != 0
                || named.generic
                || named.guarded
                || named.opaque_reason.is_some()
                || (named.foreign && named.sealed)
            {
                return Err(format!("inaccessible layout: {path}"));
            }
            let name = path
                .strip_prefix("horizon_client::")
                .map_or_else(|| path.to_owned(), |p| format!("crate::{p}"));
            let ty: syn::Path =
                syn::parse_str(&name).map_err(|_| format!("invalid path: {path}"))?;
            stack.push(path.clone());
            let result = match &named.item {
                syn::Item::Struct(item) => {
                    fields(rule, named, path, &ty, &item.fields, stack, mode)
                }
                syn::Item::Enum(item) => {
                    let mut decoders = Vec::new();
                    let mut encoders = Vec::new();
                    for (index, variant) in item.variants.iter().enumerate() {
                        let variant_name = &variant.ident;
                        let variant_path: syn::Path = syn::parse_quote!(#ty::#variant_name);
                        let mut ds = Vec::new();
                        let mut es = Vec::new();
                        let mut bindings = Vec::new();
                        for (i, field) in variant.fields.iter().enumerate() {
                            let c = build(
                                rule,
                                &rule.shape(&named.module, Some(path), &field.ty),
                                stack,
                                mode,
                            )?;
                            let binding = quote::format_ident!("field_{i}");
                            let d = c.decode;
                            let e = c.encode;
                            ds.push(d);
                            es.push(quote!({ let value = #binding; #e }));
                            bindings.push(binding);
                        }
                        let (construct, pattern) = match &variant.fields {
                            syn::Fields::Named(f) => {
                                let names: Vec<_> =
                                    f.named.iter().map(|f| f.ident.as_ref().unwrap()).collect();
                                (
                                    quote!(#variant_path { #(#names: #ds,)* }),
                                    quote!(#variant_path { #(#names: #bindings,)* }),
                                )
                            }
                            syn::Fields::Unnamed(_) => (
                                quote!(#variant_path(#(#ds,)*)),
                                quote!(#variant_path(#(#bindings,)*)),
                            ),
                            syn::Fields::Unit => (quote!(#variant_path), quote!(#variant_path)),
                        };
                        let tag = index as u32;
                        decoders.push(quote!(#tag => #construct,));
                        encoders.push(quote!(#pattern => { out.put(&#tag); #(#es)* },));
                    }
                    Ok(ValueCodec {
                        decode: quote!(input.nested(|input| Ok(match input.take::<u32>()? { #(#decoders)* _ => return Err(mod_api::wire::WireError::Invalid("unknown enum variant")) }))?),
                        encode: quote!(match value { #(#encoders)* }),
                    })
                }
                _ => Err(format!("unsupported layout: {path}")),
            };
            stack.pop();
            result?
        }
        _ => return Err(format!("decoder not implemented: {shape:?}")),
    };
    Ok(pair)
}

fn fields(
    rule: &Rule,
    named: &crate::crossing::Named,
    path: &str,
    ty: &syn::Path,
    fields: &syn::Fields,
    stack: &mut Vec<String>,
    mode: Mode,
) -> Result<ValueCodec, String> {
    if named.foreign
        && !(named.widened && rule.widened.get())
        && fields
            .iter()
            .any(|f| !matches!(f.vis, syn::Visibility::Public(_)))
    {
        return Err(format!("private foreign constructor: {path}"));
    }
    let mut ds = Vec::new();
    let mut es = Vec::new();
    for (i, f) in fields.iter().enumerate() {
        let c = build(
            rule,
            &rule.shape(&named.module, Some(path), &f.ty),
            stack,
            mode,
        )?;
        let member = f.ident.clone().map_or(
            syn::Member::Unnamed(syn::Index::from(i)),
            syn::Member::Named,
        );
        let d = c.decode;
        let e = c.encode;
        ds.push(quote!(#d));
        es.push(quote!({ let value = &value.#member; #e }));
    }
    let construct = match fields {
        syn::Fields::Named(f) => {
            let names = f.named.iter().map(|f| f.ident.as_ref().unwrap());
            quote!(#ty { #(#names: #ds,)* })
        }
        syn::Fields::Unnamed(_) => quote!(#ty(#(#ds,)*)),
        syn::Fields::Unit => quote!(#ty),
    };
    Ok(ValueCodec {
        decode: quote!(input.nested(|input| Ok(#construct))?),
        encode: quote!(#(#es)*),
    })
}
