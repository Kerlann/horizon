// hidden: a module of game-side types; mods use their re-exports
#[doc(hidden)]
pub mod block_sprites;
pub use crate::block_sprites::{
    AtlasRegion, BlockSprites, FaceSprite, RuntimeSprite, SpriteRef, TextureAtlas,
};
pub use block_sprites::BlockFaceTexture;
