// Generated fixtures exercise every shape; TODO(#2273): make the generator emit clippy-clean code.
#![allow(clippy::all, unused_comparisons, unreachable_patterns, dead_code)]
use mod_api::{
    RowError,
    wire::{Id, Reader, ResourceCodec, ResourceScope, WireError, Writer},
};
use mod_points::{callable::FunctionSource, snapshot::with_root};
use std::{
    any::{Any, TypeId},
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
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
fn decode(input: &mut Reader<'_>) -> Result<u32, WireError> {
    input.take()
}
fn encode(value: &u32, output: &mut Writer) {
    output.put(value);
}
#[test]
fn scoped_callable_checks_both_leases_input_and_reentrancy() {
    let count = Cell::new(0);
    let callback = RefCell::new(|n: u32| {
        count.set(count.get() + 1);
        n + 1
    });
    let scope = unsafe { ResourceScope::native_root(Rc::new(NoResources)) };
    let escaped = with_root(&callback, |root| {
        let source = unsafe { FunctionSource::new(&root, decode, encode) }.unwrap();
        assert_eq!(
            source.invoke(5u32.to_le_bytes().to_vec()).unwrap(),
            6u32.to_le_bytes()
        );
        assert!(source.invoke(vec![1]).is_err());
        assert!(source.invoke(vec![1, 0, 0, 0, 0]).is_err());
        assert_eq!(count.get(), 1);
        let other = Arc::clone(&source);
        assert!(matches!(
            std::thread::spawn(move || other.invoke(vec![]))
                .join()
                .unwrap(),
            Err(RowError::WrongThread)
        ));
        let borrow = callback.borrow_mut();
        assert!(matches!(
            source.invoke(5u32.to_le_bytes().to_vec()),
            Err(RowError::Reentrant)
        ));
        drop(borrow);
        std::mem::forget(root);
        source
    });
    assert!(matches!(escaped.invoke(vec![]), Err(RowError::Stale)));
    drop(scope);
}

#[test]
fn expired_call_is_refused_while_value_scope_is_live() {
    let count = Cell::new(0);
    let callback = RefCell::new(|n: u32| {
        count.set(count.get() + 1);
        n
    });
    let scope = unsafe { ResourceScope::native_root(Rc::new(NoResources)) };
    with_root(&callback, |root| {
        let source = unsafe { FunctionSource::new(&root, decode, encode) }.unwrap();
        drop(scope);
        assert!(matches!(
            source.invoke(1u32.to_le_bytes().to_vec()),
            Err(RowError::Stale)
        ));
        assert_eq!(count.get(), 0);
    });
}
