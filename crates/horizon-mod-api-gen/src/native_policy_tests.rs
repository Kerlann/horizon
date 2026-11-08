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
