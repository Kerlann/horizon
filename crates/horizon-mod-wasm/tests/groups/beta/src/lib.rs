//! Asks counter_lib's capability, which a mod of its group answers.
use counter_lib::COUNTER;
use mod_api::{Flow, Hooks, ServerChatEvent, ServerHost, Text};

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
