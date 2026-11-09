// Generated fixtures exercise every shape; TODO(#2273): make the generator emit clippy-clean code.
#![allow(clippy::all, unused_comparisons, unreachable_patterns, dead_code)]
use mod_api::{RowError, RowWindow, wire::Reader};
use mod_points::{
    locators::{Locators, Step},
    native_window::WindowBuilder,
    snapshot::Window,
};

fn request(node: u32, rows: u32, depth: u16) -> Result<Window, RowError> {
    Window::try_from(RowWindow {
        node,
        start: 0,
        rows,
        depth,
    })
}

#[test]
fn rows_use_stable_source_ids_and_owned_values() {
    let mut paths = Locators::default();
    let root = paths.intern(None, Step::Root(0)).unwrap();
    let (mut out, path) = WindowBuilder::new(&mut paths, request(root, 2, 1).unwrap()).unwrap();
    assert_eq!(path, [Step::Root(0)]);
    out.root(None, 0, 11, None).unwrap();
    let child = out
        .child(root, Step::Field(3), 3, 12, 1, Some(&42u32))
        .unwrap();
    let rows = out.finish();
    assert_eq!(rows[1].0, child);
    assert_eq!(rows[1].1, Some(root));
    assert_eq!(Reader::decode::<u32>(&rows[1].4), Ok(42));
    assert_eq!(paths.path(child).unwrap(), [Step::Root(0), Step::Field(3)]);
    let (mut out, _) = WindowBuilder::new(&mut paths, request(root, 2, 1).unwrap()).unwrap();
    out.root(None, 0, 11, None).unwrap();
    assert_eq!(
        out.child(root, Step::Field(3), 3, 12, 1, Some(&43u32)),
        Ok(child)
    );
}

#[test]
fn rejects_oversized_windows_values_and_unknown_nodes() {
    let mut paths = Locators::default();
    let root = paths.intern(None, Step::Root(0)).unwrap();
    assert!(matches!(request(root, 65_537, 1), Err(RowError::Budget)));
    assert!(matches!(request(root, 1, 1041), Err(RowError::Depth)));
    assert!(matches!(
        WindowBuilder::new(&mut paths, request(999, 1, 0).unwrap()),
        Err(RowError::Bounds)
    ));
    let (mut out, _) = WindowBuilder::new(&mut paths, request(root, 1, 0).unwrap()).unwrap();
    let big = "x".repeat(1_048_576);
    assert_eq!(out.root(None, 0, 0, Some(&big)), Err(RowError::Budget));
    let (mut out, _) = WindowBuilder::new(&mut paths, request(root, 0, 0).unwrap()).unwrap();
    assert_eq!(out.root(None, 0, 0, None), Err(RowError::Budget));
}
