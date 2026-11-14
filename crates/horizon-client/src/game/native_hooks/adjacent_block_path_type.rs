use super::*;

impl Game<'_> {
    /// `IBlockExtension.getAdjacentBlockPathType`: set `adjacent` to change the `PathType`
    /// `state`, of type `path_type`, gives a walk node beside it; `None` leaves the node its own
    /// type. It starts with vanilla's answer for `path_type` and depends on the state alone, so
    /// the game asks once per state when the parts resolve (at load and on every `/reload`) and
    /// path finding reads the answers. Server-side.
    pub(in crate::game) fn native_adjacent_block_path_type(
        &self,
        state: BlockStateId,
        path_type: mod_api::PathType,
        adjacent: &mut Option<mod_api::PathType>,
    ) {
        self.fire_adjacent_block_path_type(state, path_type, adjacent);
    }
}
