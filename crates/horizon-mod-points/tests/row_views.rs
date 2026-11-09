// Generated fixtures exercise every shape; TODO(#2273): make the generator emit clippy-clean code.
#![allow(clippy::all, unused_comparisons, unreachable_patterns, dead_code)]
use mod_api::{
    RowError, RowView,
    wire::{Id, ResourceCodec, ResourceScope, WireError},
};
use mod_points::snapshot::{PageSource, Window, with_root};
use std::{
    any::{Any, TypeId},
    rc::Rc,
};

struct NoResources;
impl ResourceCodec for NoResources {
    fn store(&self, _: Box<dyn Any>) -> Result<Id, WireError> {
        Err(WireError::Invalid("unused"))
    }
    fn load(&self, _: Id, _: TypeId) -> Result<Box<dyn Any>, WireError> {
        Err(WireError::Invalid("unused"))
    }
}
// `PageSource::new` takes a reader of `&T` for the `Vec` it pages.
#[allow(clippy::ptr_arg)]
fn read(values: &Vec<u32>, window: Window) -> Result<Vec<mod_api::Row>, RowError> {
    let mut out = window.builder();
    if window.node() != 0 || window.start() > values.len() {
        return Err(RowError::Bounds);
    }
    out.external(0, None, 0, 1, 0, Some(&(values.len() as u64)))?;
    if window.depth() > 0 {
        for (i, value) in values
            .iter()
            .enumerate()
            .skip(window.start())
            .take(window.rows().saturating_sub(1))
        {
            out.external(i as u32 + 1, Some(0), i as u32, 2, 1, Some(value))?;
        }
    }
    Ok(window.finish(out))
}
#[test]
fn row_view_reads_real_pages_and_refuses_after_expiry() {
    let scope = unsafe { ResourceScope::native_root(Rc::new(NoResources)) };
    let (root, child) = with_root(&vec![10, 20], |value| {
        let pages = unsafe { PageSource::new(&value, read) }.unwrap();
        let root = RowView::new(
            vec![(0, None, 0, 1, 2u64.to_le_bytes().to_vec())],
            Some(pages),
            0,
        )
        .unwrap();
        let child = root.child(1).unwrap();
        assert_eq!(child.read::<u32>(), Ok(20));
        (root, child)
    });
    assert!(matches!(root.child(0), Err(RowError::Stale)));
    assert_eq!(child.read::<u32>(), Ok(20));
    drop(scope);
}
