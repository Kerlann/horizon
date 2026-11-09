// Generated fixtures exercise every shape; TODO(#2273): make the generator emit clippy-clean code.
#![allow(clippy::all, unused_comparisons, unreachable_patterns, dead_code)]
use mod_api::{
    RowError, RowPages, RowWindow,
    wire::{Id, ResourceCodec, ResourceScope, WireError},
};
use mod_points::snapshot::{PageSource, Snapshot, Window, key, with_root};
use std::{
    any::{Any, TypeId},
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
};

struct NoResources;
impl ResourceCodec for NoResources {
    fn store(&self, _: Box<dyn Any>) -> Result<Id, WireError> {
        Err(WireError::Invalid("not used"))
    }
    fn load(&self, _: Id, _: TypeId) -> Result<Box<dyn Any>, WireError> {
        Err(WireError::Invalid("not used"))
    }
}
struct Root {
    values: RefCell<Vec<u32>>,
    reads: Cell<usize>,
}
fn read(root: &Root, window: Window) -> Result<Vec<mod_api::Row>, RowError> {
    let mut out = window.builder();
    root.reads.set(root.reads.get() + 1);
    if window.node() != 0 {
        return Err(RowError::Bounds);
    }
    let values = root.values.borrow();
    if window.start() > values.len() {
        return Err(RowError::Bounds);
    }
    let parent = out.node(None, 0, const { key("Vec<u32>") })?;
    if window.depth() > 0 {
        for (index, value) in values
            .iter()
            .enumerate()
            .skip(window.start())
            .take(window.rows() - 1)
        {
            out.value(
                Some(parent),
                u32::try_from(index).map_err(|_| RowError::Bounds)?,
                const { key("u32") },
                value,
            )?;
        }
    }
    Ok(window.finish(out))
}
fn request() -> RowWindow {
    RowWindow {
        node: 0,
        start: 1,
        rows: 2,
        depth: 1,
    }
}

#[test]
fn retained_pages_cannot_read_after_the_call_and_rows_outlive_the_root() {
    let root = Box::new(Root {
        values: RefCell::new(vec![10, 20, 30]),
        reads: Cell::new(0),
    });
    let scope = unsafe { ResourceScope::native_root(Rc::new(NoResources)) };
    let ordinary = ResourceScope::new(Rc::new(NoResources));
    let (pages, owned) = with_root(&*root, |value| {
        let pages = unsafe { PageSource::new(&value, read) }.unwrap();
        drop(ordinary); // A shorter import-like scope must not own the borrow.
        let owned = pages.window(request()).unwrap();
        assert_eq!(Snapshot::new(&owned).read::<u32>(1), Ok(20));
        root.values.replace(vec![70, 80]); // Resolve elements afresh, not cached Vec addresses.
        assert_eq!(
            Snapshot::new(&pages.window(request()).unwrap()).read::<u32>(1),
            Ok(80)
        );
        drop(scope);
        assert!(matches!(pages.window(request()), Err(RowError::Stale)));
        (pages, owned)
    });
    drop(root);
    assert!(matches!(pages.window(request()), Err(RowError::Stale)));
    assert_eq!(Snapshot::new(&owned).read::<u32>(1), Ok(20));
}

#[test]
fn scope_expiration_is_checked_before_the_reader_and_cross_thread_access_is_refused() {
    let root = Root {
        values: RefCell::new(vec![1, 2]),
        reads: Cell::new(0),
    };
    let scope = unsafe { ResourceScope::native_root(Rc::new(NoResources)) };
    with_root(&root, |value| {
        let pages = unsafe { PageSource::new(&value, read) }.unwrap();
        let retained = Arc::clone(&pages);
        assert!(matches!(
            std::thread::spawn(move || retained.window(request()))
                .join()
                .unwrap(),
            Err(RowError::WrongThread)
        ));
        assert_eq!(root.reads.get(), 0);
        drop(scope);
        assert!(matches!(pages.window(request()), Err(RowError::Stale)));
        assert_eq!(root.reads.get(), 0);
    });
}

