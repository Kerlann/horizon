//! Rule-driven native window projection generation.
use crate::{
    crossing::{Collection, Rule, Scalar, Shape, Wrapper},
    native_schema::{self, Exposure},
};
use proc_macro2::TokenStream;
use quote::quote;
use syn::Type;

/// Expression constructing a borrowed native node. The expression borrows `place` and is valid
/// only while that native value remains live. Unsupported shapes fail generation explicitly.
pub fn generate(
    rule: &Rule,
    module: &str,
    owner: Option<&str>,
    ty: &Type,
    place: TokenStream,
    exposure: &dyn Exposure,
) -> Result<TokenStream, String> {
    fn emit(
        rule: &Rule,
        shape: &Shape,
        place: TokenStream,
        exposure: &dyn Exposure,
        depth: usize,
    ) -> Result<TokenStream, String> {
        if depth > 128 {
            return Err("unimplemented: recursive native projection".into());
        }
        let identity = native_schema::identity(rule, shape, exposure)?;
        let key = quote!(mod_points::snapshot::key(#identity));
        Ok(match shape {
            Shape::Scalar(Scalar::String | Scalar::Str) => {
                let unit = native_schema::identity(
                    rule,
                    &Shape::Scalar(Scalar::Unsigned("u16")),
                    exposure,
                )?;
                quote!(mod_points::native_nodes::Text((#place).as_ref(), #key, mod_points::snapshot::key(#unit)))
            }
            Shape::Scalar(Scalar::Signed("isize")) => {
                return Err("unimplemented: isize has no Wire encoding".into());
            }
            Shape::Scalar(_) | Shape::Unit => {
                quote!(mod_points::native_nodes::Scalar(&(#place), #key))
            }
            Shape::Ref { to, .. }
            | Shape::Wrapped(Wrapper::Box | Wrapper::Arc | Wrapper::Rc, to) => {
                return emit(rule, to, quote!(**(#place)), exposure, depth + 1);
            }
            Shape::Wrapped(Wrapper::Cow, to) => {
                return emit(rule, to, quote!(*(#place).as_ref()), exposure, depth + 1);
            }
            Shape::Array(_, _) | Shape::Slice(_) | Shape::Collection(Collection::Vec, _) => {
                let to = match shape {
                    Shape::Collection(_, args) => {
                        args.first().ok_or("unimplemented: malformed vector")?
                    }
                    Shape::Array(to, _) | Shape::Slice(to) => to,
                    _ => unreachable!(),
                };
                let child = emit(rule, to, quote!(*__horizon_item), exposure, depth + 1)?;
                quote!(mod_points::native_nodes::sequence(&(#place), #key, |__horizon_item, __horizon_visit| __horizon_visit(mod_points::locators::Step::Index(0), 0, &#child)))
            }
            Shape::Collection(kind, args) => {
                let first = args.first().ok_or("unimplemented: malformed collection")?;
                let key_child = emit(rule, first, quote!(*__horizon_first), exposure, depth + 1)?;
                let visit = match kind {
                    Collection::VecDeque | Collection::HashSet | Collection::BTreeSet => quote! {
                        let __horizon_first = (#place).iter().nth(usize::try_from(__horizon_index).map_err(|_| mod_api::RowError::Bounds)?).ok_or(mod_api::RowError::Bounds)?;
                        __horizon_visit(mod_points::locators::Step::Index(0), 0, &#key_child)
                    },
                    Collection::HashMap | Collection::BTreeMap => {
                        let second = args.get(1).ok_or("unimplemented: malformed map")?;
                        let value_child =
                            emit(rule, second, quote!(*__horizon_second), exposure, depth + 1)?;
                        let pair = format!("{identity};pair");
                        quote! {
                            let (__horizon_first, __horizon_second) = (#place).iter().nth(usize::try_from(__horizon_index).map_err(|_| mod_api::RowError::Bounds)?).ok_or(mod_api::RowError::Bounds)?;
                            __horizon_visit(mod_points::locators::Step::Index(0), 0, &mod_points::native_nodes::Record { key: mod_points::snapshot::key(#pair), count: 2, visit: |__horizon_index, __horizon_visit: &mut mod_points::native_nodes::Visitor<'_>| match __horizon_index {
                                0 => __horizon_visit(mod_points::locators::Step::Field(0), 0, &#key_child),
                                1 => __horizon_visit(mod_points::locators::Step::Field(1), 1, &#value_child),
                                _ => Err(mod_api::RowError::Bounds),
                            } })
                        }
                    }
                    Collection::Vec => unreachable!(),
                };
                quote!(mod_points::native_nodes::Collection { key: #key, count: (#place).len() as u64, visit: |__horizon_index, __horizon_visit: &mut mod_points::native_nodes::Visitor<'_>| { #visit } })
            }
            Shape::Option(to) => {
                let child = emit(rule, to, quote!(*__horizon_some), exposure, depth + 1)?;
                let some = format!("{identity};variant(1)");
                let none = format!("{identity};variant(0)");
                quote!({
                    let __horizon_tag: u32 = u32::from((#place).is_some());
                    mod_points::native_nodes::Variant { key: #key, tag: __horizon_tag, visit: |__horizon_visit: &mut mod_points::native_nodes::Visitor<'_>| {
                        match &(#place) {
                            Some(__horizon_some) => __horizon_visit(mod_points::locators::Step::Variant(1), 1, &mod_points::native_nodes::Record { key: mod_points::snapshot::key(#some), count: 1, visit: |__horizon_index, __horizon_visit: &mut mod_points::native_nodes::Visitor<'_>| match __horizon_index { 0 => __horizon_visit(mod_points::locators::Step::Field(0), 0, &#child), _ => Err(mod_api::RowError::Bounds) } }),
                            None => __horizon_visit(mod_points::locators::Step::Variant(0), 0, &mod_points::native_nodes::Record { key: mod_points::snapshot::key(#none), count: 0, visit: |_, _: &mut mod_points::native_nodes::Visitor<'_>| Err(mod_api::RowError::Bounds) }),
                        }
                    } }
                })
            }
            Shape::Tuple(parts) => {
                let mut arms = Vec::new();
                for (index, part) in parts.iter().enumerate() {
                    let member = syn::Index::from(index);
                    let field = index as u32;
                    let ordinal = index as u64;
                    let child = emit(rule, part, quote!((#place).#member), exposure, depth + 1)?;
                    arms.push(quote!(#ordinal => __horizon_visit(mod_points::locators::Step::Field(#field), #field, &#child),));
                }
                let count = parts.len() as u64;
                quote!(mod_points::native_nodes::Record { key: #key, count: #count, visit: |__horizon_index, __horizon_visit: &mut mod_points::native_nodes::Visitor<'_>| match __horizon_index { #(#arms)* _ => Err(mod_api::RowError::Bounds) } })
            }
            Shape::Named(path, _) => {
                let named = rule
                    .named(path)
                    .ok_or("unimplemented: missing named layout")?;
                if let syn::Item::Enum(item) = &named.item {
                    let mut variants = Vec::new();
                    let fields = rule.fields(path);
                    let name_path = path.clone();
                    let path: syn::Path =
                        syn::parse_str(&path.replace("horizon_client::", "crate::"))
                            .map_err(|_| "unimplemented: enum path")?;
                    for (case, variant) in item.variants.iter().enumerate() {
                        let tag = case as u32;
                        let name = &variant.ident;
                        let mut bindings = Vec::new();
                        let mut arms = Vec::new();
                        for (index, field) in fields
                            .iter()
                            .enumerate()
                            .filter(|(_, f)| f.case.is_some_and(|c| c.0 == tag))
                        {
                            let member = &field.member;
                            let bind = quote::format_ident!("__horizon_field_{index}");
                            bindings.push(quote!(#member: #bind));
                            if !exposure.field(&name_path, Some(tag), index as u32) {
                                continue;
                            }
                            let child_shape = rule.shape(&named.module, None, field.ty);
                            let child =
                                emit(rule, &child_shape, quote!(*#bind), exposure, depth + 1)?;
                            let field_id = index as u32;
                            let ordinal = arms.len() as u64;
                            arms.push(quote!(#ordinal => __horizon_visit(mod_points::locators::Step::Field(#field_id), #field_id, &#child),));
                        }
                        let count = arms.len() as u64;
                        let variant_key = format!("{identity};variant({tag})");
                        variants.push(quote!(#path::#name { #(#bindings,)* .. } => __horizon_visit(mod_points::locators::Step::Variant(#tag), #tag, &mod_points::native_nodes::Record { key: mod_points::snapshot::key(#variant_key), count: #count, visit: |__horizon_index, __horizon_visit: &mut mod_points::native_nodes::Visitor<'_>| match __horizon_index { #(#arms)* _ => Err(mod_api::RowError::Bounds) } }),));
                    }
                    let tags = item.variants.iter().enumerate().map(|(i, v)| {
                        let name = &v.ident;
                        let i = i as u32;
                        quote!(#path::#name { .. } => #i,)
                    });
                    return Ok(
                        quote!({ let __horizon_tag = match &(#place) { #(#tags)* _ => u32::MAX }; mod_points::native_nodes::Variant { key: #key, tag: __horizon_tag, visit: |__horizon_visit: &mut mod_points::native_nodes::Visitor<'_>| match &(#place) { #(#variants)* _ => Err(mod_api::RowError::Unsupported) } } }),
                    );
                }
                let mut arms = Vec::new();
                for (index, field) in rule.fields(path).iter().enumerate() {
                    if !exposure.field(path, None, index as u32) {
                        continue;
                    }
                    let member = &field.member;
                    let shape = rule.shape(&named.module, Some(path), field.ty);
                    let child = emit(rule, &shape, quote!((#place).#member), exposure, depth + 1)?;
                    let field_id = index as u32;
                    let ordinal = arms.len() as u64;
                    arms.push(quote!(#ordinal => __horizon_visit(mod_points::locators::Step::Field(#field_id), #field_id, &#child),));
                }
                let count = arms.len() as u64;
                quote!(mod_points::native_nodes::Record { key: #key, count: #count, visit: |__horizon_index, __horizon_visit: &mut mod_points::native_nodes::Visitor<'_>| match __horizon_index { #(#arms)* _ => Err(mod_api::RowError::Bounds) } })
            }
            _ => {
                return Err(format!(
                    "unimplemented: native row projection for {shape:?}"
                ));
            }
        })
    }
    emit(rule, &rule.shape(module, owner, ty), place, exposure, 0)
}
