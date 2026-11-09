// Generated fixtures exercise every shape; TODO(#2273): make the generator emit clippy-clean code.
#![allow(clippy::all, unused_comparisons, unreachable_patterns, dead_code)]
#![cfg(feature = "wasm-guest")]

//! Real-catalog nominal point fixture, using the generated dispatch and shared Wire resources.
use mod_api::wire::{Id, Reader, Record, ResourceCodec, ResourceScope, WireError, Writer};
use mod_api::{RowError, RowPages, RowWindow};
use mod_points::{
    __private::{RawForward, SlotListener},
    PointId,
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

include!(concat!(env!("OUT_DIR"), "/native_raw_link_buttons.rs"));

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
    retained: Mutex<Option<Arc<dyn RowPages>>>,
}
static HEAD: Endpoint = Endpoint {
    listener: 0,
    retained: Mutex::new(None),
};
static TAIL: Endpoint = Endpoint {
    listener: 1,
    retained: Mutex::new(None),
};
static MODE: AtomicUsize = AtomicUsize::new(0);
static RETURNS: AtomicUsize = AtomicUsize::new(0);
impl RawForward for Endpoint {
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
        if (self.listener == 1 && MODE.load(Ordering::Relaxed) == 1)
            || (self.listener == 0 && MODE.load(Ordering::Relaxed) == 3)
        {
            assert!(record.apply(reply.rest()).is_err());
        } else {
            record.apply(reply.rest()).unwrap();
        }
        let source = resources.0.borrow().first().cloned().unwrap();
        // The call marker is still live: apply must already have closed the encoded value.
        assert_eq!(
            source.window(RowWindow {
                node: 0,
                start: 0,
                rows: 1,
                depth: 0
            }),
            Err(RowError::Stale)
        );
        *self.retained.lock().unwrap() = Some(source);
    }
}
fn head(snapshot: mod_points::snapshot::Snapshot<'_>, edits: &mut mod_api::wire::RecordPatch) {
    assert_eq!(
        snapshot.read::<f32>(snapshot.field(0, 0).unwrap()),
        Ok(400.0)
    );
    assert_eq!(snapshot.read::<usize>(snapshot.field(0, 2).unwrap()), Ok(2));
    if MODE.load(Ordering::Relaxed) == 2 {
        edits
            .set(
                snapshot.field(0, 3).unwrap(),
                &Some(vec![(71.0f32, 73.0f32)]),
            )
            .unwrap();
    } else {
        edits.set(snapshot.field(0, 1).unwrap(), &27.0f32).unwrap();
        if MODE.load(Ordering::Relaxed) == 3 {
            edits.set(snapshot.field(0, 3).unwrap(), &2u8).unwrap();
        }
    }
}
fn tail(snapshot: mod_points::snapshot::Snapshot<'_>, edits: &mut mod_api::wire::RecordPatch) {
    RETURNS.fetch_add(1, Ordering::Relaxed);
    let vector = snapshot.field(0, 0).unwrap();
    assert_eq!(snapshot.read::<u64>(vector), Ok(2));
    let rows = snapshot
        .window(RowWindow {
            node: vector,
            start: 0,
            rows: 7,
            depth: 2,
        })
        .unwrap();
    let owned = mod_points::snapshot::Snapshot::new(&rows);
    let first = owned.field(vector, 0).unwrap();
    assert_eq!(owned.read::<f32>(owned.field(first, 0).unwrap()), Ok(45.0));
    assert_eq!(
        owned.read::<f32>(owned.field(first, 1).unwrap()),
        Ok(if MODE.load(Ordering::Relaxed) == 3 {
            5.0
        } else {
            27.0
        })
    );
    if MODE.load(Ordering::Relaxed) == 3 {
        return;
    }
    edits.set(vector, &vec![(17.0f32, 19.0f32)]).unwrap();
    if MODE.load(Ordering::Relaxed) == 1 {
        edits.set(42, &99u32).unwrap();
    }
}
#[test]
fn generated_nominal_head_return_and_pages_use_one_atomic_record() {
    mod_points::guest::register_raw(&mod_points::raw_listeners::game_server_links_link_buttons(
        "nominal-head",
        head,
    ))
    .unwrap();
    mod_points::guest::register_raw(
        &mod_points::raw_listeners::game_server_links_link_buttons_return("nominal-tail", tail),
    )
    .unwrap();
    for (place, endpoint) in [
        (PointId::GameServerLinksLinkButtons, &HEAD),
        (PointId::GameServerLinksLinkButtonsReturn, &TAIL),
    ] {
        mod_points::register(
            &SlotListener::snapshot(
                "nominal-fixture",
                place,
                endpoint,
                0xC000_0000 + place as u32,
            )
            .unwrap(),
        )
        .unwrap();
    }
    mod_points::__private::seal();
    assert_eq!(link_buttons(400.0, 5.0, 2), vec![(17.0, 19.0)]);
    MODE.store(1, Ordering::Relaxed);
    assert_eq!(
        link_buttons(400.0, 5.0, 2),
        vec![(45.0, 27.0), (45.0, 49.0)]
    );
    MODE.store(2, Ordering::Relaxed);
    assert_eq!(link_buttons(400.0, 5.0, 2), vec![(71.0, 73.0)]);
    MODE.store(3, Ordering::Relaxed);
    assert_eq!(link_buttons(400.0, 5.0, 2), vec![(45.0, 5.0), (45.0, 27.0)]);
    assert_eq!(RETURNS.load(Ordering::Relaxed), 3);
    for endpoint in [&HEAD, &TAIL] {
        let source = endpoint.retained.lock().unwrap().take().unwrap();
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
}