#[test]
fn value_lease_expires_before_the_call_and_cannot_be_revived() {
    let root = Root {
        values: RefCell::new(vec![1, 2]),
        reads: Cell::new(0),
    };
    let _scope = unsafe { ResourceScope::native_root(Rc::new(NoResources)) };
    let pages = with_root(&root, |value| {
        let pages = unsafe { PageSource::new(&value, read) }.unwrap();
        value.close(); // Before mutable apply, even while the callback and call remain live.
        assert!(matches!(pages.window(request()), Err(RowError::Stale)));
        pages
    });
    // Keep actual storage valid: missing lease checks fail without dangling dereferences.
    assert!(matches!(pages.window(request()), Err(RowError::Stale)));
    assert_eq!(root.reads.get(), 0);
    with_root(&root, |replacement| {
        let current = unsafe { PageSource::new(&replacement, read) }.unwrap();
        assert!(matches!(pages.window(request()), Err(RowError::Stale)));
        assert_eq!(
            Snapshot::new(&current.window(request()).unwrap()).read::<u32>(1),
            Ok(2)
        );
        assert_eq!(root.reads.get(), 1);
    });
}

#[test]
fn forgetting_the_public_handle_cannot_leak_the_private_value_lease() {
    let root = Box::new(Root {
        values: RefCell::new(vec![1, 2]),
        reads: Cell::new(0),
    });
    let _scope = unsafe { ResourceScope::native_root(Rc::new(NoResources)) };
    let pages = with_root(&*root, |value| {
        let pages = unsafe { PageSource::new(&value, read) }.unwrap();
        std::mem::forget(value);
        pages
    });
    // The first assertion is also a memory-safe probe for the missing-check mutant.
    assert!(matches!(pages.window(request()), Err(RowError::Stale)));
    assert_eq!(root.reads.get(), 0);
    drop(root);
    assert!(matches!(pages.window(request()), Err(RowError::Stale)));
}

#[test]
fn window_bounds_are_checked_before_reading_and_overruns_expose_no_partial_rows() {
    let root = Root {
        values: RefCell::new(vec![1, 2]),
        reads: Cell::new(0),
    };
    let huge = "x".repeat(1_048_576);
    let _scope = unsafe { ResourceScope::native_root(Rc::new(NoResources)) };
    with_root(&root, |value| {
        let pages = unsafe { PageSource::new(&value, read) }.unwrap();
        assert!(matches!(
            pages.window(RowWindow {
                rows: 65_537,
                ..request()
            }),
            Err(RowError::Budget)
        ));
        assert!(matches!(
            pages.window(RowWindow {
                depth: mod_api::wire::MAX_DEPTH + 1,
                ..request()
            }),
            Err(RowError::Depth)
        ));
        assert_eq!(root.reads.get(), 0);
        assert!(matches!(
            pages.window(RowWindow {
                node: 1,
                ..request()
            }),
            Err(RowError::Bounds)
        ));
        assert!(matches!(
            pages.window(RowWindow {
                start: 3,
                ..request()
            }),
            Err(RowError::Bounds)
        ));
    });
    fn spill(_: &(), window: Window) -> Result<Vec<mod_api::Row>, RowError> {
        let mut out = window.builder();
        let parent = out.node(None, 0, key("R"))?;
        let child = out.node(Some(parent), 0, key("R"))?;
        out.node(Some(child), 0, key("R"))?;
        Ok(window.finish(out))
    }
    with_root(&(), |unit| {
        let deep: Arc<dyn RowPages> = unsafe { PageSource::new(&unit, spill) }.unwrap();
        assert!(matches!(
            deep.window(RowWindow {
                rows: 4,
                depth: 1,
                ..request()
            }),
            Err(RowError::Depth)
        ));
        assert!(matches!(
            deep.window(RowWindow {
                rows: 2,
                depth: 4,
                ..request()
            }),
            Err(RowError::Budget)
        ));
    });
    with_root(&huge, |text| {
        let bytes = unsafe {
            PageSource::new(&text, |root, window| {
                let mut out = window.builder();
                out.value(None, 0, key("String"), root)?;
                Ok(window.finish(out))
            })
        }
        .unwrap();
        assert!(matches!(bytes.window(request()), Err(RowError::Budget)));
    });
}
