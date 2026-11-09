// Generated fixtures exercise every shape; TODO(#2273): make the generator emit clippy-clean code.
#![allow(clippy::all, unused_comparisons, unreachable_patterns, dead_code)]
use mod_points::endpoint_policy::{
    EndpointPolicy, InternalDeclaration, InternalField, InternalsRefusal, PointSpec, SchemaCatalog,
};
use mod_points::exposure::{Field, Layout};
use std::sync::Arc;

fn layout(name: &str, fields: Vec<Field>, opaque: bool) -> Layout {
    Layout {
        name: name.into(),
        signature: name.into(),
        fields,
        children: vec![],
        opaque: opaque.then(|| "opaque".into()),
    }
}
fn field(name: &str, ordinal: u32, target: usize) -> Field {
    Field {
        case: None,
        ordinal,
        name: name.into(),
        target,
    }
}
fn catalog() -> SchemaCatalog {
    SchemaCatalog {
        layouts: vec![
            layout(
                "receiver",
                vec![field("visible", 0, 1), field("secret", 1, 2)],
                false,
            ),
            layout("scalar", vec![], false),
            layout("opaque", vec![], true),
            layout("call", vec![field("root0", 0, 0)], false),
        ],
        points: vec![PointSpec {
            head_call: 3,
            tail_call: None,
            receiver: Some(0),
        }],
        fields: vec![
            InternalField {
                path: "Game::visible".into(),
                kind: "field".into(),
                hash: "abc".into(),
                receiver: 0,
                field: "visible".into(),
            },
            InternalField {
                path: "Game::secret".into(),
                kind: "field".into(),
                hash: "def".into(),
                receiver: 0,
                field: "secret".into(),
            },
        ],
    }
}
fn declaration(path: &str, hash: &str) -> InternalDeclaration {
    InternalDeclaration {
        path: path.into(),
        kind: Some("field".into()),
        hash: hash.into(),
    }
}
#[test]
fn grants_are_per_module_validated_and_freed() {
    let c = catalog();
    let empty = EndpointPolicy::build(&c, &[], &[3]).unwrap();
    let granted = EndpointPolicy::build(&c, &[declaration("Game::visible", "abc")], &[3]).unwrap();
    let a = empty.plan(3).unwrap();
    let b = granted.plan(3).unwrap();
    assert!(!a.field(0, None, 0));
    assert!(b.field(0, None, 0));
    assert!(!b.field(0, None, 1));
    assert_ne!(a.key(0), b.key(0));
    let weak = Arc::downgrade(&b);
    drop(b);
    drop(granted);
    assert!(weak.upgrade().is_none());
}

fn refusal(c: &SchemaCatalog, d: InternalDeclaration, listened: &[usize]) -> InternalsRefusal {
    match EndpointPolicy::build(c, &[d], listened) {
        Err(why) => why,
        Ok(_) => panic!("declaration was accepted"),
    }
}
#[test]
fn invalid_declarations_get_typed_refusals() {
    let c = catalog();
    let path = |p: &str| p.to_string();
    assert_eq!(
        refusal(&c, declaration("Game::secret", "def"), &[3]),
        InternalsRefusal::OpaqueReceiverMember(path("Game::secret"))
    );
    assert_eq!(
        refusal(&c, declaration("Game::missing", "abc"), &[3]),
        InternalsRefusal::UnknownPath(path("Game::missing"))
    );
    assert_eq!(
        refusal(&c, declaration("Game::visible", "bad"), &[3]),
        InternalsRefusal::HashMismatch(path("Game::visible"))
    );
    let mut method = declaration("Game::visible", "abc");
    method.kind = Some("method".into());
    assert_eq!(
        refusal(&c, method, &[3]),
        InternalsRefusal::NotAField(path("Game::visible"))
    );
    assert_eq!(
        refusal(&c, declaration("Game::visible", "abc"), &[]),
        InternalsRefusal::NoListener(path("Game::visible"))
    );
    assert_eq!(
        refusal(&c, declaration("Game::visible", "abc"), &[9]),
        InternalsRefusal::UnknownPoint(9)
    );
    // The same path under two kinds is ambiguous without a declared kind.
    let mut ambiguous = catalog();
    let mut twin = ambiguous.fields[0].clone();
    twin.kind = "method".into();
    ambiguous.fields.push(twin);
    let mut open = declaration("Game::visible", "abc");
    open.kind = None;
    assert_eq!(
        refusal(&ambiguous, open, &[3]),
        InternalsRefusal::AmbiguousKind(path("Game::visible"))
    );
}
