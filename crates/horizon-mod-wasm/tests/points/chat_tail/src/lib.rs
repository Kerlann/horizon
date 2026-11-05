//! Appends `W` to every chat input, at the `handle_chat_input` point.
use mod_api::{Hooks, HostSide};
use mod_points::{HandleChatInputArgs, Points};

#[derive(Default)]
pub struct Mod;

#[mod_api::horizon_mod]
impl Hooks for Mod {}

#[mod_api::horizon_mod]
impl Points for Mod {
    fn handle_chat_input(&self, _host: HostSide<'_>, args: &mut HandleChatInputArgs<'_>) {
        args.message = format!("{}W", args.message).into();
    }
}
