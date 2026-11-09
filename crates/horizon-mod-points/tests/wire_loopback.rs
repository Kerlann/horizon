// Generated fixtures exercise every shape; TODO(#2273): make the generator emit clippy-clean code.
#![allow(clippy::all, unused_comparisons, unreachable_patterns, dead_code)]
#![cfg(feature = "wasm-guest")]

use mod_api::wire::{Reader, Record, Writer};
use mod_points::{
    __private::{Forward, PointHost, RawForward, SlotListener},
    PointId,
};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

struct Quiet;
impl mod_api::Host for Quiet {}
mod_api::impl_host_areas!(Quiet);
impl mod_api::ServerHost for Quiet {}
mod_api::impl_server_host_areas!(Quiet);
#[mod_api::__subscriptions]
impl mod_api::Hooks for Quiet {}

#[unsafe(no_mangle)]
pub fn __horizon_mod_hooks() -> Box<dyn mod_api::Hooks> {
    Box::new(Quiet)
}

static HEAD: AtomicUsize = AtomicUsize::new(0);
static RETURN: AtomicUsize = AtomicUsize::new(0);
fn head(_: mod_api::HostSide<'_>, args: &mut mod_points::TickArgs) {
    HEAD.fetch_add(1, Ordering::Relaxed);
    args.cancel = true;
}
fn tail(_: mod_api::HostSide<'_>, args: &mut mod_points::TickArgs) {
    RETURN.fetch_add(1, Ordering::Relaxed);
    args.cancel = false;
}

struct Loopback(u32);
impl Forward for Loopback {
    fn forward(&self, id: u32, host: PointHost<'_>, record: &mut dyn Record) {
        let mut input = Writer::new();
        input.put(&self.0);
        input.put(&matches!(
            host,
            PointHost::Side(mod_api::HostSide::Server(_))
        ));
        loopback(id, input, record);
    }
}

fn loopback(id: u32, mut input: Writer, record: &mut dyn Record) {
    record.encode(&mut input);
    let pointer = mod_api::wire::guest::input(input.bytes().len());
    // SAFETY: input() reserves this many writable bytes until run().
    unsafe {
        std::ptr::copy_nonoverlapping(input.bytes().as_ptr(), pointer, input.bytes().len());
    }
    let (pointer, len) = mod_api::wire::guest::run(id);
    assert_ne!(len, u32::MAX);
    // SAFETY: run() retains this reply until the next call at this depth.
    let reply = unsafe { std::slice::from_raw_parts(pointer, len as usize) };
    let mut reply = Reader::new(reply);
    reply.take::<()>().unwrap();
    let bad_return = id == 0xC000_0000 + PointId::GameServerLinksListTopReturn as u32
        && BAD_RAW_RETURN.swap(false, Ordering::Relaxed);
    if REJECT_READONLY.swap(false, Ordering::Relaxed) {
        assert!(record.apply(reply.rest()).is_err());
    } else if id & 0xC000_0000 == 0xC000_0000
        && (BAD_RAW.swap(false, Ordering::Relaxed) || bad_return)
    {
        assert!(
            record
                .apply(&reply.rest()[..reply.rest().len() - 1])
                .is_err()
        );
    } else {
        record.apply(reply.rest()).unwrap();
    }
}

impl RawForward for Loopback {
    fn forward(&self, id: u32, record: &mut dyn Record) {
        let mut input = Writer::new();
        input.put(&self.0);
        loopback(id, input, record);
    }
}

static LOOPBACK_RAW: Loopback = Loopback(2);
static LOOPBACK_RAW_RETURN: Loopback = Loopback(3);
static RAW: AtomicUsize = AtomicUsize::new(0);
static BAD_RAW: AtomicBool = AtomicBool::new(false);
static BAD_RAW_RETURN: AtomicBool = AtomicBool::new(false);
static RAW_RETURN: AtomicUsize = AtomicUsize::new(0);
static FORGE_READONLY: AtomicBool = AtomicBool::new(false);
static REJECT_READONLY: AtomicBool = AtomicBool::new(false);
static BODY: AtomicUsize = AtomicUsize::new(0);
fn raw(snapshot: mod_points::snapshot::Snapshot<'_>, args: &mut mod_api::wire::RecordPatch) {
    assert_eq!(
        snapshot.read::<f32>(snapshot.field(0, 0).unwrap()).unwrap(),
        3.0
    );
    assert_eq!(
        snapshot.read::<u64>(snapshot.field(0, 1).unwrap()).unwrap(),
        7
    );
    let control = snapshot.field(0, 2).unwrap();
    assert_eq!(snapshot.read::<Option<f32>>(control).unwrap(), None);
    args.set(snapshot.field(0, 0).unwrap(), &999.0f32).unwrap();
    args.set(snapshot.field(0, 1).unwrap(), &999u64).unwrap();
    let result = if RAW.fetch_add(1, Ordering::Relaxed) == 1 {
        Some(42.5f32)
    } else {
        None
    };
    args.set(control, &result).unwrap();
}

fn raw_return(snapshot: mod_points::snapshot::Snapshot<'_>, args: &mut mod_api::wire::RecordPatch) {
    let call = RAW_RETURN.fetch_add(1, Ordering::Relaxed);
    let control = snapshot.field(0, 2).unwrap();
    let value = snapshot.read::<f32>(control).unwrap();
    assert_eq!(value, if call == 1 { 10.0 } else { 1998.0 });
    if FORGE_READONLY.swap(false, Ordering::Relaxed) {
        args.set(snapshot.field(0, 0).unwrap(), &555.0f32).unwrap();
        REJECT_READONLY.store(true, Ordering::Relaxed);
    }
    args.set(control, &(value + 1.0)).unwrap();
}

