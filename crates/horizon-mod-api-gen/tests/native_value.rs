use horizon_mod_api_gen::{
    crossing::{Rule, Shape},
    internals::Internal,
    native_value::codec,
};
use std::collections::BTreeMap;

fn entry(path: &str, kind: &'static str, signature: &str) -> Internal {
    Internal {
        path: path.into(),
        kind,
        signature: signature.into(),
        cfg: String::new(),
        hash: String::new(),
        shape_only: false,
        opaque_reason: None,
        refs: BTreeMap::new(),
    }
}

#[test]
fn nominal_codecs_emit_parseable_tagged_records_and_reject_inaccessible_values() {
    let entries = [
        entry(
            "horizon_client::demo::Thing",
            "struct",
            "struct Thing { number: u32, enabled: bool }",
        ),
        entry(
            "horizon_client::demo::Choice",
            "enum",
            "enum Choice { Empty, Pair(u32, bool), Named { item: u8 } }",
        ),
    ];
    let rule = Rule::new(&entries);
    for path in [
        "horizon_client::demo::Thing",
        "horizon_client::demo::Choice",
    ] {
        let c = codec(&rule, &Shape::Named(path.into(), 0)).unwrap();
        let decode = c.decode;
        let encode = c.encode;
        let _: syn::Expr = syn::parse2(quote::quote!({ #decode })).unwrap();
        let _: syn::Expr = syn::parse2(quote::quote!({ #encode })).unwrap();
        if path.ends_with("Choice") {
            assert!(decode.to_string().contains("unknown enum variant"));
        }
    }
    assert!(
        codec(
            &rule,
            &Shape::Ref {
                mutable: false,
                to: Box::new(Shape::Unit)
            }
        )
        .is_err()
    );
    let foreign = [entry(
        "foreign::Private",
        "struct",
        "pub struct Private(u32);",
    )];
    assert!(
        codec(
            &Rule::new(&foreign),
            &Shape::Named("foreign::Private".into(), 0)
        )
        .is_err()
    );
}
