// Generated fixtures exercise every shape; TODO(#2273): make the generator emit clippy-clean code.
#![allow(clippy::all, unused_comparisons, unreachable_patterns, dead_code)]
use mod_api::RowError;
use mod_points::locators::{Locators, MAX_DEPTH, MAX_LOCATORS, Step};

#[test]
fn paths_are_stable_and_refuse_invalid_edges() {
    let mut paths = Locators::default();
    let root = paths.intern(None, Step::Root(3)).unwrap();
    let field = paths.intern(Some(root), Step::Field(4)).unwrap();
    let variant = paths.intern(Some(field), Step::Variant(1)).unwrap();
    let index = paths.intern(Some(variant), Step::Index(7)).unwrap();
    assert_eq!(
        paths.intern(Some(field), Step::Variant(1)).unwrap(),
        variant
    );
    assert_eq!(
        paths.path(index).unwrap(),
        vec![
            Step::Root(3),
            Step::Field(4),
            Step::Variant(1),
            Step::Index(7)
        ]
    );
    assert_eq!(paths.path(u32::MAX), Err(RowError::Bounds));
    assert_eq!(paths.intern(None, Step::Field(0)), Err(RowError::Bounds));
    assert_eq!(
        paths.intern(Some(root), Step::Root(0)),
        Err(RowError::Bounds)
    );
    assert_eq!(
        paths.intern(Some(u32::MAX), Step::Field(0)),
        Err(RowError::Bounds)
    );
}

#[test]
fn depth_and_count_are_bounded_without_losing_existing_paths() {
    let mut paths = Locators::default();
    let root = paths.intern(None, Step::Root(0)).unwrap();
    let mut node = root;
    for _ in 0..MAX_DEPTH {
        node = paths.intern(Some(node), Step::Field(0)).unwrap();
    }
    assert_eq!(
        paths.intern(Some(node), Step::Field(0)),
        Err(RowError::Depth)
    );
    for i in 1..(MAX_LOCATORS - MAX_DEPTH) {
        paths.intern(Some(root), Step::Index(i as u64)).unwrap();
    }
    assert_eq!(
        paths.intern(Some(root), Step::Index(u64::MAX)),
        Err(RowError::Budget)
    );
    assert_eq!(paths.intern(None, Step::Root(0)), Ok(root));
}