#[horizon_mod_api_macros::point(name = "game_server_links_list_top", raw, free)]
fn fixture_list_top(h: f32, n: usize) -> f32 {
    BODY.fetch_add(1, Ordering::Relaxed);
    h + n as f32
}

pub struct __GameServerLinksListTopRaw<'r, H, N, R> {
    pub h: &'r mut H,
    pub n: &'r mut N,
    pub result: &'r mut Option<R>,
}
pub type __GameServerLinksListTopListener =
    for<'r> fn(&'r mut __GameServerLinksListTopRaw<'r, f32, usize, f32>);
pub type __GameServerLinksListTopReturnListener = fn(&mut f32);

static LOOPBACK_HEAD: Loopback = Loopback(0);
static LOOPBACK_RETURN: Loopback = Loopback(1);

#[test]
fn head_and_return_share_the_hook_arena_and_native_listener_list() {
    mod_points::guest::register(&mod_points::listeners::tick("guest", 0, "head", head)).unwrap();
    mod_points::guest::register(&mod_points::listeners::tick_return(
        "guest", 0, "return", tail,
    ))
    .unwrap();
    assert_eq!(
        mod_points::guest::subscriptions(),
        [
            (PointId::Tick as u32, "head".into()),
            (PointId::TickReturn as u32, "return".into())
        ]
    );
    for (place, endpoint) in [
        (PointId::Tick, &LOOPBACK_HEAD),
        (PointId::TickReturn, &LOOPBACK_RETURN),
    ] {
        mod_points::register(&SlotListener::forwarded(
            "guest",
            place,
            endpoint,
            0x8000_0000 + place as u32,
        ))
        .unwrap();
    }
    mod_points::guest::register_raw(&mod_points::raw_listeners::game_server_links_list_top(
        "raw", raw,
    ))
    .unwrap();
    mod_points::register(
        &SlotListener::snapshot(
            "guest",
            PointId::GameServerLinksListTop,
            &LOOPBACK_RAW,
            0xC000_0000 + PointId::GameServerLinksListTop as u32,
        )
        .unwrap(),
    )
    .unwrap();
    mod_points::guest::register_raw(
        &mod_points::raw_listeners::game_server_links_list_top_return("raw_return", raw_return),
    )
    .unwrap();
    mod_points::register(
        &SlotListener::snapshot(
            "guest",
            PointId::GameServerLinksListTopReturn,
            &LOOPBACK_RAW_RETURN,
            0xC000_0000 + PointId::GameServerLinksListTopReturn as u32,
        )
        .unwrap(),
    )
    .unwrap();
    mod_points::__private::seal();
    let mut args = mod_points::TickArgs::__new();
    mod_points::dispatch::tick(mod_api::HostSide::Server(&mut Quiet), &mut args);
    assert!(args.cancel);
    mod_points::dispatch::tick_return(mod_api::HostSide::Server(&mut Quiet), &mut args);
    assert!(!args.cancel);
    assert_eq!(
        (HEAD.load(Ordering::Relaxed), RETURN.load(Ordering::Relaxed)),
        (1, 1)
    );
    assert_eq!(
        fixture_list_top(3.0, 7),
        1999.0,
        "raw head record writes replace by-value inputs; the raw return listener changes the outcome"
    );
    assert_eq!(
        fixture_list_top(3.0, 7),
        42.5,
        "declared outcome writes cancel the body"
    );
    assert_eq!(BODY.load(Ordering::Relaxed), 1);
    assert_eq!(RAW.load(Ordering::Relaxed), 2);
    BAD_RAW.store(true, Ordering::Relaxed);
    assert_eq!(
        fixture_list_top(3.0, 7),
        11.0,
        "a rejected raw head reply applies no writes; the valid return listener still adds one"
    );
    assert_eq!(BODY.load(Ordering::Relaxed), 2);
    BAD_RAW_RETURN.store(true, Ordering::Relaxed);
    assert_eq!(
        fixture_list_top(3.0, 7),
        1998.0,
        "a rejected raw return reply does not replace the outcome"
    );
    assert_eq!(BODY.load(Ordering::Relaxed), 3);
    assert_eq!(RAW_RETURN.load(Ordering::Relaxed), 3);
    FORGE_READONLY.store(true, Ordering::Relaxed);
    assert_eq!(
        fixture_list_top(3.0, 7),
        1998.0,
        "one readonly argument edit rejects the entire sparse return patch"
    );
    assert_eq!(BODY.load(Ordering::Relaxed), 4);
    assert_eq!(RAW_RETURN.load(Ordering::Relaxed), 4);
    mod_points::stop("guest");
    mod_points::dispatch::tick(mod_api::HostSide::Server(&mut Quiet), &mut args);
    assert!(!args.cancel);
    assert_eq!(HEAD.load(Ordering::Relaxed), 1);
    assert_eq!(fixture_list_top(3.0, 7), 10.0);
    assert_eq!(RAW.load(Ordering::Relaxed), 5);
    assert_eq!(RAW_RETURN.load(Ordering::Relaxed), 4);
}

#[test]
fn malformed_writeback_is_atomic() {
    let mut args = mod_points::TickArgs::__new();
    assert!(
        args.apply(&[1, 0]).is_err(),
        "trailing bytes are not a partial update"
    );
    assert!(!args.cancel);
    assert!(args.apply(&[]).is_err());
    assert!(!args.cancel);
    args.apply(&[1]).unwrap();
    assert!(args.cancel);
}
