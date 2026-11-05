//! Depends on `counter_lib` and listens at `handle_chat_input`, appending `L`.
use mod_api::{Hooks, HostSide};
use mod_points::{HandleChatInputArgs, Points};

#[derive(Default)]
pub struct Mod;

#[mod_api::horizon_mod]
impl Hooks for Mod {}

#[mod_api::horizon_mod]
impl Points for Mod {
    fn handle_chat_input(&self, _host: HostSide<'_>, args: &mut HandleChatInputArgs<'_>) {
        let _ = &counter_lib::COUNTER;
        args.message = format!("{}L", args.message).into();
    }
}
