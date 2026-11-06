//! The `extract_block_screen_effect` hooks and helpers: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// Before one of the first-person view's block overlays (`ScreenEffectRenderer`) is drawn,
    /// once a frame each is shown: `Block` with the view-blocking `state` at `pos` the camera is
    /// inside, `Water` with the `state` at the eye's block `pos` while the eye is under water, and
    /// `Fire` with fire's default `state` at the player's block `pos` while they burn. Never for a
    /// spectator; `Fire` only in first person. Set `canceled` to leave that overlay out; the
    /// frame, the other block overlays and the HUD's camera overlays are drawn as before.
    /// Client-side.
    #[hook(
        side = "client",
        contract = "cancel",
        out = "canceled",
        neoforge = "ExtractBlockScreenEffectEvent",
        cancellable = true
    )]
    fn extract_block_screen_effect();
}
