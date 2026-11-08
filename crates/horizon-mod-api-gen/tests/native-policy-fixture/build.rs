use horizon_mod_api_gen::{
    crossing::Rule, internals::Internal, native_policy::Graph, native_window,
};
use quote::quote;
use std::{collections::BTreeMap, fs, path::PathBuf};
fn main() {
    let entries = [
        (
            "Game",
            "struct Game { hidden: Unknown, later: Nested, other: u32 }",
        ),
        (
            "Nested",
            "struct Nested { value: u16, back: Option<Box<Game>> }",
        ),
    ]
    .map(|(name, signature)| Internal {
        path: format!("horizon_client::model::{name}"),
        kind: "struct",
        signature: signature.into(),
        cfg: String::new(),
        hash: String::new(),
        shape_only: false,
        opaque_reason: None,
        refs: BTreeMap::new(),
    });
    let rule = Rule::new(&entries);
    let ty = syn::parse_str("Game").unwrap();
    let mut graph = Graph::default();
    let root = graph.add(&rule, &rule.shape("horizon_client::model", None, &ty));
    let metadata = graph.tokens();
    let body = native_window::generate_runtime(
        &rule,
        "horizon_client::model",
        None,
        &ty,
        quote!(*value),
        &graph,
        quote!(plan),
    )
    .unwrap();
    fs::write(PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("reader.rs"), quote!(fn layouts() -> Vec<mod_points::exposure::Layout> { #metadata } const ROOT: usize = #root; fn reader<'a>(value: &'a model::Game, plan: &'a mod_points::exposure::Plan) -> impl mod_points::native_read::NativeNode + 'a { #body }).to_string()).unwrap();
}
