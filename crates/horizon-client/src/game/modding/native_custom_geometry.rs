//! The typed submission surface; renderer storage is never exposed to mods.
use super::Game;
use super::mods;
use glam::Mat4;
use horizon_client::world_geometry::WorldGeometry;
use mod_api::{DrawError, WorldDrawUsage, WorldLine, WorldQuad};

impl Game<'_> {
    /// `DebugRenderer.refreshRendererList`: the active renderers are replaced by those the mods
    /// push now whose entries are on. Runs at start, at login and when the selection changes.
    pub(in crate::game) fn refresh_debug_renderers(&mut self) {
        let renderers = mods::debug_renderers(self.options.execution_role);
        let reduced = self.reduced_debug_info();
        self.debug_renderers = self
            .debug_entries
            .active_renderers(renderers, self.debug, reduced);
    }
    /// `DebugRenderer.emitGizmos`'s first half: a selection the list has not seen refreshes it.
    fn follow_debug_selection(&mut self) {
        let version = self.debug_entries.enabled_version();
        if self.debug_renderers_version != Some(version) {
            self.debug_renderers_version = Some(version);
            self.refresh_debug_renderers();
        }
    }
    /// One view's geometry, the last view's gone first. `title` views draw no world: the
    /// title and connect screens have no mod geometry, so none outlives leaving a world.
    pub(super) fn fill_world_geometry(
        &mut self,
        title: bool,
        camera: [f64; 3],
        view: Mat4,
        alpha: f64,
        draw: &mut WorldGeometry,
    ) {
        let white = self.gui.atlas.sprites["horizon/white"];
        draw.reset(
            camera,
            view,
            [(white.u0 + white.u1) * 0.5, (white.v0 + white.v1) * 0.5],
        );
        if title {
            return;
        }
        let view = mod_api::WorldRenderView::new(camera, alpha as f32, self.world.game_time);
        self.native_submit_custom_geometry(&view, draw);
        self.follow_debug_selection();
        for id in &self.debug_renderers {
            self.native_debug_renderer_draw(id, &view, draw);
        }
    }
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
