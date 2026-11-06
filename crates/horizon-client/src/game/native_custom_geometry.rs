//! The typed submission surface; renderer storage is never exposed to mods.
use super::Game;
use horizon_client::world_geometry::WorldGeometry;
use mod_api::{DrawError, WorldDrawUsage, WorldLine, WorldQuad};

impl Game<'_> {
    /// Submit unlit ARGB world lines atomically. Invalid coordinates or widths,
    /// or a batch exceeding the view's fixed capacity, leave the batch unchanged.
    pub(super) fn native_world_draw_lines(
        &self,
        draw: &mut WorldGeometry,
        lines: &[WorldLine],
    ) -> Result<(), DrawError> {
        draw.lines(lines)
    }
    /// Submit unlit, alpha-blended world quads atomically, using the white atlas texel.
    pub(super) fn native_world_draw_quads(
        &self,
        draw: &mut WorldGeometry,
        quads: &[WorldQuad],
    ) -> Result<(), DrawError> {
        draw.quads(quads)
    }
    /// Queued counts and fixed capacities, shared by both depth modes and all listeners.
    /// Capacity is allocated during setup, never grown during submission.
    pub(super) fn native_world_draw_usage(&self, draw: &WorldGeometry) -> WorldDrawUsage {
        draw.usage()
    }
}
