// Generated fixtures exercise every shape; TODO(#2273): make the generator emit clippy-clean code.
#![allow(clippy::all, unused_comparisons, unreachable_patterns, dead_code)]
#![cfg(feature = "wasm-guest")]

//! A real catalog point with a trait-object argument, `Loading::menu_action(&mut self,
//! event_loop: &dyn EventLoopCtl, action: Action)`, on a stand-in receiver. The trait is a listed
//! catalog trait, so the argument crosses as a table with one host-held callable per method, in
//! declaration order (`exit`, `set_control_flow(ControlFlow)`), made by the generic generator
//! rule. The callables run the real object within the call's lease: refused after the call ends,
//! on another thread and re-entrantly. No edit reaches the table. The object stays usable by the
//! native body.
mod common;
use common::Endpoint;
use mod_api::{RowError, RowFunction, wire::Writer};
use mod_points::{__private::SlotListener, PointId, endpoint_policy::EndpointPolicy};
use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

use startup_menu::{Action, ControlFlow, Loading};
pub mod startup_menu {
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub enum Action {
        Quit,
        Options,
    }
    /// The shape of winit's `ControlFlow`.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub enum ControlFlow {
        Poll,
        Wait,
        WaitUntil(std::time::Instant),
    }
    pub struct Loading {
        pub actions: u32,
    }
}
pub trait EventLoopCtl {
    fn exit(&self);
    fn set_control_flow(&self, flow: ControlFlow);
    fn bump(&self, n: &mut u32) -> bool;
}
#[derive(Default)]
struct Ctl {
    exits: AtomicUsize,
    flows: Mutex<Vec<ControlFlow>>,
}
impl EventLoopCtl for Ctl {
    fn exit(&self) {
        self.exits.fetch_add(1, Ordering::Relaxed);
        if REENTER.swap(false, Ordering::Relaxed) {
            let retained = RETAINED.lock().unwrap().clone().unwrap();
            *REENTERED.lock().unwrap() = Some(retained.invoke(bytes(&())));
        }
    }
    fn set_control_flow(&self, flow: ControlFlow) {
        self.flows.lock().unwrap().push(flow);
    }
    fn bump(&self, n: &mut u32) -> bool {
        *n += 10;
        *n > 12
    }
}
include!(concat!(env!("OUT_DIR"), "/native_raw_unclassified.rs"));

static MODE: AtomicUsize = AtomicUsize::new(0);
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

