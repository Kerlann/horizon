// Generated fixtures exercise every shape; TODO(#2273): make the generator emit clippy-clean code.
#![allow(clippy::all, unused_comparisons, unreachable_patterns, dead_code)]
#![cfg(feature = "wasm-guest")]

//! A real catalog borrowed-return point, `MapData::holder` (`&mut self -> &mut Holder`), on
//! stand-in layouts. The return phase projects the `&mut` through its pointee: edits write
//! the `Holder` in place at its node, atomically, and the reference is never retargeted.
mod common;
use common::Endpoint;
use mod_api::RowWindow;
use mod_points::{__private::SlotListener, PointId, endpoint_policy::EndpointPolicy};
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

use maps::MapData;
pub mod maps {
    pub struct MapData {
        pub holders: Vec<Holder>,
    }
    pub struct Holder {
        pub n: u32,
        pub label: String,
    }
}
use maps::Holder;
include!(concat!(env!("OUT_DIR"), "/native_raw_borrowed_mut.rs"));

static MODE: AtomicUsize = AtomicUsize::new(0);
static TAILS: AtomicUsize = AtomicUsize::new(0);
static REFUSED: AtomicBool = AtomicBool::new(false);
static TAIL: OnceLock<Endpoint> = OnceLock::new();

fn tail(snapshot: mod_points::snapshot::Snapshot<'_>, edits: &mut mod_api::wire::RecordPatch) {
    TAILS.fetch_add(1, Ordering::Relaxed);
    let holder = snapshot.field(0, 0).unwrap();
    assert!(snapshot.field(0, 1).is_none());
    let rows = snapshot
        .window(RowWindow {
            node: holder,
            start: 0,
            rows: 8,
            depth: 2,
        })
        .unwrap();
    let owned = mod_points::snapshot::Snapshot::new(&rows);
    let (n, label) = (
        owned.field(holder, 0).unwrap(),
        owned.field(holder, 1).unwrap(),
    );
    assert_eq!(owned.read::<u32>(n), Ok(5));
    assert_eq!(owned.read::<u64>(label), Ok(1));
    match MODE.load(Ordering::Relaxed) {
        1 => {
            edits.set(n, &9u32).unwrap();
            edits.set(label, &"b".to_owned()).unwrap();
        }
        // One edit outside the pointee's fields: the whole patch is refused.
        2 => {
            edits.set(n, &9u32).unwrap();
            edits.set(99, &1u32).unwrap();
        }
        _ => {}
    }
}

#[test]
fn mut_borrowed_return_writes_its_pointee_in_place_and_never_retargets() {
    let policy = Arc::new(EndpointPolicy::build(&catalog(), &[], &[HEAD_CALL]).unwrap());
    assert!(
        TAIL.set(Endpoint {
            listener: 0,
            policy,
            refused: &REFUSED,
            retained: Default::default(),
        })
        .is_ok()
    );
    mod_points::guest::register_raw(
        &mod_points::raw_listeners::game_maps_map_data_holder_return("borrowed-mut", tail),
    )
    .unwrap();
    let place = PointId::GameMapsMapDataHolderReturn;
    mod_points::register(
        &SlotListener::snapshot(
            "borrowed-mut-fixture",
            place,
            TAIL.get().unwrap(),
            0xC000_0000 + place as u32,
        )
        .unwrap(),
    )
    .unwrap();
    mod_points::__private::seal();
    let mut map = MapData {
        holders: vec![
            Holder {
                n: 5,
                label: "a".into(),
            },
            Holder {
                n: 5,
                label: "a".into(),
            },
        ],
    };
    // Observed only.
    let held = map.holder(0);
    assert_eq!((held.n, held.label.as_str()), (5, "a"));
    // Edits land in the returned pointee, and only in that element.
    MODE.store(1, Ordering::Relaxed);
    let held = map.holder(1);
    assert_eq!((held.n, held.label.as_str()), (9, "b"));
    assert_eq!((map.holders[0].n, map.holders[0].label.as_str()), (5, "a"));
    // A refused patch changes nothing.
    MODE.store(2, Ordering::Relaxed);
    REFUSED.store(true, Ordering::Relaxed);
    map.holders[0].n = 5;
    let held = map.holder(0);
    assert_eq!((held.n, held.label.as_str()), (5, "a"));
    assert_eq!(TAILS.load(Ordering::Relaxed), 3);
}
