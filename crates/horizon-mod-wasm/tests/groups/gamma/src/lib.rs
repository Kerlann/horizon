//! Asks a capability named like counter_lib's, with a handler trait of its own: alpha, in
//! another group, answers counter_lib's, which this mod isn't linked with.
use mod_api::{CapabilityKey, Flow, Hooks, ServerChatEvent, ServerHost, Text};

/// This mod's own counter.
pub trait Counter: Send + Sync {
    /// Its count.
    fn count(&self) -> u32;
}

const COUNTER: CapabilityKey<dyn Counter> = CapabilityKey::new("test:counter");

/// Where the test world's counter box is.
const BOX: [i32; 3] = [0, 64, 0];

#[derive(Default)]
pub struct Mod;

#[mod_api::horizon_mod]
impl Hooks for Mod {
    fn server_chat(&self, host: &mut dyn ServerHost, event: &mut ServerChatEvent<'_>) -> Flow {
        let answer = host
            .level("minecraft:overworld")
            .and_then(|level| level.capability(host, &COUNTER, BOX, None))
            .map_or("none".to_string(), |counter| counter.count().to_string());
        *event.message = Text::literal(format!("{}: {answer}", env!("CARGO_PKG_NAME")));
        Flow::Continue
    }
}