fn head(snapshot: mod_points::snapshot::Snapshot<'_>, edits: &mut mod_api::wire::RecordPatch) {
    let first = HEADS.fetch_add(1, Ordering::Relaxed) == 0;
    // Roots: receiver, event_loop, action, cancel.
    let table = snapshot.field(0, 1).unwrap();
    assert!(snapshot.field(0, 2).is_some() && snapshot.field(0, 3).is_some());
    assert!(snapshot.ty(table).is_some());
    // The table's methods arrive in a lazy window, as a receiver's members do.
    let rows = snapshot
        .window(mod_api::RowWindow {
            node: table,
            start: 0,
            rows: 8,
            depth: 2,
        })
        .unwrap();
    let owned = mod_points::snapshot::Snapshot::new(&rows);
    let method = |ordinal| {
        owned
            .read::<Arc<dyn RowFunction>>(owned.field(table, ordinal).unwrap())
            .unwrap()
    };
    // Three methods, and no fourth.
    assert!(owned.field(table, 3).is_none());
    let (exit, set_flow, bump) = (method(0), method(1), method(2));
    if MODE.load(Ordering::Relaxed) == 0 && first {
        // Methods are called through the real object.
        assert_eq!(exit.invoke(bytes(&())).unwrap(), bytes(&()));
        set_flow.invoke(bytes(&(1u32,))).unwrap();
        // `WaitUntil` carries its instant as nanoseconds from now.
        set_flow.invoke(bytes(&(2u32, 5_000_000_000u64))).unwrap();
        // A `&mut u32` argument is lent exclusively; its final value returns beside the result.
        assert_eq!(
            mod_api::wire::Reader::decode::<(bool, u32)>(&bump.invoke(bytes(&(5u32,))).unwrap()),
            Ok((true, 15))
        );
        assert_eq!(
            mod_api::wire::Reader::decode::<(bool, u32)>(&bump.invoke(bytes(&(1u32,))).unwrap()),
            Ok((false, 11))
        );
        // An unknown variant, short and trailing input are refused before the method runs.
        assert!(set_flow.invoke(bytes(&(9u32,))).is_err());
        assert!(set_flow.invoke(vec![]).is_err());
        assert!(exit.invoke(vec![0]).is_err());
        // Another thread is refused.
        let other = Arc::clone(&exit);
        assert_eq!(
            std::thread::spawn(move || other.invoke(bytes(&())))
                .join()
                .unwrap(),
            Err(RowError::WrongThread)
        );
        // Re-entrancy: the object's method calls the same callable back while it runs.
        *RETAINED.lock().unwrap() = Some(Arc::clone(&exit));
        REENTER.store(true, Ordering::Relaxed);
        exit.invoke(bytes(&())).unwrap();
        assert_eq!(
            REENTERED.lock().unwrap().take(),
            Some(Err(RowError::Reentrant))
        );
    } else if MODE.load(Ordering::Relaxed) == 1 {
        // A cancel edit next to the table, and an edit of the table itself.
        edits.set(snapshot.field(0, 3).unwrap(), &true).unwrap();
        edits.set(table, &0u32).unwrap();
    }
}

#[test]
fn trait_object_argument_crosses_as_a_method_table() {
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
        &mod_points::raw_listeners::game_startup_menu_loading_menu_action("table", head),
    )
    .unwrap();
    mod_points::register(
        &SlotListener::snapshot(
            "table-fixture",
            PointId::GameStartupMenuLoadingMenuAction,
            HEAD.get().unwrap(),
            0xC000_0000 + PointId::GameStartupMenuLoadingMenuAction as u32,
        )
        .unwrap(),
    )
    .unwrap();
    mod_points::__private::seal();
    let ctl = Ctl::default();
    let mut menu = Loading { actions: 0 };
    // The native body gets the real object, never a copy.
    menu.menu_action(&ctl, Action::Quit);
    menu.menu_action(&ctl, Action::Options);
    assert_eq!((menu.actions, HEADS.load(Ordering::Relaxed)), (1, 2));
    // The listener's two exits (the second is the re-entrancy probe's outer call) and the
    // body's own exit for `Quit`.
    assert_eq!(ctl.exits.load(Ordering::Relaxed), 3);
    let flows = ctl.flows.lock().unwrap().clone();
    assert_eq!(flows.len(), 2);
    assert_eq!(flows[0], ControlFlow::Wait);
    let ControlFlow::WaitUntil(at) = flows[1] else {
        panic!("{:?}", flows[1])
    };
    let ahead = at.saturating_duration_since(std::time::Instant::now());
    assert!(
        ahead > std::time::Duration::from_secs(3) && ahead <= std::time::Duration::from_secs(5)
    );
    // After the call ends the retained callable is refused and the object is not touched.
    let before = ctl.exits.load(Ordering::Relaxed);
    let retained = RETAINED.lock().unwrap().take().unwrap();
    assert_eq!(retained.invoke(bytes(&())), Err(RowError::Stale));
    assert_eq!(ctl.exits.load(Ordering::Relaxed), before);
    // An edit of the table refuses the whole patch, so the cancel next to it is not applied.
    MODE.store(1, Ordering::Relaxed);
    REFUSED.store(true, Ordering::Relaxed);
    menu.menu_action(&ctl, Action::Quit);
    assert_eq!(menu.actions, 1);
    assert_eq!(HEADS.load(Ordering::Relaxed), 3);
}
