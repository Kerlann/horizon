use horizon_mod_api_gen::{crossing::Rule, internals::Internal, native_patch};
use quote::quote;
use std::{collections::BTreeMap, fs, path::PathBuf};
fn main() {
    let entries = [("Data", "struct", "struct Data { list: Vec<u32>, choice: Choice, map: BTreeMap<u8,u32>, set: BTreeSet<u32> }"), ("Choice", "enum", "enum Choice { Empty, Value(u32) }")].map(|(name,kind,signature)| Internal { path: format!("horizon_client::model::{name}"), kind, signature: signature.into(), cfg: String::new(), hash: String::new(), shape_only: false, opaque_reason: None, refs: BTreeMap::from([("BTreeMap".into(), "std::collections::BTreeMap".into()),("BTreeSet".into(),"std::collections::BTreeSet".into())]) });
    let mut entries = Vec::from(entries);
    for (name, signature, opaque) in [
        (
            "Game",
            "struct Game { hidden: Opaque, later: Leaf, other: u32 }",
            None,
        ),
        ("Opaque", "struct Opaque { value: u32 }", Some("opaque")),
        ("Leaf", "struct Leaf { value: u32 }", None),
    ] {
        entries.push(Internal {
            path: format!("horizon_client::model::{name}"),
            kind: "struct",
            signature: signature.into(),
            cfg: String::new(),
            hash: String::new(),
            shape_only: false,
            opaque_reason: opaque,
            refs: BTreeMap::new(),
        });
    }
    let rule = Rule::new(&entries);
    let ty: syn::Type = syn::parse_str("Game").unwrap();
    let shape = rule.shape("horizon_client::model", None, &ty);
    let mut graph = horizon_mod_api_gen::native_policy::Graph::default();
    let root = graph.add(&rule, &shape);
    let layouts = graph.tokens();
    let body = native_patch::generate_runtime(
        &rule,
        "horizon_client::model",
        None,
        &ty,
        quote!(*value),
        quote!(tree),
        quote!(*plan),
        true,
        &graph,
        quote!(policy),
    )
    .unwrap();
    let helpers = native_patch::helpers(&rule, &graph, &graph.emittable(&rule)).unwrap();
    let runtime = quote! {
        fn policy() -> mod_points::exposure::Plan { mod_points::exposure::Plan::build(&#layouts, Some(#root), &[mod_points::exposure::Grant { case: None, ordinal: 1 }], &[]).unwrap() }
        fn plan_game<'a>(value: &'a mut model::Game, tree: &mod_points::native_patch::PatchTree<'_>, plan: &mut mod_api::wire::PatchPlan<'a>, policy: &mod_points::exposure::Plan) -> Result<(), mod_api::wire::WireError> { #helpers #body Ok(()) }
    };
    let mut funcs = Vec::new();
    for (name, ty, writable) in [
        ("plan_data", "Data", true),
        ("plan_opaque", "Opaque", true),
        ("plan_readonly", "Data", false),
        ("plan_shared", "&u32", true),
        ("plan_ref", "&mut u32", true),
        ("plan_ref_data", "&mut Data", true),
        (
            "plan_complex",
            "(Box<Option<[u32; 2]>>, std::collections::VecDeque<u32>)",
            true,
        ),
    ] {
        let name = quote::format_ident!("{name}");
        let ty: syn::Type = syn::parse_str(ty).unwrap();
        let mut graph = horizon_mod_api_gen::native_policy::Graph::default();
        let root = graph.add(&rule, &rule.shape("horizon_client::model", None, &ty));
        let layouts = graph.tokens();
        let body = native_patch::generate_runtime(
            &rule,
            "horizon_client::model",
            None,
            &ty,
            quote!(*value),
            quote!(tree),
            quote!(*plan),
            writable,
            &graph,
            quote!(policy),
        )
        .unwrap();
        let helpers = native_patch::helpers(&rule, &graph, &graph.emittable(&rule)).unwrap();
        let native_ty = match name.to_string().as_str() {
            "plan_data" | "plan_readonly" => quote!(model::Data),
            "plan_opaque" => quote!(model::Opaque),
            "plan_ref_data" => quote!(&mut model::Data),
            _ => quote!(#ty),
        };
        funcs.push(quote!(fn #name<'a>(value: &'a mut #native_ty, tree: &mod_points::native_patch::PatchTree<'_>, plan: &mut mod_api::wire::PatchPlan<'a>) -> Result<(), mod_api::wire::WireError> { let policy = mod_points::exposure::Plan::build(&#layouts, None, &[], &[#root]).unwrap(); #helpers #body Ok(()) }));
    }
    fs::write(
        PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("patch.rs"),
        quote!(#runtime #(#funcs)*).to_string(),
    )
    .unwrap();
}
