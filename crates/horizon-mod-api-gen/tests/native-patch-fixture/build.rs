use horizon_mod_api_gen::{crossing::Rule, internals::Internal, native_patch, native_schema::All};
use quote::quote;
use std::{collections::BTreeMap, fs, path::PathBuf};
fn main() {
    let entries = [("Data", "struct", "struct Data { list: Vec<u32>, choice: Choice, map: BTreeMap<u8,u32>, set: BTreeSet<u32> }"), ("Choice", "enum", "enum Choice { Empty, Value(u32) }")].map(|(name,kind,signature)| Internal { path: format!("horizon_client::model::{name}"), kind, signature: signature.into(), cfg: String::new(), hash: String::new(), shape_only: false, opaque_reason: None, refs: BTreeMap::from([("BTreeMap".into(), "std::collections::BTreeMap".into()),("BTreeSet".into(),"std::collections::BTreeSet".into())]) });
    let rule = Rule::new(&entries);
    let mut funcs = Vec::new();
    for (name, ty, writable) in [
        ("plan_data", "Data", true),
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
        let body = native_patch::generate(
            &rule,
            "horizon_client::model",
            None,
            &ty,
            quote!(*value),
            quote!(tree),
            quote!(*plan),
            writable,
            &All,
        )
        .unwrap();
        let native_ty = match name.to_string().as_str() {
            "plan_data" | "plan_readonly" => quote!(model::Data),
            "plan_ref_data" => quote!(&mut model::Data),
            _ => quote!(#ty),
        };
        funcs.push(quote!(fn #name<'a>(value: &'a mut #native_ty, tree: &mod_points::native_patch::PatchTree<'_>, plan: &mut mod_api::wire::PatchPlan<'a>) -> Result<(), mod_api::wire::WireError> { #body Ok(()) }));
    }
    fs::write(
        PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("patch.rs"),
        quote!(#(#funcs)*).to_string(),
    )
    .unwrap();
}
