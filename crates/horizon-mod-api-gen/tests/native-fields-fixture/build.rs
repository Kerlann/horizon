use horizon_mod_api_gen::{crossing::Rule, internals::Internal, native_fields::generate};
use quote::quote;

fn main() {
    let internal = Internal {
        path: "horizon_client::Thing".into(),
        kind: "struct",
        cfg: String::new(),
        signature: "struct Thing { number: u32, nested: (bool, [i16; 2]), readonly: &'static u8 }"
            .into(),
        hash: String::new(),
        shape_only: false,
        opaque_reason: None,
        refs: Default::default(),
    };
    let rule = Rule::new(&[internal]);
    let p = generate(
        &rule,
        "horizon_client",
        None,
        &syn::parse_str("Thing").unwrap(),
        quote!(*value),
        quote!(builder),
        1,
        Some(0),
        4,
        true,
    )
    .unwrap();
    let snapshot = p.snapshot;
    let patch = p.patch;
    let code = quote! {
        fn snapshot(value: &Thing) -> Result<mod_points::snapshot::Built, mod_api::wire::WireError> {
            let mut builder = mod_points::snapshot::Builder::default();
            builder.node(None, 0, const { mod_points::snapshot::key("call") })?;
            #snapshot?;
            Ok(builder.finish())
        }
        fn apply(value: &mut Thing, bytes: &[u8]) -> Result<(), mod_api::wire::WireError> {
            use mod_api::wire::Record;
            #patch.apply(bytes)
        }
    };
    std::fs::write(
        std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("projection.rs"),
        code.to_string(),
    )
    .unwrap();
}
