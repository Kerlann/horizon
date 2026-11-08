//! A call's resource registry. Data still uses Wire; only trait objects need the active side.
use super::{Id, WireError};
use std::{
    any::{Any, TypeId},
    cell::RefCell,
    rc::Rc,
};

/// Resolves resource identities for the side of a synchronous boundary call.
pub trait ResourceCodec {
    fn store(&self, value: Box<dyn Any>) -> Result<Id, WireError>;
    fn load(&self, id: Id, view: TypeId) -> Result<Box<dyn Any>, WireError>;
}
struct Scope {
    codec: Option<Rc<dyn ResourceCodec>>,
    previous: Option<usize>,
    next: Option<usize>,
}
#[derive(Default)]
struct Scopes {
    slots: Vec<Scope>,
    current: Option<usize>,
    free: Option<usize>,
}
thread_local! { static ACTIVE: RefCell<Scopes> = RefCell::new(Scopes::default()); }
/// Activates a call's codec, including guest-to-host-to-guest callbacks. Dropping scopes
/// out of order unlinks them: a retained inner guard cannot revive an expired outer codec.
/// Slots are reused, so steady-state calls allocate no scope bookkeeping.
pub struct ResourceScope {
    slot: usize,
    // The slot belongs to this thread even though its index alone would be Send + Sync.
    _thread: std::marker::PhantomData<Rc<()>>,
}
impl ResourceScope {
    pub fn new(codec: Rc<dyn ResourceCodec>) -> Self {
        let slot = ACTIVE.with(|active| {
            let mut scopes = active.borrow_mut();
            let slot = match scopes.free {
                Some(slot) => {
                    scopes.free = scopes.slots[slot].next;
                    slot
                }
                None => {
                    let slot = scopes.slots.len();
                    scopes.slots.push(Scope {
                        codec: None,
                        previous: None,
                        next: None,
                    });
                    slot
                }
            };
            let previous = scopes.current;
            scopes.slots[slot] = Scope {
                codec: Some(codec),
                previous,
                next: None,
            };
            if let Some(previous) = previous {
                scopes.slots[previous].next = Some(slot);
            }
            scopes.current = Some(slot);
            slot
        });
        Self {
            slot,
            _thread: std::marker::PhantomData,
        }
    }
}
impl Drop for ResourceScope {
    fn drop(&mut self) {
        // The codec's destructor can reenter Wire. Release the TLS borrow before dropping it.
        let codec = ACTIVE.try_with(|active| {
            let mut scopes = active.borrow_mut();
            let slot = self.slot;
            let previous = scopes.slots[slot].previous;
            let next = scopes.slots[slot].next;
            if let Some(previous) = previous {
                scopes.slots[previous].next = next;
            }
            if let Some(next) = next {
                scopes.slots[next].previous = previous;
            } else {
                scopes.current = previous;
            }
            let codec = scopes.slots[slot].codec.take();
            scopes.slots[slot].next = scopes.free;
            scopes.free = Some(slot);
            codec
        });
        drop(codec);
    }
}
fn current() -> Result<Rc<dyn ResourceCodec>, WireError> {
    ACTIVE
        .with(|active| {
            let scopes = active.borrow();
            scopes
                .current
                .and_then(|slot| scopes.slots[slot].codec.clone())
        })
        .ok_or(WireError::Invalid("no resource context"))
}
#[cfg(test)]
mod tests {
    use super::*;
    struct Codec(u64);
    impl ResourceCodec for Codec {
        fn store(&self, _: Box<dyn Any>) -> Result<Id, WireError> {
            Ok(Id(self.0))
        }
        fn load(&self, _: Id, _: TypeId) -> Result<Box<dyn Any>, WireError> {
            Ok(Box::new(self.0))
        }
    }
    #[test]
    fn resource_scope_out_of_order_drop_never_revives_expired_codec() {
        let outer = ResourceScope::new(Rc::new(Codec(1)));
        let middle = ResourceScope::new(Rc::new(Codec(2)));
        let inner = ResourceScope::new(Rc::new(Codec(3)));
        assert_eq!(store(Box::new(())), Ok(Id(3)));
        drop(middle);
        assert_eq!(store(Box::new(())), Ok(Id(3)));
        let newest = ResourceScope::new(Rc::new(Codec(4)));
        drop(outer);
        assert_eq!(store(Box::new(())), Ok(Id(4)));
        drop(newest);
        assert_eq!(store(Box::new(())), Ok(Id(3)));
        drop(inner);
        assert_eq!(
            store(Box::new(())),
            Err(WireError::Invalid("no resource context"))
        );
        // A new call after retained guards have gone cannot inherit any expired scope.
        let next = ResourceScope::new(Rc::new(Codec(5)));
        assert_eq!(load::<u64>(Id(0)), Ok(5));
        drop(next);
        assert_eq!(
            store(Box::new(())),
            Err(WireError::Invalid("no resource context"))
        );
    }
    #[test]
    fn resource_scope_callbacks_and_codec_drop_do_not_hold_tls_borrows() {
        struct Reentrant;
        impl ResourceCodec for Reentrant {
            fn store(&self, _: Box<dyn Any>) -> Result<Id, WireError> {
                let _inner = ResourceScope::new(Rc::new(Codec(9)));
                store(Box::new(()))
            }
            fn load(&self, _: Id, _: TypeId) -> Result<Box<dyn Any>, WireError> {
                unreachable!()
            }
        }
        let reentrant = ResourceScope::new(Rc::new(Reentrant));
        assert_eq!(store(Box::new(())), Ok(Id(9)));
        drop(reentrant);
        struct OnDrop(Rc<std::cell::Cell<u64>>);
        impl ResourceCodec for OnDrop {
            fn store(&self, _: Box<dyn Any>) -> Result<Id, WireError> {
                unreachable!()
            }
            fn load(&self, _: Id, _: TypeId) -> Result<Box<dyn Any>, WireError> {
                unreachable!()
            }
        }
        impl Drop for OnDrop {
            fn drop(&mut self) {
                self.0.set(store(Box::new(())).unwrap().0);
            }
        }
        let seen = Rc::new(std::cell::Cell::new(0));
        let outer = ResourceScope::new(Rc::new(OnDrop(seen.clone())));
        let inner = ResourceScope::new(Rc::new(Codec(10)));
        drop(outer);
        assert_eq!(seen.get(), 10);
        drop(inner);
    }
    #[test]
    fn resource_scope_dropping_outer_preserves_only_live_inner() {
        let outer = ResourceScope::new(Rc::new(Codec(6)));
        let retained = ResourceScope::new(Rc::new(Codec(7)));
        drop(outer);
        assert_eq!(store(Box::new(())), Ok(Id(7)));
        drop(retained);
        assert_eq!(
            store(Box::new(())),
            Err(WireError::Invalid("no resource context"))
        );
    }
}
pub(super) fn store(value: Box<dyn Any>) -> Result<Id, WireError> {
    current()?.store(value)
}
pub(super) fn load<T: 'static>(id: Id) -> Result<T, WireError> {
    current()?
        .load(id, TypeId::of::<T>())?
        .downcast::<T>()
        .map(|value| *value)
        .map_err(|_| WireError::Invalid("wrong resource view"))
}
