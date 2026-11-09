// Generated fixtures exercise every shape; TODO(#2273): make the generator emit clippy-clean code.
#![allow(clippy::all, unused_comparisons, unreachable_patterns, dead_code)]
#![cfg(feature = "wasm-guest")]

//! A callback taking the call's own receiver (`fire: fn(&mut Self, u32)`, as on the real
//! `Game::guest_player_tick`) crosses as a callable that takes only the remaining arguments: the
//! host lends its own receiver, so the callee's writes land on the real object and nothing is
//! copied back. A callback returning JSON crosses with the value as its text.
mod common;
use common::Endpoint;
use mod_api::{
    RowFunction,
    wire::{Reader, Writer},
};
use mod_points::{__private::SlotListener, PointId, endpoint_policy::EndpointPolicy};
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

pub mod startup_menu {
    #[derive(Clone, Copy)]
    pub enum Action {
        Quit,
        Options,
    }
    pub struct Loading {
        pub actions: u32,
    }
}
use startup_menu::{Action, Loading};
include!(concat!(env!("OUT_DIR"), "/native_raw_bound.rs"));

static HEADS: AtomicUsize = AtomicUsize::new(0);
static REFUSED: AtomicBool = AtomicBool::new(false);
static HEAD: OnceLock<Endpoint> = OnceLock::new();

fn bytes<T: mod_api::wire::Put>(value: &T) -> Vec<u8> {
    let mut out = Writer::new();
    out.put(value);
    out.into_bytes()
}

fn head(snapshot: mod_points::snapshot::Snapshot<'_>, _: &mut mod_api::wire::RecordPatch) {
    HEADS.fetch_add(1, Ordering::Relaxed);
    // Roots: receiver, fire, data, action, cancel.
    let function = |root| {
        snapshot
            .read::<Arc<dyn RowFunction>>(snapshot.field(0, root).unwrap())
            .unwrap()
    };
    let (fire, data) = (function(1), function(2));
    // The listener supplies only `n`; the callee adds it to the host's own receiver.
    assert_eq!(fire.invoke(bytes(&(5u32,))).unwrap(), bytes(&()));
    assert!(fire.invoke(bytes(&(5u32, 6u32))).is_err());
    // JSON crosses as text, both ways.
    let found = data.invoke(bytes(&("k".to_string(),))).unwrap();
    let text = Reader::decode::<Option<String>>(&found).unwrap().unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&text).unwrap(),
        serde_json::json!({"a": [1, 2.5, null], "b": "x"})
    );
    assert_eq!(
        Reader::decode::<Option<String>>(&data.invoke(bytes(&("none".to_string(),))).unwrap()),
        Ok(None)
    );
}

#[test]
fn receiver_bound_callbacks_and_json_cross_as_callables() {
    let policy = Arc::new(EndpointPolicy::build(&catalog(), &[], &[HEAD_CALL]).unwrap());
    assert!(
        HEAD.set(Endpoint {
            listener: 0,
            policy,
            refused: &REFUSED,
            retained: Default::default(),
        })
        .is_ok()
    );
    mod_points::guest::register_raw(
        &mod_points::raw_listeners::game_startup_menu_loading_menu_action("bound", head),
    )
    .unwrap();
    mod_points::register(
        &SlotListener::snapshot(
            "bound-fixture",
            PointId::GameStartupMenuLoadingMenuAction,
            HEAD.get().unwrap(),
            0xC000_0000 + PointId::GameStartupMenuLoadingMenuAction as u32,
        )
        .unwrap(),
    )
    .unwrap();
    mod_points::__private::seal();
    let mut game = Loading { actions: 0 };
    let fire = |game: &mut Loading, n: u32| game.actions += n;
    let data = |key: &str| (key == "k").then(|| serde_json::json!({"a": [1, 2.5, null], "b": "x"}));
    game.menu_action(fire, &data, Action::Quit);
    // 5 from the listener's call, 7 from the native body.
    assert_eq!(game.actions, 12);
    assert_eq!(HEADS.load(Ordering::Relaxed), 1);
}
