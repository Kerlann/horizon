//! The owned-value Wire derive. Call-table borrowing is deliberately not handled here.
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{Data, DeriveInput, Fields, Type, parse_quote, visit::Visit};

pub(crate) fn expand(mut input: DeriveInput) -> syn::Result<TokenStream> {
    let name = &input.ident;
    let mut via: Option<Type> = None;
    for attr in &input.attrs {
        if attr.path().is_ident("wire") {
            attr.parse_nested_meta(|meta| {
                if !meta.path.is_ident("via") || via.is_some() {
                    return Err(meta.error("expected one `via = Type`"));
                }
                via = Some(meta.value()?.parse()?);
                Ok(())
            })?;
        }
    }
    if via.is_none() {
        for param in input.generics.type_params_mut() {
            param.bounds.push(parse_quote!(::mod_api::wire::Wire));
        }
    }
    if let Some(via) = &via {
        let where_clause = input.generics.make_where_clause();
        where_clause
            .predicates
            .push(parse_quote!(#via: ::mod_api::wire::Wire));
        where_clause
            .predicates
            .push(parse_quote!(#via: for<'__wire> ::core::convert::From<&'__wire Self>));
        where_clause
            .predicates
            .push(parse_quote!(Self: ::core::convert::TryFrom<#via>));
    }
    let (imp, ty, clause) = input.generics.split_for_impl();
    let w = quote!(::mod_api::wire);
    if let Some(via) = via {
        return Ok(quote! {
            #[automatically_derived]
            impl #imp #w::Wire for #name #ty #clause {
                const MIN: usize = <#via as #w::Wire>::MIN;
                const LAYOUT_SEED: u64 = #w::layout_name("via");
                fn layout(visiting: &mut Vec<&'static str>) -> u64 {
                    <#via as #w::Wire>::layout(visiting)
                }
                fn put(&self, out: &mut #w::Writer) {
                    out.put(&<#via as ::core::convert::From<&Self>>::from(self));
                }
                fn put_owned(self, out: &mut #w::Writer) { self.put(out); }
                fn take(input: &mut #w::Reader<'_>) -> Result<Self, #w::WireError> {
                    <Self as ::core::convert::TryFrom<#via>>::try_from(input.take::<#via>()?)
                        .map_err(|_| #w::WireError::Invalid(stringify!(#name)))
                }
            }
        });
    }
    let mut layout = quote!(#w::layout_name(stringify!(#name)));
    let mut min = quote!(0usize);
    let mut puts = Vec::new();
    let mut owned = Vec::new();
    let mut takes = Vec::new();
    let shape;
    let nested;
    match &input.data {
        Data::Struct(data) => {
            shape = match &data.fields {
                Fields::Named(_) => "record",
                Fields::Unnamed(_) => "tuple",
                Fields::Unit => "unit",
            };
            nested = !matches!(data.fields, Fields::Unit);
            let (pattern, build, fields) = fields(&data.fields);
            for (field, bind) in data.fields.iter().zip(&fields) {
                check_owned(&field.ty)?;
                let ft = &field.ty;
                min = quote!(#min + <#ft as #w::Wire>::MIN);
                layout = field_layout(layout, field, name);
                puts.push(quote!(out.put(#bind);));
                owned.push(quote!(out.put_owned(#bind);));
            }
            takes.push(quote!(#build));
            puts = vec![quote!(let Self #pattern = self; #(#puts)*)];
            owned = vec![quote!(let Self #pattern = self; #(#owned)*)];
        }
        Data::Enum(data) => {
            shape = "enum";
            min = quote!(4usize);
            nested = data
                .variants
                .iter()
                .any(|v| !matches!(v.fields, Fields::Unit));
            for (index, variant) in data.variants.iter().enumerate() {
                let index = index as u32;
                let case = &variant.ident;
                layout = quote!(#w::layout_mix(#layout, #w::layout_name(stringify!(#case))));
                let (pat, _, binds) = fields(&variant.fields);
                let pat = quote!(Self::#case #pat);
                // `fields` returns a constructor beginning with Self; enum cases supply that prefix.
                let build = construct(&variant.fields, quote!(Self::#case));
                let mut ps = Vec::new();
                let mut os = Vec::new();
                for (field, bind) in variant.fields.iter().zip(binds) {
                    check_owned(&field.ty)?;
                    layout = field_layout(layout, field, name);
                    ps.push(quote!(out.put(#bind);));
                    os.push(quote!(out.put_owned(#bind);));
                }
                puts.push(quote!(#pat => { out.put(&#index); #(#ps)* }));
                owned.push(quote!(#pat => { out.put(&#index); #(#os)* }));
                takes.push(quote!(#index => #build,));
            }
            puts = vec![quote!(match self { #(#puts,)* _ => unreachable!() })];
            owned = vec![quote!(match self { #(#owned,)* _ => unreachable!() })];
            takes = vec![
                quote!(match input.take::<u32>()? { #(#takes)* tag => return Err(#w::WireError::Tag(tag)) }),
            ];
        }
        Data::Union(_) => return Err(syn::Error::new_spanned(name, "Wire cannot encode a union")),
    }
    layout = quote!(#w::layout_mix(#w::layout_name(#shape), #layout));
    let take = if nested {
        quote!(input.nested(|input| Ok(#(#takes)*)))
    } else {
        quote!(Ok(#(#takes)*))
    };
    Ok(quote! {
        #[allow(unreachable_patterns, unused_variables)]
        #[automatically_derived]
            impl #imp #w::Wire for #name #ty #clause {
            const MIN: usize = #min;
            const LAYOUT_SEED: u64 = #w::layout_name(stringify!(#name));
            fn layout(visiting: &mut Vec<&'static str>) -> u64 {
                let identity = ::core::any::type_name::<Self>();
                if visiting.contains(&identity) { return #w::layout_name("recursive"); }
                visiting.push(identity);
                let hash = #layout;
                visiting.pop();
                hash
            }
            fn put(&self, out: &mut #w::Writer) { #(#puts)* }
            fn put_owned(self, out: &mut #w::Writer) { #(#owned)* }
            fn take(input: &mut #w::Reader<'_>) -> Result<Self, #w::WireError> { #take }
        }
    })
}

fn fields(fields: &Fields) -> (TokenStream, TokenStream, Vec<syn::Ident>) {
    let binds: Vec<_> = fields
        .iter()
        .enumerate()
        .map(|(i, _)| format_ident!("__wire_f_{i}"))
        .collect();
    let pattern = match fields {
        Fields::Named(fields) => {
            let names = fields.named.iter().map(|f| &f.ident);
            quote!({ #(#names: #binds),* })
        }
        Fields::Unnamed(_) => quote!((#(#binds),*)),
        Fields::Unit => quote!(),
    };
    (pattern, construct(fields, quote!(Self)), binds)
}

fn construct(fields: &Fields, prefix: TokenStream) -> TokenStream {
    match fields {
        Fields::Named(fields) => {
            let names = fields.named.iter().map(|f| &f.ident);
            quote!(#prefix { #(#names: input.take()?),* })
        }
        Fields::Unnamed(fields) => {
            let takes = fields.unnamed.iter().map(|_| quote!(input.take()?));
            quote!(#prefix (#(#takes),*))
        }
        Fields::Unit => prefix,
    }
}

fn field_layout(previous: TokenStream, field: &syn::Field, _: &syn::Ident) -> TokenStream {
    let ty = &field.ty;
    let name = field
        .ident
        .as_ref()
        .map(ToString::to_string)
        .unwrap_or_default();
    quote!(::mod_api::wire::layout_mix(::mod_api::wire::layout_mix(#previous, ::mod_api::wire::layout_name(#name)), <#ty as ::mod_api::wire::Wire>::layout(visiting)))
}

fn check_owned(ty: &Type) -> syn::Result<()> {
    struct Borrowed(Option<syn::Error>);
    impl<'ast> Visit<'ast> for Borrowed {
        fn visit_type_path(&mut self, p: &'ast syn::TypePath) {
            if p.path.segments.last().is_some_and(|s| s.ident == "Cow") {
                if let syn::PathArguments::AngleBracketed(args) =
                    &p.path.segments.last().unwrap().arguments
                {
                    for arg in &args.args {
                        if let syn::GenericArgument::Type(t) = arg {
                            self.visit_type(t);
                        }
                    }
                }
                return;
            }
            syn::visit::visit_type_path(self, p);
        }
        fn visit_lifetime(&mut self, lifetime: &'ast syn::Lifetime) {
            if lifetime.ident != "static" {
                self.0 = Some(syn::Error::new_spanned(
                    lifetime,
                    "Wire is for owned values; a borrowed record crosses as a call argument",
                ));
            }
        }
        fn visit_type_reference(&mut self, r: &'ast syn::TypeReference) {
            let static_str = r.mutability.is_none()
                && r.lifetime.as_ref().is_some_and(|l| l.ident == "static")
                && matches!(&*r.elem, Type::Path(p) if p.path.is_ident("str"));
            if !static_str {
                self.0 = Some(syn::Error::new_spanned(
                    r,
                    "Wire is for owned values; a borrowed record crosses as a call argument",
                ));
            }
        }
    }
    let mut check = Borrowed(None);
    check.visit_type(ty);
    check.0.map_or(Ok(()), Err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes_expand_to_rust_items() {
        for source in [
            "struct Named { a: u32, b: String }",
            "struct Tuple(u32, String);",
            "struct Unit;",
            "enum Plain { A = 7, B = 9 }",
            "enum Empty {}",
            "struct Shadow { out: u32, input: u8 }",
            "enum Payload { A(u32), B { text: String }, C }",
            "struct Generic<T, const N: usize> { value: [T; N] }",
            "struct Recursive { children: Vec<Recursive> }",
            "#[wire(via = String)] struct Checked(String);",
            "struct Static { text: &'static str }",
        ] {
            let expanded = expand(syn::parse_str(source).unwrap()).unwrap();
            syn::parse2::<syn::ItemImpl>(expanded).unwrap_or_else(|e| panic!("{source}: {e}"));
        }
    }

    #[test]
    fn borrowed_fields_are_rejected_even_inside_containers() {
        for source in [
            "struct Borrow<'a> { value: &'a str }",
            "struct Borrow<'a> { value: Vec<&'a u8> }",
            "struct Borrow { value: &'static [u8] }",
            "struct Borrow { value: &'static mut str }",
            "struct Borrow<'a> { value: Option<Other<'a>> }",
        ] {
            let err = expand(syn::parse_str(source).unwrap()).unwrap_err();
            assert!(err.to_string().contains("Wire is for owned values"));
        }
    }

    #[test]
    fn union_and_unknown_options_are_rejected() {
        for source in [
            "union Union { a: u32, b: u8 }",
            "#[wire(unknown)] struct Unknown;",
            "#[wire(via = String, via = u8)] struct Duplicate;",
        ] {
            assert!(expand(syn::parse_str(source).unwrap()).is_err());
        }
    }
}
