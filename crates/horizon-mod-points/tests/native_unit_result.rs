// Generated fixtures exercise every shape; TODO(#2273): make the generator emit clippy-clean code.
#![allow(clippy::all, unused_comparisons, unreachable_patterns, dead_code)]
#![cfg(feature = "wasm-guest")]

//! A real catalog `UnitResult` point (`Split::can_join`) on a stand-in receiver. The head
//! carries the bool cancel control; the return phase exposes the `Result` slot as its own
//! root, distinct from that control. The slot's `Result<(), String>` is a tagged enum: the
//! listener reads the Err message, and an edit that is not a valid case is refused.
mod common;
use common::Endpoint;
use mod_api::RowWindow;
use mod_points::{__private::SlotListener, PointId, endpoint_policy::EndpointPolicy};
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

use split::Split;
pub mod split {
    pub struct Split {
        pub seats: u32,
    }
}
include!(concat!(env!("OUT_DIR"), "/native_raw_unit_result.rs"));

static MODE: AtomicUsize = AtomicUsize::new(0);
static HEADS: AtomicUsize = AtomicUsize::new(0);
static TAILS: AtomicUsize = AtomicUsize::new(0);
static HEAD_REFUSED: AtomicBool = AtomicBool::new(false);
static TAIL_REFUSED: AtomicBool = AtomicBool::new(false);
static HEAD: OnceLock<Endpoint> = OnceLock::new();
static TAIL: OnceLock<Endpoint> = OnceLock::new();

fn head(snapshot: mod_points::snapshot::Snapshot<'_>, edits: &mut mod_api::wire::RecordPatch) {
    HEADS.fetch_add(1, Ordering::Relaxed);
    // Roots: receiver, name, cancel.
    let name = snapshot.field(0, 1).unwrap();
    let cancel = snapshot.field(0, 2).unwrap();
    assert!(snapshot.field(0, 3).is_none());
    assert_eq!(snapshot.read::<bool>(cancel), Ok(false));
    let _ = name;
    if MODE.load(Ordering::Relaxed) == 1 {
        edits.set(cancel, &true).unwrap();
    }
}

fn tail(snapshot: mod_points::snapshot::Snapshot<'_>, edits: &mut mod_api::wire::RecordPatch) {
    TAILS.fetch_add(1, Ordering::Relaxed);
    // The return phase has exactly one root, the Result slot, and no receiver or cancel.
    let slot = snapshot.field(0, 0).unwrap();
    assert!(snapshot.field(0, 1).is_none());
    let rows = snapshot
        .window(RowWindow {
            node: slot,
            start: 0,
            rows: 8,
            depth: 3,
        })
        .unwrap();
    let owned = mod_points::snapshot::Snapshot::new(&rows);
    // The native body returns Err("full"): case 1, its message the payload.
    assert_eq!(owned.read::<u32>(slot), Ok(1));
    let payload = owned.field(owned.field(slot, 1).unwrap(), 0).unwrap();
    assert_eq!(owned.read::<u64>(payload), Ok(4));
    if MODE.load(Ordering::Relaxed) == 2 {
        edits.set(slot, &7u32).unwrap();
    }
}

#[test]
fn unit_result_head_cancels_and_return_exposes_the_result_slot_as_its_own_root() {
    let policy = Arc::new(EndpointPolicy::build(&catalog(), &[], &[HEAD_CALL]).unwrap());
    let endpoint = |listener, refused| Endpoint {
        listener,
        policy: policy.clone(),
        refused,
        retained: Default::default(),
    };
    assert!(HEAD.set(endpoint(0, &HEAD_REFUSED)).is_ok());
    assert!(TAIL.set(endpoint(1, &TAIL_REFUSED)).is_ok());
    mod_points::guest::register_raw(
        &mod_points::raw_listeners::game_split_players_split_can_join("unit-head", head),
    )
    .unwrap();
    mod_points::guest::register_raw(
        &mod_points::raw_listeners::game_split_players_split_can_join_return("unit-tail", tail),
    )
    .unwrap();
    for (place, endpoint) in [
        (PointId::GameSplitPlayersSplitCanJoin, HEAD.get().unwrap()),
        (
            PointId::GameSplitPlayersSplitCanJoinReturn,
            TAIL.get().unwrap(),
        ),
    ] {
        mod_points::register(
            &SlotListener::snapshot("unit-fixture", place, endpoint, 0xC000_0000 + place as u32)
                .unwrap(),
        )
        .unwrap();
    }
    mod_points::__private::seal();
    let split = Split { seats: 2 };
    // Observed: both phases fire and the native result is returned.
    assert_eq!(split.can_join("no"), Err("full".to_owned()));
    assert_eq!(
        (HEADS.load(Ordering::Relaxed), TAILS.load(Ordering::Relaxed)),
        (1, 1)
    );
    // The head's cancel returns Ok(()) at once: the body does not run and the return phase
    // does not fire.
    MODE.store(1, Ordering::Relaxed);
    assert_eq!(split.can_join("no"), Ok(()));
    assert_eq!(
        (HEADS.load(Ordering::Relaxed), TAILS.load(Ordering::Relaxed)),
        (2, 1)
    );
    // An invalid whole value is refused: the value is returned as is.
    MODE.store(2, Ordering::Relaxed);
    TAIL_REFUSED.store(true, Ordering::Relaxed);
    assert_eq!(split.can_join("no"), Err("full".to_owned()));
    assert_eq!(
        (HEADS.load(Ordering::Relaxed), TAILS.load(Ordering::Relaxed)),
        (3, 2)
    );
    assert_eq!(split.seats, 2);
}
