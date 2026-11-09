// Generated fixtures exercise every shape; TODO(#2273): make the generator emit clippy-clean code.
#![allow(clippy::all, unused_comparisons, unreachable_patterns, dead_code)]
#![cfg(feature = "wasm-guest")]

//! A real catalog free-function point with callback arguments (`&dyn Fn`, `&mut dyn FnMut`).
//! A callback whose signature projects crosses as a host-held callable the listener invokes with
//! Wire-encoded arguments, within the call's lease; one whose signature has no Wire codec stays
//! an opaque leaf. The native body still calls the real closures.
mod common;
use common::Endpoint;
use mod_api::{
    RowError, RowFunction,
    wire::{Reader, Writer},
};
use mod_points::{__private::SlotListener, PointId, endpoint_policy::EndpointPolicy};
use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

pub mod test_environments {
    #[derive(Clone, Copy)]
    pub struct BlockPos {
        pub x: i32,
        pub y: i32,
        pub z: i32,
    }
}
use test_environments::BlockPos;

include!(concat!(env!("OUT_DIR"), "/native_raw_callback.rs"));

static HEADS: AtomicUsize = AtomicUsize::new(0);
static REFUSED: AtomicBool = AtomicBool::new(false);
static HEAD: OnceLock<Endpoint> = OnceLock::new();
static RETAINED: Mutex<Option<Arc<dyn RowFunction>>> = Mutex::new(None);
static REENTER: AtomicBool = AtomicBool::new(false);
static REENTERED: Mutex<Option<Result<Vec<u8>, RowError>>> = Mutex::new(None);

fn bytes<T: mod_api::wire::Put>(value: &T) -> Vec<u8> {
    let mut out = Writer::new();
    out.put(value);
    out.into_bytes()
}

fn head(snapshot: mod_points::snapshot::Snapshot<'_>, _: &mut mod_api::wire::RecordPatch) {
    HEADS.fetch_add(1, Ordering::Relaxed);
    // Roots: pos, load, wanted, rain, emit, publish, result.
    let function = |root| {
        snapshot
            .read::<Arc<dyn RowFunction>>(snapshot.field(0, root).unwrap())
            .unwrap()
    };
    let (load, wanted, rain, emit) = (function(1), function(2), function(3), function(4));
    assert_eq!(
        Reader::decode::<Option<u32>>(
            &load
                .invoke(bytes(&("minecraft:overworld".to_string(),)))
                .unwrap()
        ),
        Ok(Some(100))
    );
    assert_eq!(
        Reader::decode::<Option<u32>>(&load.invoke(bytes(&("other".to_string(),))).unwrap()),
        Ok(None)
    );
    assert_eq!(
        Reader::decode::<bool>(&wanted.invoke(bytes(&("w".to_string(),))).unwrap()),
        Ok(true)
    );
    // A struct argument crosses as its fields; the closure sees the rebuilt `BlockPos`.
    assert_eq!(
        Reader::decode::<bool>(&rain.invoke(bytes(&((1i32, 64i32, -3i32),))).unwrap()),
        Ok(true)
    );
    assert_eq!(
        Reader::decode::<bool>(&rain.invoke(bytes(&((0i32, 64i32, 0i32),))).unwrap()),
        Ok(false)
    );
    // A `FnMut` keeps its state across invocations.
    assert_eq!(
        Reader::decode::<u32>(&emit.invoke(bytes(&(5u32,))).unwrap()),
        Ok(5)
    );
    assert_eq!(
        Reader::decode::<u32>(&emit.invoke(bytes(&(5u32,))).unwrap()),
        Ok(10)
    );
    // Short and trailing input are refused before the closure runs.
    assert!(emit.invoke(vec![1]).is_err());
    assert!(emit.invoke(bytes(&(5u32, 6u32))).is_err());
    // Another thread is refused.
    let other = Arc::clone(&emit);
    assert_eq!(
        std::thread::spawn(move || other.invoke(bytes(&(1u32,))))
            .join()
            .unwrap(),
        Err(RowError::WrongThread)
    );
    // Re-entrancy: the native closure calls the same source back while it runs.
    *RETAINED.lock().unwrap() = Some(Arc::clone(&load));
    REENTER.store(true, Ordering::Relaxed);
    load.invoke(bytes(&("minecraft:overworld".to_string(),)))
        .unwrap();
    assert_eq!(
        REENTERED.lock().unwrap().take(),
        Some(Err(RowError::Reentrant))
    );
    // `&Path` crosses as its text: the listener publishes through the real closure.
    assert_eq!(
        function(5)
            .invoke(bytes(&(3u32, "some/dir".to_string())))
            .unwrap(),
        bytes(&())
    );
}

#[test]
fn callback_arguments_cross_as_callables_within_the_call_lease() {
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
    mod_points::guest::register_raw(&mod_points::raw_listeners::game_test_environments_parse(
        "callback", head,
    ))
    .unwrap();
    mod_points::register(
        &SlotListener::snapshot(
            "callback-fixture",
            PointId::GameTestEnvironmentsParse,
            HEAD.get().unwrap(),
            0xC000_0000 + PointId::GameTestEnvironmentsParse as u32,
        )
        .unwrap(),
    )
    .unwrap();
    mod_points::__private::seal();
    let calls = AtomicUsize::new(0);
    let load = |name: &str| {
        calls.fetch_add(1, Ordering::Relaxed);
        if REENTER.swap(false, Ordering::Relaxed) {
            let retained = RETAINED.lock().unwrap().clone().unwrap();
            *REENTERED.lock().unwrap() =
                Some(retained.invoke(bytes(&("minecraft:overworld".to_string(),))));
        }
        (name == "minecraft:overworld").then_some(100)
    };
    let wanted = |name: &str| name == "w";
    let rain = |pos: BlockPos| pos.x == 1 && pos.y == 64 && pos.z == -3;
    let total = AtomicUsize::new(0);
    let mut emit = |n: u32| {
        total.fetch_add(n as usize, Ordering::Relaxed);
        total.load(Ordering::Relaxed) as u32
    };
    let mut published = 0;
    let mut publish = |n: u32, _: &std::path::Path| published += n;
    let result = parse(1, &load, &wanted, &rain, &mut emit, &mut publish);
    // The listener's invocations ran the real closures: `emit` accumulated 5 + 5, then the
    // native body added its own call with 100.
    assert_eq!(total.load(Ordering::Relaxed), 110);
    assert_eq!(result, Some(100 + 1000 + 10000 + 110 * 100000));
    assert_eq!(published, 1 + 3);
    assert_eq!(HEADS.load(Ordering::Relaxed), 1);
    // After the call ends the retained callable is refused and the closure is not run again.
    let before = calls.load(Ordering::Relaxed);
    let retained = RETAINED.lock().unwrap().take().unwrap();
    assert_eq!(
        retained.invoke(bytes(&("minecraft:overworld".to_string(),))),
        Err(RowError::Stale)
    );
    assert_eq!(calls.load(Ordering::Relaxed), before);
}
