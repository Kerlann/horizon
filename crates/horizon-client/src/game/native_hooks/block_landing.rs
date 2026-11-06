use super::*;

impl HitLevel<'_> {
    /// `Block.fallOn`: entity `entity` of type `kind` landed on `state` at `pos` after falling
    /// `distance` blocks. `damage` is the fall damage `causeFallDamage` calculated, before hurt
    /// handling and the fall sounds: change it, and zero prevents both. A creative or
    /// fall-immune entity is told with zero damage and keeps its immunity. Players and mobs
    /// land here; dropped items, projectiles and the other entities do not yet.
    pub(in crate::game) fn native_fall_on(&mut self, call: mod_api::__private::FallOnCall<'_>) {
        self.fire_fall_on(call);
    }
    /// `Block.bounceOn`: entity `entity` of type `kind` bounced off `state` at `pos` and fell
    /// `fall_distance` blocks. `movement` is the movement the bounce leaves it, before
    /// vanilla sets it: change it to change what the entity keeps. Players and mobs bounce
    /// here; dropped items and the other entities do not yet.
    pub(in crate::game) fn native_bounce_on(&mut self, call: mod_api::__private::BounceOnCall<'_>) {
        self.fire_bounce_on(call);
    }
}
