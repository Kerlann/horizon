use super::*;

/// Helpers for native mods, kept as stable as the hook points.
// Native mods use these; nothing in Horizon does.
#[allow(dead_code)]
impl Game<'_> {
    /// The most recently extracted world frame's camera, if one exists.
    /// During GUI rendering this is the current frame, including bob/spin and FOV.
    pub(in crate::game) fn camera(&self) -> Option<NativeCamera> {
        self.native_camera_frame.clone()
    }
    /// The entity the view follows (`Minecraft.getCameraEntity`): the entity a server attached
    /// the camera to, else the local player. None outside a client world.
    pub(in crate::game) fn camera_entity(&self) -> Option<EntityRef> {
        if !self.native_window() || self.title.is_some() {
            return None;
        }
        self.player.as_ref()?;
        let id = self
            .remote_camera_id()
            .unwrap_or_else(|| self.own_entity_id() as u32);
        Some(EntityRef::new(self.native_lifecycle.epoch()?, id))
    }
    /// Collision boxes in block-local coordinates with an empty collision context.
    pub(in crate::game) fn block_collision(&self, pos: [i32; 3]) -> Vec<[f64; 6]> {
        self.native_block_collision_in(&self.world, pos)
    }
}
