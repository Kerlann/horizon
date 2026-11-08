//! Rule-derived sparse native edit planning.
use crate::{
    crossing::{Collection, Rule, Shape, Wrapper},
    native_schema::Exposure,
    native_value,
};
use proc_macro2::TokenStream;
use quote::quote;
use syn::Type;

/// Emit a fallible planning block. `tree` is a PatchTree subtree for this exact root;
/// `plan` is a caller-owned PatchPlan shared by all roots. Commit only after all blocks succeed.
/// The producer's authorization is authoritative. No native pointers are retained.
/// The caller validates the lease, builds all root trees before borrowing native places, and
/// propagates every planning error before calling commit. Plans remain inside this immediate
/// apply transaction. Sequence and map descendants currently scan the live collection once;
/// they do not allocate a destination or clone a value for every unmodified element.
/// Recursive layouts and wrappers other than Box are explicit generation gaps.
pub fn generate(
    rule: &Rule,
    module: &str,
    owner: Option<&str>,
    ty: &Type,
    place: TokenStream,
    tree: TokenStream,
    plan: TokenStream,
    writable: bool,
    exposure: &dyn Exposure,
) -> Result<TokenStream, String> {
    if !writable {
        return Ok(
            quote!(if !(#tree).is_empty() { return Err(mod_api::wire::WireError::Invalid("readonly patch root")); }),
        );
    }
    let body = emit(
        rule,
        &rule.shape(module, owner, ty),
        quote!(*__np_value),
        exposure,
        None,
        0,
    )?;
    Ok(
        quote!({ let __np_value = &mut (#place); let __np_tree = #tree; let __np_plan = &mut (#plan); #body; }),
    )
}
/// Emit a sparse planner gated by the registration-time runtime exposure plan.
pub fn generate_runtime(
    rule: &Rule,
    module: &str,
    owner: Option<&str>,
    ty: &Type,
    place: TokenStream,
    tree: TokenStream,
    plan: TokenStream,
    writable: bool,
    graph: &crate::native_policy::Graph,
    policy: TokenStream,
) -> Result<TokenStream, String> {
    if !writable {
        return Ok(
            quote!(if !(#tree).is_empty() { return Err(mod_api::wire::WireError::Invalid("readonly patch root")); }),
        );
    }
    let body = emit(
        rule,
        &rule.shape(module, owner, ty),
        quote!(*__np_value),
        &crate::native_schema::All,
        Some((graph, &policy)),
        0,
    )
    .unwrap_or_else(|_| {
        quote!(if !__np_tree.is_empty() {
            return Err(mod_api::wire::WireError::Invalid(
                "unsupported native patch root",
            ));
        })
    });
    Ok(
        quote!({ let __np_value = &mut (#place); let __np_tree = #tree; let __np_plan = &mut (#plan); #body; }),
    )
}
/// Generate runtime patch planners once per owning crate.
pub fn helpers(
    rule: &Rule,
    graph: &crate::native_policy::Graph,
) -> Result<Vec<(String, TokenStream)>, String> {
    let mut result = Vec::new();
    for (id, layout) in graph.layouts.iter().enumerate() {
        if rule.named(&layout.name).is_none() || layout.opaque.is_some() {
            continue;
        }
        let owner = layout.name.split("::").next().ok_or("missing crate")?;
        let native: Type =
            syn::parse_str(&layout.name.replacen(owner, "crate", 1)).map_err(|e| e.to_string())?;
        let shape = rule.shape(
            "",
            None,
            &syn::parse_str::<Type>(&layout.name).map_err(|e| e.to_string())?,
        );
        let helper = quote::format_ident!("__horizon_patch_{id}");
        let body = emit(
            rule,
            &shape,
            quote!(*__np_value),
            &crate::native_schema::All,
            Some((graph, &quote!(__policy))),
            0,
        )?;
        result.push((owner.to_string(), quote!(fn #helper<'a>(__np_value: &'a mut #native, __np_tree: &mod_points::native_patch::PatchTree<'_>, __np_plan: &mut mod_api::wire::PatchPlan<'a>, __policy: &mod_points::exposure::Plan) -> Result<(), mod_api::wire::WireError> { #body; Ok(()) })));
    }
    Ok(result)
}
fn emit(
    rule: &Rule,
    shape: &Shape,
    place: TokenStream,
    exposure: &dyn Exposure,
    runtime: Option<(&crate::native_policy::Graph, &TokenStream)>,
    depth: usize,
) -> Result<TokenStream, String> {
    if depth > 128 {
        return Err("unimplemented: recursive patch shape".into());
    }
    if depth > 0 {
        if let (Shape::Named(_, _), Some((graph, policy))) = (shape, runtime) {
            let id = graph.id(shape).ok_or("missing runtime patch layout")?;
            let helper = quote::format_ident!("__horizon_patch_{id}");
            return Ok(quote!(#helper(&mut (#place), __np_tree, __np_plan, &(#policy))?;));
        }
    }
    let reject = quote!(
        return Err(mod_api::wire::WireError::Invalid(
            "invalid or readonly patch path"
        ))
    );
    if matches!(shape, Shape::Ref { mutable: false, .. }) {
        return Ok(quote!(if !__np_tree.is_empty() { #reject; }));
    }
    let codec = if fully_exposed(rule, shape, exposure, 0) {
        native_value::codec(rule, shape)
    } else {
        Err("partial exposure forbids whole-value replacement".into())
    };
    let whole = match codec {
        Ok(codec) => {
            let decode = codec.decode;
            quote!(__np_plan.prepare(&mut (#place), bytes, |input| Ok(#decode))?;)
        }
        Err(_) => quote!(#reject;),
    };
    let whole = if let Some((graph, policy)) = runtime {
        if let Some(id) = graph.id(shape) {
            quote!(if !(#policy).complete(#id) { #reject; } #whole)
        } else {
            quote!(#reject;)
        }
    } else {
        whole
    };
    let child = |s: &Shape, p| {
        emit(rule, s, p, exposure, runtime, depth + 1)
            .or_else(|_| Ok::<_, String>(quote!(#reject;)))
    };
    let descendants = match shape {
        Shape::Ref { mutable: true, to } | Shape::Wrapped(Wrapper::Box, to) => {
            let body = child(to, quote!(*(#place)))?;
            // References and Box are transparent in the reader path; do not consume a step.
            return Ok(if matches!(shape, Shape::Ref { .. }) {
                body
            } else {
                quote!(if let Some(bytes) = __np_tree.value() { #whole } else { #body })
            });
        }
        Shape::Tuple(parts) => {
            let mut blocks = Vec::new();
            for (i, s) in parts.iter().enumerate() {
                let member = syn::Index::from(i);
                let id = i as u32;
                let body = child(s, quote!((#place).#member))?;
                blocks.push(quote!(if let Some(__np_tree) = __np_tree.child(mod_points::locators::Step::Field(#id)) { #body }));
            }
            let len = parts.len() as u32;
            quote!(for (step,_) in __np_tree.children() { if !matches!(step,mod_points::locators::Step::Field(id) if id < #len) { #reject; } } #(#blocks)*)
        }
        Shape::Array(s, _) | Shape::Slice(s) => {
            sequence(child(s, quote!(*__np_item))?, place.clone(), reject.clone())
        }
        Shape::Collection(Collection::Vec | Collection::VecDeque, args) => sequence(
            child(&args[0], quote!(*__np_item))?,
            place.clone(),
            reject.clone(),
        ),
        Shape::Collection(Collection::HashMap | Collection::BTreeMap, args) => {
            let body = child(&args[1], quote!(*__np_item))?;
            quote! {
                for (step, tree) in __np_tree.children() {
                    let mod_points::locators::Step::Index(index) = step else { #reject; };
                    if index >= (#place).len() as u64 || tree.value().is_some() { #reject; }
                    for (step,_) in tree.children() { if step != mod_points::locators::Step::Field(1) { #reject; } }
                }
                for (__np_index, (_, __np_item)) in (#place).iter_mut().enumerate() {
                    if let Some(pair) = __np_tree.child(mod_points::locators::Step::Index(__np_index as u64)) {
                        if let Some(__np_tree) = pair.child(mod_points::locators::Step::Field(1)) { #body }
                    }
                }
            }
        }
        Shape::Option(s) => {
            let body = child(s, quote!(*__np_some))?;
            quote! {
                let tag = u32::from((#place).is_some());
                for (step, tree) in __np_tree.children() {
                    if step != mod_points::locators::Step::Variant(tag) || tree.value().is_some() { #reject; }
                    for (step,_) in tree.children() { if tag != 1 || step != mod_points::locators::Step::Field(0) { #reject; } }
                }
                if let Some(__np_some) = &mut (#place) {
                    if let Some(variant) = __np_tree.child(mod_points::locators::Step::Variant(1)) {
                        if let Some(__np_tree) = variant.child(mod_points::locators::Step::Field(0)) { #body }
                    }
                }
            }
        }
        Shape::Named(path, _) => {
            if runtime.is_some_and(|(graph, _)| graph.id(shape).is_none()) {
                return Err("missing runtime patch layout".into());
            }
            let named = rule.named(path).ok_or("missing patch layout")?;
            if named.guarded || named.opaque_reason.is_some() {
                return Err(format!(
                    "unimplemented: inaccessible native patch layout {path}"
                ));
            }
            let fields = rule.fields(path);
            if let syn::Item::Enum(item) = &named.item {
                let ty: syn::Path = syn::parse_str(&path.replace("horizon_client::", "crate::"))
                    .map_err(|e| e.to_string())?;
                let mut arms = Vec::new();
                for (case, v) in item.variants.iter().enumerate() {
                    let tag = case as u32;
                    let name = &v.ident;
                    let mut binds = Vec::new();
                    let mut blocks = Vec::new();
                    let mut ids = Vec::new();
                    for (i, f) in fields
                        .iter()
                        .enumerate()
                        .filter(|(_, f)| f.case.is_some_and(|c| c.0 == tag))
                    {
                        if !exposure.field(path, Some(tag), i as u32) {
                            continue;
                        }
                        let id = i as u32;
                        ids.push(id);
                        let member = &f.member;
                        let bind = quote::format_ident!("__np_field_{i}");
                        binds.push(quote!(#member: #bind));
                        let s = rule.shape(&named.module, None, f.ty);
                        let body = child(&s, quote!(*#bind))?;
                        let ordinal = fields[..i]
                            .iter()
                            .filter(|f| f.case.is_some_and(|c| c.0 == tag))
                            .count() as u32;
                        let gate = runtime.and_then(|(graph, policy)| graph.id(shape).map(|layout| quote!(if !(#policy).field(#layout, Some(#tag), #ordinal) { #reject; })));
                        blocks.push(quote!(if let Some(__np_tree)=variant.child(mod_points::locators::Step::Field(#id)) { #gate #body }));
                    }
                    arms.push(quote!(#ty::#name { #(#binds,)* .. } => {
                        for (step,variant) in __np_tree.children() {
                            if step != mod_points::locators::Step::Variant(#tag) || variant.value().is_some() { #reject; }
                            for (step,_) in variant.children() { if ![#(mod_points::locators::Step::Field(#ids)),*].contains(&step) { #reject; } }
                        }
                        if let Some(variant)=__np_tree.child(mod_points::locators::Step::Variant(#tag)) { #(#blocks)* }
                    }));
                }
                let preflight = runtime.and_then(|(graph, policy)| graph.id(shape).map(|layout| {
                    let checks = fields.iter().enumerate().map(|(i, f)| {
                        let tag = f.case.unwrap().0;
                        let ordinal = fields[..i].iter().filter(|f| f.case.is_some_and(|c| c.0 == tag)).count() as u32;
                        let id = i as u32;
                        quote!(if variant.child(mod_points::locators::Step::Field(#id)).is_some() && !(#policy).field(#layout, Some(#tag), #ordinal) { #reject; })
                    });
                    quote!(for (_, variant) in __np_tree.children() { #(#checks)* })
                }));
                quote!(#preflight match &mut (#place) { #(#arms,)* _ => { #reject; } })
            } else {
                let mut blocks = Vec::new();
                let mut ids = Vec::new();
                for (i, f) in fields.iter().enumerate() {
                    if !exposure.field(path, None, i as u32) {
                        continue;
                    }
                    let id = i as u32;
                    ids.push(id);
                    let member = &f.member;
                    let s = rule.shape(&named.module, Some(path), f.ty);
                    let body = child(&s, quote!((#place).#member))?;
                    let gate = runtime.and_then(|(graph, policy)| {
                        graph.id(shape).map(
                            |layout| quote!(if !(#policy).field(#layout, None, #id) { #reject; }),
                        )
                    });
                    blocks.push(quote!(if let Some(__np_tree)=__np_tree.child(mod_points::locators::Step::Field(#id)) { #gate #body }));
                }
                quote!(for (step,_) in __np_tree.children() { if ![#(mod_points::locators::Step::Field(#ids)),*].contains(&step) { #reject; } } #(#blocks)*)
            }
        }
        Shape::Scalar(_)
        | Shape::Unit
        | Shape::Ref { mutable: false, .. }
        | Shape::Collection(Collection::HashSet | Collection::BTreeSet, _) => {
            quote!(if __np_tree.children().next().is_some() { #reject; })
        }
        _ => return Err(format!("unimplemented: native patch planner for {shape:?}")),
    };
    Ok(quote!(if let Some(bytes)=__np_tree.value() { #whole } else { #descendants }))
}
fn fully_exposed(rule: &Rule, shape: &Shape, exposure: &dyn Exposure, depth: usize) -> bool {
    if depth > 128 {
        return false;
    }
    let child = |shape: &Shape| fully_exposed(rule, shape, exposure, depth + 1);
    match shape {
        Shape::Named(path, _) => rule.named(path).is_some_and(|named| {
            rule.fields(path).iter().enumerate().all(|(i, field)| {
                exposure.field(path, field.case.map(|c| c.0), i as u32)
                    && child(&rule.shape(&named.module, Some(path), field.ty))
            })
        }),
        Shape::Tuple(parts) | Shape::Collection(_, parts) => parts.iter().all(child),
        Shape::Array(inner, _)
        | Shape::Slice(inner)
        | Shape::Option(inner)
        | Shape::Wrapped(_, inner)
        | Shape::Ref { to: inner, .. } => child(inner),
        _ => true,
    }
}
fn sequence(body: TokenStream, place: TokenStream, reject: TokenStream) -> TokenStream {
    quote! {
        for (step,_) in __np_tree.children() { let mod_points::locators::Step::Index(index)=step else { #reject; }; if index >= (#place).len() as u64 { #reject; } }
        for (__np_index,__np_item) in (#place).iter_mut().enumerate() {
            if let Some(__np_tree)=__np_tree.child(mod_points::locators::Step::Index(__np_index as u64)) { #body }
        }
    }
}
