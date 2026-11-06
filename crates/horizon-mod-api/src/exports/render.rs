// hidden: a module of game-side types; mods use their re-exports
#[doc(hidden)]
pub mod render;
pub use crate::render::{
    BlockSpriteArrays, RenderHookId, RenderHooks, RenderNeeds, RenderNeedsEvent, WorldDrawn,
    WorldDrawnEvent, render_hook_ids, wgpu,
};
mod __private {
    mod built {
        pub use crate::render::built::{BlockSpriteArrays, HandDrawn, WorldDrawn};
    }
}
