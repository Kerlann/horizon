//! Read-only observer for the fifteen block differences found in the parity fixture.
use horizon_client::mod_api::*;
use std::sync::atomic::{AtomicU64, Ordering};

pub struct Trace {
    tick: AtomicU64,
}
impl Trace {
    pub fn new() -> Self {
        Self {
            tick: AtomicU64::new(0),
        }
    }
}
// Palette index -> x + 16*z + 256*(y%16), in chunk (0,0).
const POSITIONS: [[i32; 3]; 15] = [
    [5, 63, 10],
    [14, 64, 8],
    [12, 64, 14],
    [10, 65, 10],
    [13, 68, 11],
    [4, 68, 12],
    [6, 70, 10],
    [7, 71, 12],
    [0, 73, 9],
    [0, 73, 13],
    [7, 75, 11],
    [7, 75, 12],
    [10, 79, 13],
    [2, 79, 15],
    [9, 79, 15],
];
impl Hooks for Trace {
    fn server_tick(&self, host: &mut dyn Host, event: &mut ServerTickEvent<'_>) {
        if matches!(
            event.phase,
            ServerTickPhase::ServerTickStart {
                world_running: true
            }
        ) {
            self.tick.fetch_add(1, Ordering::Relaxed);
        }
        if matches!(
            event.phase,
            ServerTickPhase::ServerTickEnd {
                world_running: true
            }
        ) {
            let blocks: Vec<_> = POSITIONS.iter().map(|&pos| host.block(pos)).collect();
            eprintln!(
                "TRACE_END {} {}",
                self.tick.load(Ordering::Relaxed),
                serde_json::to_string(&blocks).unwrap()
            );
        }
    }
    fn random_tick(&self, level: &mut dyn LevelWriteHost, event: &mut RandomTickEvent) -> Flow {
        let state = level.random().map(|random| random.state());
        eprintln!(
            "TRACE_RNG {} {:?} {:?} {:?}",
            self.tick.load(Ordering::Relaxed),
            event.pos,
            event.state,
            state
        );
        Flow::Continue
    }
}

#[unsafe(no_mangle)]
pub fn horizon_mod_entry() -> Box<dyn Hooks> {
    Box::new(Trace::new())
}
