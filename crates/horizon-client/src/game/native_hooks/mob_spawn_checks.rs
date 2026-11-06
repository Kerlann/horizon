use super::*;

/// The hook points. Each does nothing in Horizon.
impl Game<'_> {
    /// `MobSpawnEvent.SpawnPlacementCheck`: the entity type `kind` may spawn
    /// at `pos` for `reason` (light, ground, biome: `SpawnPlacements.checkSpawnRules`).
    /// `default_result` is what vanilla's predicate answered; `result` starts
    /// as `Default` (vanilla's answer stands). Server-side.
    pub(in crate::game) fn native_mob_spawn_placement_check(
        &self,
        dimension: &str,
        kind: &str,
        reason: NativeSpawnReason,
        pos: BlockPos,
        default_result: bool,
        result: &mut NativeSpawnCheck,
    ) {
        self.fire_mob_spawn_placement_check(dimension, kind, reason, pos, default_result, result);
    }
    /// `MobSpawnEvent.PositionCheck`: the freshly created mob of type `kind`
    /// may stand at `pos` for `reason` (`Mob.checkSpawnRules` and
    /// `checkSpawnObstruction`). `result` starts as `Default`: vanilla's two
    /// checks decide; `Succeed` or `Fail` skips them. Server-side.
    pub(in crate::game) fn native_mob_spawn_position_check(
        &self,
        dimension: &str,
        kind: &str,
        reason: NativeSpawnReason,
        pos: [f64; 3],
        result: &mut NativeSpawnCheck,
    ) {
        self.fire_mob_spawn_position_check(dimension, kind, reason, pos, result);
    }
}
