//! A mod built to `mod.wasm` (`cargo horizon-mod build --wasm`): the same source builds as a
//! native mod too.
// ANCHOR: mod
use mod_api::{
    BlockKey, EffectKey, Flow, Hooks, HostSide, ItemPart, Registrar, RegistrationContext,
    ServerChatEvent, ServerHost, Text, UseItemEvent,
};
use mod_points::{Points, TickArgs};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Default)]
pub struct Mod {
    ticks: AtomicU64,
    stone: std::sync::OnceLock<BlockKey>,
    speed: std::sync::OnceLock<EffectKey>,
}

/// Sticks announce each use.
struct Stick;

#[mod_api::horizon_part]
impl ItemPart for Stick {
    fn use_item(&self, host: &mut dyn ServerHost, event: &mut UseItemEvent<'_>) -> Flow {
        host.messages()
            .broadcast(Text::literal(format!("a {} was used", event.stack.item)));
        Flow::Continue
    }
}

#[mod_api::horizon_mod]
impl Hooks for Mod {
    fn register(&self, registrar: &mut dyn Registrar, _cx: &RegistrationContext) {
        let item = registrar.item_key("minecraft:stick");
        registrar.item_part(item, Stick);
        let _ = self.stone.set(registrar.block_key("minecraft:stone"));
        let _ = self.speed.set(registrar.effect_key("minecraft:speed"));
    }

    fn server_chat(&self, host: &mut dyn ServerHost, event: &mut ServerChatEvent<'_>) -> Flow {
        let stone = *self.stone.get().expect("registered block key");
        let speed = *self.speed.get().expect("registered effect key");
        assert_eq!(stone.id(host), "minecraft:stone");
        assert_eq!(speed.id(host), "minecraft:speed");
        assert!(host.block_state_default(stone).is_some());
        *event.message = Text::literal(format!("{} (via mod.wasm)", event.raw_text));
        Flow::Continue
    }
}

/// Listens at the game's tick, a point its `horizon-mod.toml` names: every 200 server ticks it
/// says so in chat.
#[mod_api::horizon_mod]
impl Points for Mod {
    fn tick(&self, host: HostSide<'_>, _args: &mut TickArgs) {
        let HostSide::Server(host) = host else {
            return;
        };
        let ticks = self.ticks.fetch_add(1, Ordering::Relaxed) + 1;
        if ticks % 200 == 0 {
            host.messages()
                .broadcast(Text::literal(format!("hello_wasm: {ticks} server ticks")));
        }
    }
}
// ANCHOR_END: mod
