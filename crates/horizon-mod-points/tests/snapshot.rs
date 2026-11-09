// Generated fixtures exercise every shape; TODO(#2273): make the generator emit clippy-clean code.
#![allow(clippy::all, unused_comparisons, unreachable_patterns, dead_code)]
use mod_api::wire::{Reader, Writer};
use mod_points::snapshot::{Builder, Rows, Snapshot, key};

#[test]
fn snapshot_rows_are_owned_and_read_only_after_the_call() {
    let mut builder = Builder::default();
    let root = builder.node(None, 0, key("fixture::Call")).unwrap();
    let mut live = String::from("before");
    let text = builder.value(Some(root), 0, key("String"), &live).unwrap();
    let number = builder.value(Some(root), 1, key("u32"), &17u32).unwrap();
    let rows = builder.finish();
    live.clear();
    let mut bytes = Writer::new();
    bytes.put(&rows);
    let rows: Rows = Reader::decode(bytes.bytes()).unwrap();
    let snapshot = Snapshot::new(&rows);
    assert_eq!(snapshot.field(root, 0), Some(text));
    assert_eq!(snapshot.ty(text), Some(key("String")));
    assert_eq!(snapshot.read::<String>(text).unwrap(), "before");
    assert_eq!(snapshot.read::<u32>(number).unwrap(), 17);
    assert!(snapshot.field(root, 99).is_none());
    assert!(snapshot.read::<u32>(u32::MAX).is_err());
}

#[test]
fn snapshot_budget_refusal_does_not_publish_a_partial_row() {
    let mut builder = Builder::default();
    let root = builder.node(None, 0, key("R")).unwrap();
    assert!(builder.node(Some(99), 0, key("u32")).is_err());
    assert!(
        builder
            .value(Some(root), 0, key("String"), &"x".repeat(1_048_576))
            .is_err()
    );
    assert_eq!(builder.value(Some(root), 0, key("bool"), &true).unwrap(), 1);
    let rows = builder.finish();
    assert_eq!(rows.len(), 2);
    assert!(Snapshot::new(&rows).read::<bool>(1).unwrap());
}
