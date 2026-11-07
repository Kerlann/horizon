//! Appends `W` to every chat input, at the `handle_chat_input` point.
use mod_api::{Hooks, HostSide};
use mod_points::HandleChatInputArgs;

#[derive(Default)]
pub struct Mod;

#[mod_api::horizon_mod]
impl Hooks for Mod {}

pub fn handle_chat_input(_host: HostSide<'_>, args: &mut HandleChatInputArgs<'_>) {
    args.message = format!("{}W", args.message).into();
}

pub fn other_tail(_host: HostSide<'_>, args: &mut HandleChatInputArgs<'_>) {
    args.message = format!("{}V", args.message).into();
}

#[cfg(target_arch = "wasm32")]
include!(concat!(env!("OUT_DIR"), "/guest_injects.rs"));
