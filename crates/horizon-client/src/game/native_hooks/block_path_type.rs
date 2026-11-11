use super::*;

impl Game<'_> {
    /// `IBlockExtension.getBlockPathType`: set `path_type` to change the `PathType` `state` is to
    /// a mob's path finder. It starts with vanilla's answer after the state's `is_pathfindable`
    /// answers and depends on the state alone, so the game asks once per state when the parts
    /// resolve (at load and on every `/reload`) and path finding reads the answers. Server-side.
    pub(in crate::game) fn native_block_path_type(
        &self,
        state: BlockStateId,
        path_type: &mut mod_api::PathType,
    ) {
        self.fire_block_path_type(state, path_type);
    }
}
