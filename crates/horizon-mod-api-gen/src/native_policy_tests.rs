use super::*;
use crate::{
    crossing::tests::internal,
    exposure::{Grant, Plan},
};

#[test]
fn partial_receiver_nested_reachability_and_shared_schema() {
    let rule = Rule::new(&[
        internal(
            "horizon_client::Game",
            "struct",
            "struct Game { hidden: Secret, later: Nested }",
        ),
        internal(
            "horizon_client::Nested",
            "struct",
            "struct Nested { value: u16, back: Option<Box<Game>> }",
        ),
        internal(
            "horizon_client::Secret",
            "struct",
            "struct Secret { value: u32 }",
        ),
    ]);
    let shape = |name| rule.shape("horizon_client", None, &syn::parse_str(name).unwrap());
    let mut graph = Graph::default();
    let game = graph.add(&rule, &shape("Game"));
    let nested = graph.add(&rule, &shape("Nested"));
    let secret = graph.add(&rule, &shape("Secret"));
    let later = Plan::build(
        &graph.layouts,
        Some(game),
        &[Grant {
            case: None,
            ordinal: 1,
        }],
        &[],
    )
    .unwrap();
    assert!(!later.field(game, None, 0));
    assert!(later.field(game, None, 1));
    assert!(later.field(nested, None, 0));
    assert_eq!(later.key(secret), None);
    let args = Plan::build(&graph.layouts, Some(game), &[], &[nested]).unwrap();
    assert!(!args.field(game, None, 1));
    assert!(args.field(nested, None, 0));
    assert_ne!(args.key(game), later.key(game));
    let mut reordered = Graph::default();
    reordered.add(&rule, &shape("Secret"));
    let game2 = reordered.add(&rule, &shape("Game"));
    let same = Plan::build(
        &reordered.layouts,
        Some(game2),
        &[Grant {
            case: None,
            ordinal: 1,
        }],
        &[],
    )
    .unwrap();
    assert_eq!(later.key(game), same.key(game2));
    let mut changed = graph.layouts.clone();
    let scalar = graph
        .id(&crate::crossing::Shape::Scalar(
            crate::crossing::Scalar::Unsigned("u16"),
        ))
        .unwrap();
    changed[scalar].signature = "u32".into();
    let changed = Plan::build(
        &changed,
        Some(game),
        &[Grant {
            case: None,
            ordinal: 1,
        }],
        &[],
    )
    .unwrap();
    assert_ne!(later.key(game), changed.key(game));
    assert!(
        Plan::build(
            &graph.layouts,
            Some(game),
            &[Grant {
                case: None,
                ordinal: 2
            }],
            &[]
        )
        .is_err()
    );
}

#[test]
fn opaque_hidden_fields_are_not_reached_and_enum_ordinals_are_local() {
    let rule = Rule::new(&[
        internal(
            "horizon_client::Game",
            "struct",
            "struct Game { hidden: Unknown, later: u32 }",
        ),
        internal(
            "horizon_client::Choice",
            "enum",
            "enum Choice { A(u8, u16), B { value: u32 } }",
        ),
    ]);
    let mut graph = Graph::default();
    let root = graph.add(
        &rule,
        &rule.shape("horizon_client", None, &syn::parse_str("Game").unwrap()),
    );
    assert!(
        Plan::build(
            &graph.layouts,
            Some(root),
            &[Grant {
                case: None,
                ordinal: 1
            }],
            &[]
        )
        .is_ok()
    );
    assert!(
        Plan::build(
            &graph.layouts,
            Some(root),
            &[Grant {
                case: None,
                ordinal: 0
            }],
            &[]
        )
        .is_err()
    );
    let choice = graph.add(
        &rule,
        &rule.shape("horizon_client", None, &syn::parse_str("Choice").unwrap()),
    );
    let plan = Plan::build(
        &graph.layouts,
        Some(choice),
        &[Grant {
            case: Some(1),
            ordinal: 0,
        }],
        &[],
    )
    .unwrap();
    assert!(plan.field(choice, Some(1), 0));
    assert!(!plan.field(choice, Some(0), 0));
}

/// Helpers live in `crate::__horizon_points` and are called from the game's other modules, so each
/// emitted fn must not be private: a private helper is E0603 at every call site.
#[test]
fn staged_helpers_are_callable_from_sibling_modules() {
    let rule = Rule::new(&[internal(
        "horizon_client::Game",
        "struct",
        "struct Game { a: u16 }",
    )]);
    let mut graph = Graph::default();
    graph.add(
        &rule,
        &rule.shape("horizon_client", None, &syn::parse_str("Game").unwrap()),
    );
    let every: Vec<bool> = graph
        .layouts
        .iter()
        .map(|l| l.opaque.is_none() && rule.named(&l.name).is_some())
        .collect();
    for emitted in [
        crate::native_window::helpers(&rule, &graph, &every).unwrap(),
        crate::native_patch::helpers(&rule, &graph, &every).unwrap(),
    ] {
        let file: syn::File = syn::parse2(emitted).unwrap();
        let fns: Vec<_> = file
            .items
            .iter()
            .filter_map(|item| match item {
                syn::Item::Fn(item) => Some(item),
                _ => None,
            })
            .collect();
        assert!(!fns.is_empty());
        for item in fns {
            assert!(
                !matches!(item.vis, syn::Visibility::Inherited),
                "{} is private",
                item.sig.ident
            );
        }
    }
}

/// `isize` has no Wire impl, so a whole-value decode of anything holding one is a compile error
/// in the game; the decoder refuses it, and a patch root of that type takes no patch.
#[test]
fn isize_layouts_have_no_decoder_and_take_no_patch() {
    let rule = Rule::new(&[internal(
        "horizon_client::Cursor",
        "struct",
        "struct Cursor { column: isize }",
    )]);
    let cursor = rule.shape("horizon_client", None, &syn::parse_str("Cursor").unwrap());
    let scalar = rule.shape("horizon_client", None, &syn::parse_str("isize").unwrap());
    assert!(crate::native_value::decode_call(&rule, &cursor).is_err());
    assert!(crate::native_value::decode_call(&rule, &scalar).is_err());
    let mut graph = Graph::default();
    graph.add(&rule, &cursor);
    let patch = crate::native_patch::generate_runtime(
        &rule,
        "horizon_client",
        None,
        &syn::parse_str("isize").unwrap(),
        quote::quote!(**slot),
        quote::quote!(&trees[0]),
        quote::quote!(plan),
        true,
        &graph,
        quote::quote!(policy),
    )
    .unwrap()
    .to_string();
    assert!(!patch.contains("take"), "{patch}");
}
