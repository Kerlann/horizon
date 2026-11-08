//! Typed owned row wrappers derived from the same Rule and exposure as native projections.
use crate::{
    crossing::{Collection, Rule, Scalar, Shape},
    native_schema::Exposure,
};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

/// Emit a root wrapper named `name` and its reachable wrappers. Type keys are supplied by the
/// caller's shared exposure plan, never recomputed by this generator. Every wrapper owns one
/// generic RowView; no native field layout or engine type is reproduced in guest code.
pub fn generate(
    rule: &Rule,
    shape: &Shape,
    name: &str,
    exposure: &dyn Exposure,
) -> Result<TokenStream, String> {
    struct Gen<'a> {
        rule: &'a Rule,
        exposure: &'a dyn Exposure,
        seen: Vec<(Shape, syn::Ident)>,
        out: Vec<TokenStream>,
    }
    impl Gen<'_> {
        fn emit(
            &mut self,
            shape: &Shape,
            requested: Option<syn::Ident>,
        ) -> Result<syn::Ident, String> {
            if let Some((_, name)) = self.seen.iter().find(|(s, _)| s == shape) {
                return Ok(name.clone());
            }
            let name = requested.unwrap_or_else(|| format_ident!("RowType{}", self.seen.len()));
            self.seen.push((shape.clone(), name.clone()));
            let mut methods = Vec::new();
            match shape {
                Shape::Ref { to, .. } | Shape::Wrapped(_, to) => {
                    let child = self.emit(to, None)?;
                    methods.push(quote!(pub fn value(&self) -> #child { #child(self.0.clone()) }));
                }
                Shape::Scalar(Scalar::String | Scalar::Str) | Shape::DynError => {
                    methods.push(quote!(
                        pub fn encode(value: &str) -> Result<Vec<u8>, mod_api::wire::WireError> {
                            let mut out = mod_api::wire::Writer::new();
                            out.limit(1_048_576);
                            out.put_str(value);
                            if let Some(e) = out.error() {
                                return Err(e);
                            }
                            Ok(out.into_bytes())
                        }
                        pub fn text(&self, max_units: usize) -> Result<String, mod_api::RowError> {
                            self.0.utf16(max_units)
                        }
                    ))
                }
                Shape::Scalar(Scalar::Signed("isize")) => {
                    return Err("isize has no ordinary Wire encoding".into());
                }
                Shape::Scalar(_) | Shape::Unit => {
                    let ty = crate::internals_table::spell(shape).ok_or("unsupported scalar")?;
                    methods.push(quote! {
                        pub fn get(&self) -> Result<#ty, mod_api::RowError> { self.0.read() }
                        pub fn set(&self, patch: &mut mod_api::wire::RecordPatch, value: &#ty) -> Result<(), mod_api::wire::WireError> { self.0.set(patch, value) }
                        pub fn encode(value: &#ty) -> Result<Vec<u8>, mod_api::wire::WireError> { let mut out = mod_api::wire::Writer::new(); out.limit(1_048_576); out.put(value); if let Some(e) = out.error() { return Err(e); } Ok(out.into_bytes()) }
                    });
                }
                Shape::Option(to) => {
                    let child = self.emit(to, None)?;
                    methods.push(quote! {
                        pub fn encode_none() -> Vec<u8> { vec![0] }
                        pub fn encode_some(value: &[u8]) -> Result<Vec<u8>, mod_api::wire::WireError> { let mut out = mod_api::wire::Writer::new(); out.limit(1_048_576); out.put(&1u8); out.put_bytes(value); if let Some(e) = out.error() { return Err(e); } Ok(out.into_bytes()) }
                        pub fn value(&self) -> Result<Option<#child>, mod_api::RowError> {
                            match self.0.read::<u32>()? { 0 => Ok(None), 1 => Ok(Some(#child(self.0.child(1)?.child(0)?))), _ => Err(mod_api::RowError::Unsupported) }
                        }
                    });
                }
                Shape::Result(ok, err) => {
                    let ok = self.emit(ok, None)?;
                    let err = self.emit(err, None)?;
                    methods.push(quote! {
                        pub fn encode_ok(value: &[u8]) -> Result<Vec<u8>, mod_api::wire::WireError> { let mut out = mod_api::wire::Writer::new(); out.limit(1_048_576); out.put(&0u8); out.put_bytes(value); if let Some(e) = out.error() { return Err(e); } Ok(out.into_bytes()) }
                        pub fn encode_err(value: &[u8]) -> Result<Vec<u8>, mod_api::wire::WireError> { let mut out = mod_api::wire::Writer::new(); out.limit(1_048_576); out.put(&1u8); out.put_bytes(value); if let Some(e) = out.error() { return Err(e); } Ok(out.into_bytes()) }
                        pub fn value(&self) -> Result<Result<#ok, #err>, mod_api::RowError> {
                            match self.0.read::<u32>()? { 0 => Ok(Ok(#ok(self.0.child(0)?.child(0)?))), 1 => Ok(Err(#err(self.0.child(1)?.child(0)?))), _ => Err(mod_api::RowError::Unsupported) }
                        }
                    });
                }
                Shape::Array(to, n) => self.sequence(to, Some(*n), &mut methods)?,
                Shape::Slice(to) => self.sequence(to, None, &mut methods)?,
                Shape::Collection(kind, args) => {
                    if matches!(kind, Collection::HashMap | Collection::BTreeMap) {
                        let a = self.emit(&args[0], None)?;
                        let b = self.emit(&args[1], None)?;
                        methods.push(quote! {
                            pub fn len(&self) -> Result<u64, mod_api::RowError> { self.0.read() }
                            pub fn encode_entries(entries: &[(&[u8], &[u8])]) -> Result<Vec<u8>, mod_api::wire::WireError> { let mut out = mod_api::wire::Writer::new(); out.limit(1_048_576); out.put_len(entries.len()); for (key,value) in entries { out.put_bytes(key); out.put_bytes(value); } if let Some(e) = out.error() { return Err(e); } Ok(out.into_bytes()) }
                            pub fn entry(&self, index: u32) -> Result<(#a, #b), mod_api::RowError> { if u64::from(index) >= self.len()? { return Err(mod_api::RowError::Bounds); } let pair = self.0.child(index)?; Ok((#a(pair.child(0)?), #b(pair.child(1)?))) }
                        });
                    } else {
                        self.sequence(&args[0], None, &mut methods)?;
                    }
                }
                Shape::Tuple(parts) => {
                    for (i, part) in parts.iter().enumerate() {
                        self.field(
                            part,
                            i as u32,
                            None,
                            format_ident!("field_{i}"),
                            &mut methods,
                        )?;
                    }
                }
                Shape::Named(path, _) => {
                    let named = self.rule.named(path).ok_or("missing named layout")?;
                    if !named.guarded && named.opaque_reason.is_none() {
                        if matches!(named.item, syn::Item::Enum(_)) {
                            methods.push(quote!(
                                pub fn tag(&self) -> Result<u32, mod_api::RowError> {
                                    self.0.read()
                                }
                            ));
                        }
                        let mut ordinals = std::collections::BTreeMap::new();
                        for (i, f) in self.rule.fields(path).iter().enumerate() {
                            let ordinal = ordinals.entry(f.case.map(|c| c.0)).or_insert(0u32);
                            let local = *ordinal;
                            *ordinal += 1;
                            if !self.exposure.field(path, f.case.map(|c| c.0), i as u32) {
                                continue;
                            }
                            let part = self.rule.shape(&named.module, Some(path), f.ty);
                            let member = match &f.member {
                                syn::Member::Named(n) => n.to_string(),
                                syn::Member::Unnamed(n) => format!("field_{}", n.index),
                            };
                            let accessor = if let Some((_, case)) = f.case {
                                format!(
                                    "{}_{}",
                                    heck::AsSnakeCase(case.to_string()),
                                    member.trim_start_matches("r#")
                                )
                            } else {
                                member
                            };
                            self.field(
                                &part,
                                local,
                                f.case.map(|c| c.0),
                                format_ident!("{}", accessor),
                                &mut methods,
                            )?;
                        }
                    }
                }
                Shape::Opaque(_) => {}
            }
            let writable = self.writable(shape, &mut Vec::new());
            if let Shape::Named(path, _) = shape
                && writable
                && let Some(named) = self.rule.named(path)
                && let syn::Item::Enum(item) = &named.item
            {
                for (tag, variant) in item.variants.iter().enumerate() {
                    let encode = format_ident!(
                        "encode_{}",
                        heck::AsSnakeCase(variant.ident.to_string()).to_string()
                    );
                    let count = variant.fields.len();
                    let tag = tag as u32;
                    methods.push(quote! {
                                pub fn #encode(fields: &[&[u8]]) -> Result<Vec<u8>, mod_api::wire::WireError> {
                                    if fields.len() != #count { return Err(mod_api::wire::WireError::Length); }
                                    let mut out = mod_api::wire::Writer::new(); out.limit(1_048_576); out.put(&#tag);
                                    for field in fields { out.put_bytes(field); }
                                    if let Some(e) = out.error() { return Err(e); } Ok(out.into_bytes())
                                }
                            });
                }
            }
            let record_fields = match shape {
                _ if !writable => None,
                Shape::Tuple(parts) => Some(parts.len()),
                Shape::Named(path, _) => self.rule.named(path).and_then(|n| {
                    matches!(n.item, syn::Item::Struct(_)).then(|| self.rule.fields(path).len())
                }),
                _ => None,
            };
            let check = record_fields.map(|count| quote!(if fields.len() != #count { return Err(mod_api::wire::WireError::Length); }));
            let constructor = record_fields.map(|_| quote! {
                /// Encode ordinary Wire fields in declaration order, with a checked count and byte cap.
                pub fn encode_fields(fields: &[&[u8]]) -> Result<Vec<u8>, mod_api::wire::WireError> { #check let mut out = mod_api::wire::Writer::new(); out.limit(1_048_576); for field in fields { out.put_bytes(field); } if let Some(e) = out.error() { return Err(e); } Ok(out.into_bytes()) }
            });
            self.out.push(quote! {
                #[derive(Clone)] pub struct #name(mod_api::RowView);
                impl #name {
                    pub fn new(row: mod_api::RowView, expected_key: u64) -> Result<Self, mod_api::RowError> { if row.type_key() != expected_key { return Err(mod_api::RowError::Unsupported); } Ok(Self(row)) }
                    pub fn row(&self) -> &mod_api::RowView { &self.0 }
                    #constructor
                    #(#methods)*
                }
            });
            Ok(name)
        }
        fn writable(&self, shape: &Shape, open: &mut Vec<String>) -> bool {
            match shape {
                Shape::Opaque(_) => false,
                Shape::Named(path, _) => {
                    let Some(n) = self.rule.named(path) else {
                        return false;
                    };
                    if n.guarded || n.opaque_reason.is_some() || open.contains(path) {
                        return false;
                    }
                    open.push(path.clone());
                    let result = self.rule.fields(path).iter().enumerate().all(|(i, f)| {
                        self.exposure.field(path, f.case.map(|c| c.0), i as u32)
                            && self.writable(&self.rule.shape(&n.module, Some(path), f.ty), open)
                    });
                    open.pop();
                    result
                }
                Shape::Ref { to, .. }
                | Shape::Wrapped(_, to)
                | Shape::Option(to)
                | Shape::Array(to, _)
                | Shape::Slice(to) => self.writable(to, open),
                Shape::Tuple(parts) | Shape::Collection(_, parts) => {
                    parts.iter().all(|p| self.writable(p, open))
                }
                Shape::Result(ok, err) => self.writable(ok, open) && self.writable(err, open),
                _ => true,
            }
        }
        fn sequence(
            &mut self,
            to: &Shape,
            fixed: Option<usize>,
            methods: &mut Vec<TokenStream>,
        ) -> Result<(), String> {
            let child = self.emit(to, None)?;
            let length = match fixed {
                Some(n) => {
                    quote!(if items.len() != #n { return Err(mod_api::wire::WireError::Length); })
                }
                None => quote!(out.put_len(items.len());),
            };
            methods.push(quote! {
                pub fn len(&self) -> Result<u64, mod_api::RowError> { self.0.read() }
                pub fn encode_items(items: &[&[u8]]) -> Result<Vec<u8>, mod_api::wire::WireError> { let mut out = mod_api::wire::Writer::new(); out.limit(1_048_576); #length for item in items { out.put_bytes(item); } if let Some(e) = out.error() { return Err(e); } Ok(out.into_bytes()) }
                pub fn at(&self, index: u32) -> Result<#child, mod_api::RowError> { if u64::from(index) >= self.len()? { return Err(mod_api::RowError::Bounds); } Ok(#child(self.0.child(index)?)) }
            });
            Ok(())
        }
        fn field(
            &mut self,
            part: &Shape,
            index: u32,
            case: Option<u32>,
            accessor: syn::Ident,
            methods: &mut Vec<TokenStream>,
        ) -> Result<(), String> {
            let child = self.emit(part, None)?;
            let row = match case {
                Some(tag) => {
                    quote!({ if self.0.read::<u32>()? != #tag { return Err(mod_api::RowError::Bounds); } self.0.child(#tag)?.child(#index)? })
                }
                None => quote!(self.0.child(#index)?),
            };
            methods.push(quote!(pub fn #accessor(&self) -> Result<#child, mod_api::RowError> { Ok(#child(#row)) }));
            Ok(())
        }
    }
    let mut generator = Gen {
        rule,
        exposure,
        seen: Vec::new(),
        out: Vec::new(),
    };
    generator.emit(
        shape,
        Some(syn::parse_str(name).map_err(|e| format!("{e}"))?),
    )?;
    let out = generator.out;
    Ok(quote!(#(#out)*))
}
