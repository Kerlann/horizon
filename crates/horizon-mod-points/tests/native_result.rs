// Generated fixtures exercise every shape; TODO(#2273): make the generator emit clippy-clean code.
#![allow(clippy::all, unused_comparisons, unreachable_patterns, dead_code)]
#![cfg(feature = "wasm-guest")]

//! A real catalog point returning `Result<u32, Box<dyn Error>>` (`Split::take`) on a stand-in
//! receiver. The return slot is a two-case tagged enum (Ok = 0, Err = 1): a listener reads the Ok
//! payload and the Err message (a `Box<dyn Error>` crosses as its `Display` text), replaces
//! Ok(v) with Ok(v2) at the payload, replaces Ok with Err(msg) as a whole value, and an invalid
//! tag is refused with the native value left as it was.
mod common;
use common::Endpoint;
use mod_api::{RowWindow, wire::Writer};
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
include!(concat!(env!("OUT_DIR"), "/native_raw_result.rs"));

static MODE: AtomicUsize = AtomicUsize::new(0);
static TAILS: AtomicUsize = AtomicUsize::new(0);
static REFUSED: AtomicBool = AtomicBool::new(false);
static SEEN: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
static TAIL: OnceLock<Endpoint> = OnceLock::new();

fn whole(tag: u8, payload: impl FnOnce(&mut Writer)) -> Vec<u8> {
    let mut out = Writer::new();
    out.put(&tag);
    payload(&mut out);
    out.bytes().to_vec()
}

fn tail(snapshot: mod_points::snapshot::Snapshot<'_>, edits: &mut mod_api::wire::RecordPatch) {
    TAILS.fetch_add(1, Ordering::Relaxed);
    let slot = snapshot.field(0, 0).unwrap();
    assert!(snapshot.field(0, 1).is_none());
    let rows = snapshot
        .window(RowWindow {
            node: slot,
            start: 0,
            rows: 32,
            depth: 4,
        })
        .unwrap();
    let owned = mod_points::snapshot::Snapshot::new(&rows);
    let tag = owned.read::<u32>(slot).unwrap();
    let case = owned.field(slot, tag).unwrap();
    let payload = owned.field(case, 0).unwrap();
    // Only the present case has a node.
    assert!(owned.field(slot, 1 - tag).is_none());
    let seen = if tag == 0 {
        format!("ok {}", owned.read::<u32>(payload).unwrap())
    } else {
        let units = owned.read::<u64>(payload).unwrap();
        let text: Vec<u16> = (0..units as u32)
            .map(|i| owned.read::<u16>(owned.field(payload, i).unwrap()).unwrap())
            .collect();
        format!("err {}", String::from_utf16(&text).unwrap())
    };
    SEEN.lock().unwrap().push(seen);
    match MODE.load(Ordering::Relaxed) {
        // Ok(v) -> Ok(v2), by the payload's locator.
        1 if tag == 0 => edits.set(payload, &9u32).unwrap(),
        // Ok -> Err(msg), a whole replacement.
        2 if tag == 0 => edits
            .set_encoded(slot, &whole(1, |out| out.put(&"changed".to_owned())))
            .unwrap(),
        // Err -> Ok, a whole replacement.
        3 if tag == 1 => edits
            .set_encoded(slot, &whole(0, |out| out.put(&77u32)))
            .unwrap(),
        // A tag that is neither case.
        4 => edits
            .set_encoded(slot, &whole(7, |out| out.put(&1u32)))
            .unwrap(),
        // Err's message is read only through its payload locator: a payload edit is refused too.
        5 if tag == 1 => edits.set_encoded(payload, &[0xFF]).unwrap(),
        _ => {}
    }
}

#[test]
fn result_slot_is_read_and_replaced_through_its_tagged_cases() {
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
        &mod_points::raw_listeners::game_split_players_split_profile_names_return(
            "result-tail",
            tail,
        ),
    )
    .unwrap();
    let place = PointId::GameSplitPlayersSplitProfileNamesReturn;
    mod_points::register(
        &SlotListener::snapshot(
            "result-fixture",
            place,
            TAIL.get().unwrap(),
            0xC000_0000 + place as u32,
        )
        .unwrap(),
    )
    .unwrap();
    mod_points::__private::seal();
    let take = |seats| Split { seats }.profile_names().map_err(|e| e.to_string());
    let seen = || std::mem::take(&mut *SEEN.lock().unwrap());
    // Observed: Ok payload and Err message.
    assert_eq!(take(5), Ok(5));
    assert_eq!(take(0), Err("no seats".to_owned()));
    assert_eq!(seen(), ["ok 5", "err no seats"]);
    // Ok(v) -> Ok(v2).
    MODE.store(1, Ordering::Relaxed);
    assert_eq!(take(5), Ok(9));
    // Ok -> Err(msg).
    MODE.store(2, Ordering::Relaxed);
    assert_eq!(take(5), Err("changed".to_owned()));
    // Err -> Ok.
    MODE.store(3, Ordering::Relaxed);
    assert_eq!(take(0), Ok(77));
    // An invalid tag, or an edit inside the Err message, is refused: the value is unchanged.
    REFUSED.store(true, Ordering::Relaxed);
    MODE.store(4, Ordering::Relaxed);
    assert_eq!(take(5), Ok(5));
    assert_eq!(take(0), Err("no seats".to_owned()));
    MODE.store(5, Ordering::Relaxed);
    assert_eq!(take(0), Err("no seats".to_owned()));
    assert_eq!(TAILS.load(Ordering::Relaxed), 8);
}
