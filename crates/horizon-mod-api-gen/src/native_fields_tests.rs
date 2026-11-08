use super::*;
use crate::crossing::tests::internal;
use quote::quote;

fn project(rule: &Rule, ty: &str, writable: bool) -> Result<Projection, String> {
    generate(
        rule,
        "horizon_client",
        None,
        &syn::parse_str(ty).unwrap(),
        quote!(value),
        quote!(builder),
        7,
        Some(0),
        3,
        writable,
    )
}

#[test]
fn preorder_ids_and_sparse_writes_share_a_layout() {
    let rule = Rule::new(&[internal(
        "horizon_client::Thing",
        "struct",
        "struct Thing { a: u32, b: (bool, [i16; 2]), c: &'static u8 }",
    )]);
    let p = project(&rule, "Thing", true).unwrap();
    assert_eq!(p.nodes, 8);
    assert_eq!(
        p.identity,
        "struct(horizon_client::Thing;a:u32,b:tuple(bool,array(2;i16)),c:u8)"
    );
    let source = p.snapshot.to_string();
    let parsed: syn::Expr = syn::parse2(p.snapshot).unwrap();
    assert!(matches!(parsed, syn::Expr::Call(_)));
    assert_eq!(source.matches(". value").count(), 6);
    assert!(source.contains("Some (7u32)"));
    let patch = p.patch.to_string();
    for id in [8, 10, 12, 13] {
        assert!(patch.contains(&format!("new ({id}u32")), "{patch}");
    }
    assert!(!patch.contains("new (14u32"));
    assert!(patch.contains("let [__horizon_array_11_0 , __horizon_array_11_1]"));
    syn::parse2::<syn::Expr>(p.patch).unwrap();
}

#[test]
fn identities_track_layout_and_unimplemented_cases_are_explicit() {
    let make = |signature| Rule::new(&[internal("horizon_client::Thing", "struct", signature)]);
    let a = make("struct Thing { a: u8 }");
    let b = make("struct Thing { a: u16 }");
    assert_ne!(
        project(&a, "Thing", false).unwrap().identity,
        project(&b, "Thing", false).unwrap().identity
    );
    for ty in [
        "Vec<u8>",
        "Option<u8>",
        "isize",
        "String",
        "&str",
        "Unlisted",
    ] {
        assert!(
            project(&a, ty, true)
                .err()
                .unwrap()
                .starts_with("unimplemented:"),
            "{ty}"
        );
    }
    let guarded = make("struct Thing { #[cfg(feature = \"x\")] a: u8 }");
    assert!(project(&guarded, "Thing", true).is_err());
}
