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
    generate_inner(rule, module, owner, ty, place, exposure, None)
}

/// Generate a reader using a registration-time exposure plan. The graph must contain this root.
/// Unsupported members are omitted; Plan registration refuses policies reaching those members.
pub fn generate_runtime(
    rule: &Rule,
    module: &str,
    owner: Option<&str>,
    ty: &Type,
    place: TokenStream,
    graph: &crate::native_policy::Graph,
    plan: TokenStream,
) -> Result<TokenStream, String> {
    generate_inner(
        rule,
        module,
        owner,
        ty,
        place,
        &native_schema::All,
        Some((graph, plan)),
    )
}

fn generate_inner(
    rule: &Rule,
    module: &str,
    owner: Option<&str>,
    ty: &Type,
    place: TokenStream,
    exposure: &dyn Exposure,
    runtime: Option<(&crate::native_policy::Graph, TokenStream)>,
) -> Result<TokenStream, String> {
    fn emit(
        rule: &Rule,
        shape: &Shape,
        place: TokenStream,
        exposure: &dyn Exposure,
        depth: usize,
        runtime: &Option<(&crate::native_policy::Graph, TokenStream)>,
    ) -> Result<TokenStream, String> {
        if depth > 128 {
            return Err("unimplemented: recursive native projection".into());
        }
        if let Some((graph, plan)) = runtime {
            let id = graph.id(shape).ok_or("missing runtime schema")?;
            // A trait object outside an argument root is only a typed header: its method table
            // is made where the call's lease is.
            if graph.layouts[id].opaque.is_some()
                || matches!(shape, Shape::Opaque(Some(name)) if name.starts_with("dyn "))
            {
                return Ok(quote!(mod_points::native_nodes::Opaque((#plan).key(#id).unwrap_or(0))));
            }
        }
        if depth > 0
            && runtime.is_some()
            && let Shape::Named(path, _) = shape
        {
            let (graph, plan) = runtime.as_ref().unwrap();
            let id = graph.id(shape).ok_or("missing runtime schema")?;
            let helper = quote::format_ident!("__horizon_node_{id}");
            let _ = path;
            return Ok(quote!(crate::__horizon_points::#helper(&(#place), &(#plan))));
        }
        let identity = if runtime.is_some() {
            format!("runtime:{shape:?}")
        } else {
            native_schema::identity(rule, shape, exposure)?
        };
        let key = if let Some((graph, plan)) = runtime {
            let id = graph.id(shape).ok_or("missing runtime schema")?;
            quote!((#plan).key(#id).unwrap_or(0))
        } else {
            quote!(const { mod_points::snapshot::key(#identity) })
        };
        let auxiliary = |role: &str, fallback: &str| -> Result<TokenStream, String> {
            if let Some((graph, plan)) = runtime {
                let id = graph
                    .auxiliary(shape, role)
                    .ok_or("missing auxiliary schema")?;
                Ok(quote!((#plan).key(#id).unwrap_or(0)))
            } else {
                Ok(quote!(const { mod_points::snapshot::key(#fallback) }))
            }
        };
        // A node's move closures own only a copied reference to the native place, never
        // an enclosing Box, collection or aggregate reached by that place expression.
        let capture = |node: TokenStream| {
            quote!({
                let __horizon_place = &(#place);
                #node
            })
        };
        let place = quote!(*__horizon_place);
        let node = match shape {
            Shape::Scalar(Scalar::String | Scalar::Str) => {
                let unit = native_schema::identity(
                    rule,
                    &Shape::Scalar(Scalar::Unsigned("u16")),
                    exposure,
                )?;
                let unit_key = auxiliary("unit", &unit)?;
                quote!(mod_points::native_nodes::Text((#place).as_ref(), #key, #unit_key))
            }
            Shape::Scalar(Scalar::Signed("isize")) => {
                return Err("unimplemented: isize has no Wire encoding".into());
            }
            Shape::Scalar(_) | Shape::Unit => {
                quote!(mod_points::native_nodes::Scalar(&(#place), #key))
            }
            Shape::Ref { to, .. }
            | Shape::Wrapped(Wrapper::Box | Wrapper::Arc | Wrapper::Rc, to) => {
                emit(rule, to, quote!(*(#place)), exposure, depth + 1, runtime)?
            }
            Shape::Wrapped(Wrapper::Cow, to) => emit(
                rule,
                to,
                quote!(*(#place).as_ref()),
                exposure,
                depth + 1,
                runtime,
            )?,
            Shape::Array(_, _) | Shape::Slice(_) | Shape::Collection(Collection::Vec, _) => {
                let to = match shape {
                    Shape::Collection(_, args) => {
                        args.first().ok_or("unimplemented: malformed vector")?
                    }
                    Shape::Array(to, _) | Shape::Slice(to) => to,
                    _ => unreachable!(),
                };
                let child = emit(
                    rule,
                    to,
                    quote!(*__horizon_item),
                    exposure,
                    depth + 1,
                    runtime,
                )?;
                quote!(mod_points::native_nodes::sequence(&(#place), #key, |__horizon_item, __horizon_visit| __horizon_visit(mod_points::locators::Step::Index(0), 0, &#child)))
            }
            Shape::Collection(kind, args) => {
                let first = args.first().ok_or("unimplemented: malformed collection")?;
                let key_child = emit(
                    rule,
                    first,
                    quote!(*__horizon_first),
                    exposure,
                    depth + 1,
                    runtime,
                )?;
                let visit = match kind {
                    Collection::VecDeque | Collection::HashSet | Collection::BTreeSet => quote! {
                        let __horizon_first = (#place).iter().nth(usize::try_from(__horizon_index).map_err(|_| mod_api::RowError::Bounds)?).ok_or(mod_api::RowError::Bounds)?;
                        __horizon_visit(mod_points::locators::Step::Index(0), 0, &#key_child)
                    },
                    Collection::HashMap | Collection::BTreeMap => {
                        let second = args.get(1).ok_or("unimplemented: malformed map")?;
                        let value_child = emit(
                            rule,
                            second,
                            quote!(*__horizon_second),
                            exposure,
                            depth + 1,
                            runtime,
                        )?;
                        let pair = format!("{identity};pair");
                        let pair_key = auxiliary("pair", &pair)?;
                        quote! {
                            let (__horizon_first, __horizon_second) = (#place).iter().nth(usize::try_from(__horizon_index).map_err(|_| mod_api::RowError::Bounds)?).ok_or(mod_api::RowError::Bounds)?;
                            __horizon_visit(mod_points::locators::Step::Index(0), 0, &mod_points::native_nodes::Record { key: #pair_key, count: 2, visit: move |__horizon_index, __horizon_visit: &mut mod_points::native_nodes::Visitor<'_>| match __horizon_index {
                                0 => __horizon_visit(mod_points::locators::Step::Field(0), 0, &#key_child),
                                1 => __horizon_visit(mod_points::locators::Step::Field(1), 1, &#value_child),
                                _ => Err(mod_api::RowError::Bounds),
                            } })
                        }
                    }
                    Collection::Vec => unreachable!(),
                };
                quote!(mod_points::native_nodes::Collection { key: #key, count: (#place).len() as u64, visit: move |__horizon_index, __horizon_visit: &mut mod_points::native_nodes::Visitor<'_>| { #visit } })
            }
            Shape::Option(to) => {
                let child = emit(
                    rule,
                    to,
                    quote!(*__horizon_some),
                    exposure,
                    depth + 1,
                    runtime,
                )?;
                let some = format!("{identity};variant(1)");
                let none = format!("{identity};variant(0)");
                let some_key = auxiliary("variant(1)", &some)?;
                let none_key = auxiliary("variant(0)", &none)?;
                quote!({
                    let __horizon_tag: u32 = u32::from((#place).is_some());
                    mod_points::native_nodes::Variant { key: #key, tag: __horizon_tag, visit: move |__horizon_visit: &mut mod_points::native_nodes::Visitor<'_>| {
                        match &(#place) {
                            Some(__horizon_some) => __horizon_visit(mod_points::locators::Step::Variant(1), 1, &mod_points::native_nodes::Record { key: #some_key, count: 1, visit: move |__horizon_index, __horizon_visit: &mut mod_points::native_nodes::Visitor<'_>| match __horizon_index { 0 => __horizon_visit(mod_points::locators::Step::Field(0), 0, &#child), _ => Err(mod_api::RowError::Bounds) } }),
                            None => __horizon_visit(mod_points::locators::Step::Variant(0), 0, &mod_points::native_nodes::Record { key: #none_key, count: 0, visit: move |_, _: &mut mod_points::native_nodes::Visitor<'_>| Err(mod_api::RowError::Bounds) }),
                        }
                    } }
                })
            }
            Shape::Result(ok, err) => {
                let ok_child = emit(
                    rule,
                    ok,
                    quote!(*__horizon_payload),
                    exposure,
                    depth + 1,
                    runtime,
                )?;
                // An unprojectable payload stays a leaf; its case still reads, without a value.
                let err_child = emit(
                    rule,
                    err,
                    quote!(*__horizon_payload),
                    exposure,
                    depth + 1,
                    runtime,
                )?;
                let ok_key = auxiliary("variant(0)", &format!("{identity};variant(0)"))?;
                let err_key = auxiliary("variant(1)", &format!("{identity};variant(1)"))?;
                quote!({
                    let __horizon_tag: u32 = u32::from((#place).is_err());
                    mod_points::native_nodes::Variant { key: #key, tag: __horizon_tag, visit: move |__horizon_visit: &mut mod_points::native_nodes::Visitor<'_>| {
                        match &(#place) {
                            Ok(__horizon_payload) => __horizon_visit(mod_points::locators::Step::Variant(0), 0, &mod_points::native_nodes::Record { key: #ok_key, count: 1, visit: move |__horizon_index, __horizon_visit: &mut mod_points::native_nodes::Visitor<'_>| match __horizon_index { 0 => __horizon_visit(mod_points::locators::Step::Field(0), 0, &#ok_child), _ => Err(mod_api::RowError::Bounds) } }),
                            Err(__horizon_payload) => __horizon_visit(mod_points::locators::Step::Variant(1), 1, &mod_points::native_nodes::Record { key: #err_key, count: 1, visit: move |__horizon_index, __horizon_visit: &mut mod_points::native_nodes::Visitor<'_>| match __horizon_index { 0 => __horizon_visit(mod_points::locators::Step::Field(0), 0, &#err_child), _ => Err(mod_api::RowError::Bounds) } }),
                        }
                    } }
                })
            }
            Shape::DynError => {
                let unit = native_schema::identity(
                    rule,
                    &Shape::Scalar(Scalar::Unsigned("u16")),
                    exposure,
                )?;
                let unit_key = auxiliary("unit", &unit)?;
                quote!(mod_points::native_nodes::DisplayText((#place).to_string(), #key, #unit_key))
            }
            Shape::Tuple(parts) => {
                let mut arms = Vec::new();
                for (index, part) in parts.iter().enumerate() {
                    let member = syn::Index::from(index);
                    let field = index as u32;
                    let ordinal = index as u64;
                    let child = emit(
                        rule,
                        part,
                        quote!((#place).#member),
                        exposure,
                        depth + 1,
                        runtime,
                    )?;
                    arms.push(quote!(#ordinal => __horizon_visit(mod_points::locators::Step::Field(#field), #field, &#child),));
                }
                let count = parts.len() as u64;
                quote!(mod_points::native_nodes::Record { key: #key, count: #count, visit: move |__horizon_index, __horizon_visit: &mut mod_points::native_nodes::Visitor<'_>| match __horizon_index { #(#arms)* _ if __horizon_index < #count => Ok(()), _ => Err(mod_api::RowError::Bounds) } })
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
                        let mut local = 0u64;
                        for (index, field) in fields
                            .iter()
                            .enumerate()
                            .filter(|(_, f)| f.case.is_some_and(|c| c.0 == tag))
                        {
                            let member = &field.member;
                            let bind = quote::format_ident!("__horizon_field_{index}");
                            bindings.push(quote!(#member: #bind));
                            let ordinal = local;
                            local += 1;
                            if !exposure.field(&name_path, Some(tag), index as u32) {
                                continue;
                            }
                            let child_shape = rule.shape(&named.module, None, field.ty);
                            let child = match emit(
                                rule,
                                &child_shape,
                                quote!(*#bind),
                                exposure,
                                depth + 1,
                                runtime,
                            ) {
                                Ok(child) => child,
                                Err(_) if runtime.is_some() => continue,
                                Err(e) => return Err(e),
                            };
                            let field_id = ordinal as u32;
                            let guard = if let Some((graph, plan)) = runtime {
                                let id = graph
                                    .id(&Shape::Named(name_path.clone(), 0))
                                    .ok_or("missing runtime owner")?;
                                quote!((#plan).field(#id, Some(#tag), #field_id))
                            } else {
                                quote!(true)
                            };
                            arms.push(quote!(#ordinal if #guard => __horizon_visit(mod_points::locators::Step::Field(#field_id), #field_id, &#child),));
                        }
                        let count = local;
                        let variant_key = format!("{identity};variant({tag})");
                        let variant_key_expr = auxiliary(&format!("variant({tag})"), &variant_key)?;
                        variants.push(quote!(#path::#name { #(#bindings,)* .. } => __horizon_visit(mod_points::locators::Step::Variant(#tag), #tag, &mod_points::native_nodes::Record { key: #variant_key_expr, count: #count, visit: move |__horizon_index, __horizon_visit: &mut mod_points::native_nodes::Visitor<'_>| match __horizon_index { #(#arms)* _ if __horizon_index < #count => Ok(()), _ => Err(mod_api::RowError::Bounds) } }),));
                    }
                    let tags = item.variants.iter().enumerate().map(|(i, v)| {
                        let name = &v.ident;
                        let i = i as u32;
                        quote!(#path::#name { .. } => #i,)
                    });
                    return Ok(capture(
                        quote!({ let __horizon_tag = match &(#place) { #(#tags)* _ => u32::MAX }; mod_points::native_nodes::Variant { key: #key, tag: __horizon_tag, visit: move |__horizon_visit: &mut mod_points::native_nodes::Visitor<'_>| match &(#place) { #(#variants)* _ => Err(mod_api::RowError::Unsupported) } } }),
                    ));
                }
                let mut arms = Vec::new();
                let declared = rule.fields(path).len() as u64;
                for (index, field) in rule.fields(path).iter().enumerate() {
                    if !exposure.field(path, None, index as u32) {
                        continue;
                    }
                    let member = &field.member;

                    let shape = rule.shape(&named.module, Some(path), field.ty);
                    let child = match emit(
                        rule,
                        &shape,
                        quote!((#place).#member),
                        exposure,
                        depth + 1,
                        runtime,
                    ) {
                        Ok(child) => child,
                        Err(_) if runtime.is_some() => continue,
                        Err(e) => return Err(e),
                    };
                    let field_id = index as u32;
                    let ordinal = index as u64;
                    let guard = if let Some((graph, plan)) = runtime {
                        let id = graph
                            .id(&Shape::Named(path.to_string(), 0))
                            .ok_or("missing runtime owner")?;
                        quote!((#plan).field(#id, None, #field_id))
                    } else {
                        quote!(true)
                    };
                    arms.push(quote!(#ordinal if #guard => __horizon_visit(mod_points::locators::Step::Field(#field_id), #field_id, &#child),));
                }
                let count = declared;
                quote!(mod_points::native_nodes::Record { key: #key, count: #count, visit: move |__horizon_index, __horizon_visit: &mut mod_points::native_nodes::Visitor<'_>| match __horizon_index { #(#arms)* _ if __horizon_index < #count => Ok(()), _ => Err(mod_api::RowError::Bounds) } })
            }
            _ => {
                return Err(format!(
                    "unimplemented: native row projection for {shape:?}"
                ));
            }
        };
        Ok(capture(node))
    }
    emit(
        rule,
        &rule.shape(module, owner, ty),
        place,
        exposure,
        0,
        &runtime,
    )
}

/// One helper per named layout of the graph, emitted once per source rather than once per
/// point body. Each takes the native value and the endpoint's registration `Plan`. Only the
/// layouts `emit` marks (see `native_crate::emitted`) get one: the emitting crate's.
pub fn helpers(
    rule: &Rule,
    graph: &crate::native_policy::Graph,
    emit: &[bool],
) -> Result<TokenStream, String> {
    let mut helpers = Vec::new();
    for (id, layout) in graph.layouts.iter().enumerate() {
        if !emit[id] {
            continue;
        }
        let helper = quote::format_ident!("__horizon_node_{id}");
        let named: Type = syn::parse_str(&layout.name).map_err(|e| e.to_string())?;
        let native: Type = syn::parse_str(&layout.name.replace("horizon_client::", "crate::"))
            .map_err(|e| e.to_string())?;
        let inner = generate_inner(
            rule,
            "",
            None,
            &named,
            quote!(*__value),
            &native_schema::All,
            Some((graph, quote!(__policy))),
        )?;
        helpers.push(quote!(pub fn #helper<'a>(__value: &'a #native, __policy: &'a mod_points::exposure::Plan) -> impl mod_points::native_read::NativeNode + 'a { #inner }));
    }
    Ok(quote!(#(#helpers)*))
}
