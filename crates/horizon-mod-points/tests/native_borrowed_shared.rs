// Generated fixtures exercise every shape; TODO(#2273): make the generator emit clippy-clean code.
#![allow(clippy::all, unused_comparisons, unreachable_patterns, dead_code)]
#![cfg(feature = "wasm-guest")]

//! A real catalog borrowed-return point, `JigsawEditor::value` (`&self -> &str`), on a
//! stand-in receiver. The return phase projects the reference through its pointee and has
//! no receiver; a shared pointee is read-only and the reference is never retargeted.
mod common;
use common::Endpoint;
use mod_api::RowWindow;
use mod_points::{__private::SlotListener, PointId, endpoint_policy::EndpointPolicy};
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

use jigsaw::JigsawEditor;
pub mod jigsaw {
    pub struct JigsawEditor {
        pub count: u32,
    }
}
include!(concat!(env!("OUT_DIR"), "/native_raw_borrowed_shared.rs"));

static MODE: AtomicUsize = AtomicUsize::new(0);
static TAILS: AtomicUsize = AtomicUsize::new(0);
static REFUSED: AtomicBool = AtomicBool::new(false);
static TAIL: OnceLock<Endpoint> = OnceLock::new();

fn tail(snapshot: mod_points::snapshot::Snapshot<'_>, edits: &mut mod_api::wire::RecordPatch) {
    TAILS.fetch_add(1, Ordering::Relaxed);
    // One root, the returned `&str`; no receiver, no cancel.
    let value = snapshot.field(0, 0).unwrap();
    assert!(snapshot.field(0, 1).is_none());
    let expected = ["zero", "one"][MODE.load(Ordering::Relaxed) % 2];
    // Text rows carry their UTF-16 length; the code units are its children.
    assert_eq!(snapshot.read::<u64>(value), Ok(expected.len() as u64));
    let rows = snapshot
        .window(RowWindow {
            node: value,
            start: 0,
            rows: 8,
            depth: 2,
        })
        .unwrap();
    let owned = mod_points::snapshot::Snapshot::new(&rows);
    let units: Vec<u16> = (0..expected.len() as u32)
        .map(|i| owned.read::<u16>(owned.field(value, i).unwrap()).unwrap())
        .collect();
    assert_eq!(units, expected.encode_utf16().collect::<Vec<_>>());
    if MODE.load(Ordering::Relaxed) >= 2 {
        edits.set(value, &"changed".to_owned()).unwrap();
    }
}

#[test]
fn shared_borrowed_return_is_read_through_its_pointee_and_refuses_edits() {
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
        &mod_points::raw_listeners::game_jigsaw_edit_jigsaw_editor_value_return("borrowed", tail),
    )
    .unwrap();
    let place = PointId::GameJigsawEditJigsawEditorValueReturn;
    mod_points::register(
        &SlotListener::snapshot(
            "borrowed-fixture",
            place,
            TAIL.get().unwrap(),
            0xC000_0000 + place as u32,
        )
        .unwrap(),
    )
    .unwrap();
    mod_points::__private::seal();
    let editor = JigsawEditor { count: 0 };
    assert_eq!(editor.value(0), "zero");
    MODE.store(1, Ordering::Relaxed);
    assert_eq!(editor.value(1), "one");
    // A shared pointee takes no patch: the whole edit is refused and the native `&str` stays.
    MODE.store(2, Ordering::Relaxed);
    REFUSED.store(true, Ordering::Relaxed);
    assert_eq!(editor.value(0), "zero");
    assert_eq!(TAILS.load(Ordering::Relaxed), 3);
}
