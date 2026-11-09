// Generated fixtures exercise every shape; TODO(#2273): make the generator emit clippy-clean code.
#![allow(clippy::all, unused_comparisons, unreachable_patterns, dead_code)]
use mod_api::{RowWindow, wire::WireError};
use mod_points::snapshot::{Builder, Snapshot, Window, key};

#[test]
fn sparse_source_ids_and_ancestor_parents_survive_owned_windows() {
    let mut out = Window::try_from(RowWindow {
        node: 700,
        start: 0,
        rows: 3,
        depth: 1,
    })
    .unwrap()
    .builder();
    out.external(700, Some(69), 2, key("tuple"), 0, None)
        .unwrap();
    out.external(42, Some(700), 0, key("u32"), 1, Some(&81u32))
        .unwrap();
    let rows = out.finish();
    let view = Snapshot::new(&rows);
    assert_eq!(view.field(700, 0), Some(42));
    assert_eq!(view.ty(42), Some(key("u32")));
    assert_eq!(view.read::<u32>(42), Ok(81));
    assert!(view.read::<u32>(1).is_err());
}

#[test]
fn external_ids_validate_depth_duplicates_modes_and_row_limits_before_append() {
    let mut out = Window::try_from(RowWindow {
        node: 10,
        start: 0,
        rows: 2,
        depth: 1,
    })
    .unwrap()
    .builder();
    assert!(out.external(10, Some(9), 0, 0, 1, None).is_err());
    assert!(out.external(10, Some(10), 0, 0, 0, None).is_err());
    out.external(10, Some(9), 0, 0, 0, None).unwrap();
    assert!(out.external(10, None, 0, 0, 0, None).is_err());
    assert!(out.external(11, Some(10), 0, 0, 0, None).is_err());
    assert!(matches!(
        out.external(11, Some(10), 0, 0, 2, None),
        Err(WireError::Invalid(_))
    ));
    assert!(out.node(None, 0, 0).is_err());
    out.external(11, Some(10), 0, 0, 1, Some(&7u8)).unwrap();
    assert!(matches!(
        out.external(12, Some(10), 1, 0, 1, None),
        Err(WireError::Length)
    ));
    assert_eq!(out.finish().len(), 2);
    let mut ordinal = Builder::default();
    ordinal.node(None, 0, 0).unwrap();
    assert!(ordinal.external(99, None, 0, 0, 0, None).is_err());
    assert_eq!(ordinal.finish().len(), 1);
}

#[test]
fn refused_external_values_roll_back_ids_and_share_the_value_byte_cap() {
    let mut out = Builder::default();
    let oversized = "x".repeat(1_048_576);
    assert!(matches!(
        out.external(99, None, 0, key("String"), 0, Some(&oversized)),
        Err(WireError::Length)
    ));
    out.external(99, None, 0, key("bool"), 0, Some(&true))
        .unwrap();
    let rows = out.finish();
    assert_eq!(rows.len(), 1);
    assert_eq!(Snapshot::new(&rows).read::<bool>(99), Ok(true));
}
