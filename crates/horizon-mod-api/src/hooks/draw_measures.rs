//! The `draw_measures` hooks and helpers: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Helpers {
    /// Resolve a registered standalone model once after assets publish, in `client_setup`
    /// or `client_resources_reloaded`. Retain the handle between frames. An unknown or
    /// unbaked name returns `ModelError::UnknownModel`; lookup never loads resources.
    #[helper(
        side = "client",
        service = "render_assets",
        resource_read = true,
        target = "horizon_client::game::native_block_entity_draw::Game<'_>::native_model"
    )]
    fn model();
    /// Draw a resolved standalone model under the current pose and light, tinting indexed
    /// faces by 0xRRGGBB. `ModelError::StaleModel` means assets were successfully replaced;
    /// reacquire in `client_resources_reloaded`. An error appends no geometry.
    #[helper(
        side = "client",
        target = "horizon_client::game::native_block_entity_draw::Game<'_>::native_draw_model"
    )]
    fn draw_model();
    #[helper(side = "client")]
    fn text_width();
    #[helper(side = "client")]
    fn sprite_size();
    /// Draws `text` under `draw`'s pose as a sign draws its lines
    /// (`Font.drawInBatch` in a `BlockEntityRenderer`): in the pose's x-y
    /// plane, one unit a font pixel, y down, its left end at `at`, facing
    /// +z. Scale the pose first as a sign does (`draw.scale([1.0 / 64.0,
    /// -1.0 / 64.0, 1.0 / 64.0])` for its letters, 8 pixels high). Each span
    /// is drawn in its own colour, the others in the 0xRRGGBB `color`;
    /// `glowing` text is full bright (a glow ink sac's), the rest lit by
    /// `draw.light`; a nonzero 0xRRGGBB `outline` rings each glyph, as glowing
    /// sign text's. Returns the width drawn, in font pixels: centre a line
    /// by starting it at minus half its width. API @API:draw_text@.
    #[helper(
        side = "client",
        target = "horizon_client::game::native_block_entity_draw::Game<'_>::native_draw_text"
    )]
    fn draw_text();
    #[helper(
        side = "client",
        target = "horizon_client::game::native_block_entity_draw::Game<'_>::native_draw_text_width"
    )]
    fn draw_text_width();
}
