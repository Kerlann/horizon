//! `LivingEntity.onClimbable` for the player: the block it stands in, and the mods' say on whether
//! that block is a ladder (`native_is_ladder`, in `motion`) and on whether the block under an open
//! trapdoor makes it one (`native_makes_open_trapdoor_above_climbable`).
use super::*;

/// Whether an open trapdoor above a block climbs like a ladder
/// (`IBlockExtension.makesOpenTrapdoorAboveClimbable`, which `LivingEntity.onClimbable` asks of
/// the block under the open trapdoor an entity stands in) is being decided. `state` is that
/// block's state at `pos` and `trapdoor` the trapdoor's state; `climbable` starts with vanilla's
/// answer, a `minecraft:ladder` facing the trapdoor's way, and the block's parts may change it.
/// Fires on replayed ticks too, so a listener must be a pure function of its inputs.
#[inline(never)]
pub fn native_makes_open_trapdoor_above_climbable(
    context: &LoopContext,
    state: BlockStateId,
    pos: BlockPos,
    trapdoor: BlockStateId,
    climbable: &mut bool,
) {
    crate::native_client_hooks::makes_open_trapdoor_above_climbable(
        context, state, pos, trapdoor, climbable,
    );
}

impl ClimbingWorld for MotionWorld<'_, '_, '_> {
    type Error = std::convert::Infallible;
    fn is_spectator(&mut self) -> std::result::Result<bool, Self::Error> {
        Ok(self.player.spectator)
    }
    fn block_position(&mut self) -> std::result::Result<BlockPos, Self::Error> {
        Ok(BlockPos::containing(self.player.position))
    }
    fn in_block_state(&mut self) -> std::result::Result<BlockStateId, Self::Error> {
        Ok(self.block(BlockPos::containing(self.player.position)))
    }
    fn is_fall_flying(&mut self) -> std::result::Result<bool, Self::Error> {
        Ok(self.player.fall_flying)
    }
    fn block_state(
        &mut self,
        position: BlockPos,
    ) -> std::result::Result<BlockStateId, Self::Error> {
        Ok(self.block(position))
    }
    fn set_last_climbable_position(&mut self, position: BlockPos) {
        self.player.last_climbable = Some(position);
    }
    fn adjust_ladder(&mut self, position: BlockPos, state: BlockStateId, tagged: bool) -> bool {
        let mut ladder = tagged;
        native_is_ladder(
            self.loop_context,
            self.player.entity_id as u32,
            position,
            state,
            &mut ladder,
        );
        ladder
    }
    fn adjust_trapdoor_ladder(
        &mut self,
        position: BlockPos,
        state: BlockStateId,
        trapdoor: BlockStateId,
        ladder: bool,
    ) -> bool {
        let mut climbable = ladder;
        native_makes_open_trapdoor_above_climbable(
            self.loop_context,
            state,
            position,
            trapdoor,
            &mut climbable,
        );
        climbable
    }
}
