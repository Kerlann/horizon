//! Declarations carried from upstream `screen_transparent_background`; the generator owns the typed bindings.

pub trait Hooks {
    /// `Screen.renderTransparentBackground`: draws the translucent dark background
    /// behind an in-game screen (inventory, container, creative, book, sign, editor).
    /// Set `canceled` (or return `Flow::Cancel`) to suppress the vanilla dark gradient,
    /// allowing a mod to draw its own background or apply menu background blur
    /// via `Host::apply_screen_blur`. API @API:screen_transparent_background@.
    #[hook(
        side = "client",
        contract = "cancel",
        neoforge = "ScreenEvent.Render.Pre (transparent background)",
        cancellable = true,
        out = "canceled",
        target = "horizon_client::game::native_hooks::screen_transparent_background::Game<'_>::native_screen_transparent_background"
    )]
    fn screen_transparent_background();
}

pub trait Helpers {
    /// Requests vanilla's menu background blur for this frame, with a per-frame guard
    /// so blur is applied at most once per frame. Returns true if blur was newly
    /// requested this frame, or false if it was already applied/requested this frame.
    /// API @API:screen_transparent_background@.
    #[helper(
        side = "client",
        target = "horizon_client::game::native_hooks::screen_transparent_background::Game<'_>::native_apply_screen_blur"
    )]
    fn apply_screen_blur();
    /// The user's configured menu background blur radius (`menuBackgroundBlurriness` in options.txt,
    /// usually 0.0 to 10.0; default 5.0). 0.0 means blur is turned off. API @API:screen_transparent_background@.
    #[helper(
        side = "client",
        target = "horizon_client::game::native_hooks::screen_transparent_background::Game<'_>::native_menu_blur_radius"
    )]
    fn menu_blur_radius();
    /// `Screen.renderMenuBackground`: draws the tiled menu background (32x32) across `rect`
    /// (`[x, y, w, h]` in GUI pixels). If `in_world` is true, uses the dark translucent
    /// texture (`gui/inworld_menu_background`); otherwise uses the opaque dirt-like
    /// texture (`gui/menu_background`). API @API:screen_transparent_background@.
    #[helper(
        side = "client",
        target = "horizon_client::game::native_hooks::screen_transparent_background::Game<'_>::native_draw_menu_background"
    )]
    fn draw_menu_background();
    /// Repeats sprite `sprite` across `rect` (`[x, y, w, h]` in GUI pixels) with each tile
    /// sized `size` GUI pixels square, tinted by `argb`. False when the sprite is not in the HUD atlas.
    /// API @API:screen_transparent_background@.
    #[helper(
        side = "client",
        target = "horizon_client::game::native_hooks::screen_transparent_background::Game<'_>::native_tiled_sprite"
    )]
    fn tiled_sprite();
}
