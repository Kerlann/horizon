// Generated fixtures exercise every shape; TODO(#2273): make the generator emit clippy-clean code.
#![allow(clippy::all, unused_comparisons, unreachable_patterns, dead_code)]
#![cfg(feature = "wasm-guest")]

//! A real catalog `&self` point on a stand-in receiver, generated through the whole-catalog
//! graph and read under two manifest policies. The guest sees only granted receiver fields.
use mod_api::wire::{Id, Reader, Record, ResourceCodec, ResourceScope, WireError, Writer};
use mod_api::{RowError, RowPages, RowWindow};
use mod_points::{
    __private::{RawForward, SlotListener},
    PointId,
    endpoint_policy::{EndpointPolicy, InternalDeclaration},
    exposure::Plan,
};
use std::{
    any::{Any, TypeId},
    cell::RefCell,
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use model::Loading;
pub mod model {
    pub struct Loading {
        pub hidden: u32,
        pub size: [u32; 2],
    }
}
include!(concat!(env!("OUT_DIR"), "/native_raw_receiver.rs"));

#[derive(Default)]
struct Resources(RefCell<Vec<Arc<dyn RowPages>>>);
impl ResourceCodec for Resources {
    fn store(&self, value: Box<dyn Any>) -> Result<Id, WireError> {
        let value = value
            .downcast::<Arc<dyn RowPages>>()
            .map_err(|_| WireError::Invalid("wrong resource"))?;
        let mut values = self.0.borrow_mut();
        values.push(*value);
        Ok(Id(values.len() as u64))
    }
    fn load(&self, id: Id, view: TypeId) -> Result<Box<dyn Any>, WireError> {
        if view != TypeId::of::<Arc<dyn RowPages>>() {
            return Err(WireError::Invalid("wrong view"));
        }
        self.0
            .borrow()
            .get(id.0 as usize - 1)
            .cloned()
            .map(|value| Box::new(value) as Box<dyn Any>)
            .ok_or(WireError::Invalid("unknown resource"))
    }
}

struct Endpoint {
    listener: u32,
    policy: EndpointPolicy,
    retained: Mutex<Option<Arc<dyn RowPages>>>,
}
impl RawForward for Endpoint {
    fn plan(&self, head_call: usize) -> Option<Arc<Plan>> {
        self.policy.plan(head_call)
    }
    fn forward(&self, id: u32, record: &mut dyn Record) {
        let resources = Rc::new(Resources::default());
        // SAFETY: this is the synchronous native guest loopback's actual Hook call frame.
        let _call = unsafe { ResourceScope::native_root(resources.clone()) };
        let mut input = Writer::new();
        input.put(&self.listener);
        record.encode(&mut input);
        assert_eq!(input.error(), None);
        let mut input = Reader::new(input.bytes());
        let mut reply = Writer::new();
        mod_points::guest_dispatch(id, &mut input, &mut reply).unwrap();
        let mut reply = Reader::new(reply.bytes());
        reply.take::<()>().unwrap();
        record.apply(reply.rest()).unwrap();
        let source = resources.0.borrow().first().cloned().unwrap();
        *self.retained.lock().unwrap() = Some(source);
    }
}

static SEEN: AtomicUsize = AtomicUsize::new(0);
static ENDPOINT: std::sync::OnceLock<Endpoint> = std::sync::OnceLock::new();

fn declaration(field: &str) -> InternalDeclaration {
    InternalDeclaration {
        path: format!("horizon_client::model::Loading::{field}"),
        kind: Some("field".into()),
        hash: field.into(),
    }
}

fn head(snapshot: mod_points::snapshot::Snapshot<'_>, edits: &mut mod_api::wire::RecordPatch) {
    // Root 0 is the receiver; its members arrive in a lazy window.
    let receiver = snapshot.field(0, 0).unwrap();
    let rows = snapshot
        .window(RowWindow {
            node: 0,
            start: 0,
            rows: 16,
            depth: 4,
        })
        .unwrap();
    // A window starting on the hidden ordinal is refused, never shifted onto the next field;
    // one starting at the granted ordinal reads it.
    let node = |start| RowWindow {
        node: receiver,
        start,
        rows: 8,
        depth: 3,
    };
    assert_eq!(snapshot.window(node(0)), Err(RowError::Bounds));
    assert!(snapshot.window(node(1)).is_ok());
    let owned = mod_points::snapshot::Snapshot::new(&rows);
    // Only the granted `size` is present, under its declaration ordinal 1.
    assert!(owned.field(receiver, 0).is_none(), "hidden field leaked");
    let size = owned.field(receiver, 1).unwrap();
    assert_eq!(owned.read::<u32>(owned.field(size, 0).unwrap()), Ok(3));
    assert_eq!(owned.read::<u32>(owned.field(size, 1).unwrap()), Ok(4));
    SEEN.fetch_add(1, Ordering::Relaxed);
    let _ = edits;
}

#[test]
fn generated_receiver_reads_only_granted_fields_through_the_endpoint_plan() {
    let policy = EndpointPolicy::build(&catalog(), &[declaration("size")], &[HEAD_CALL]).unwrap();
    ENDPOINT
        .set(Endpoint {
            listener: 0,
            policy,
            retained: Mutex::new(None),
        })
        .ok()
        .unwrap();
    mod_points::guest::register_raw(
        &mod_points::raw_listeners::game_startup_menu_loading_menu_size("receiver-head", head),
    )
    .unwrap();
    mod_points::register(
        &SlotListener::snapshot(
            "receiver-fixture",
            PointId::GameStartupMenuLoadingMenuSize,
            ENDPOINT.get().unwrap(),
            0xC000_0000 + PointId::GameStartupMenuLoadingMenuSize as u32,
        )
        .unwrap(),
    )
    .unwrap();
    mod_points::__private::seal();
    let loading = model::Loading {
        hidden: 7,
        size: [3, 4],
    };
    // The listener ran exactly once and the call returned the native value.
    assert_eq!(loading.menu_size(), [3, 4]);
    assert_eq!(loading.hidden, 7);
    assert_eq!(SEEN.load(Ordering::Relaxed), 1);
    let source = ENDPOINT
        .get()
        .unwrap()
        .retained
        .lock()
        .unwrap()
        .take()
        .unwrap();
    assert_eq!(
        source.window(RowWindow {
            node: 0,
            start: 0,
            rows: 1,
            depth: 0
        }),
        Err(RowError::Stale)
    );
}
