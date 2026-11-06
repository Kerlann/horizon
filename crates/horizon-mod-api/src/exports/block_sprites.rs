// hidden: a module of game-side types; mods use their re-exports
#[doc(hidden)]
pub mod block_sprites;
pub use crate::block_sprites::{BlockSprites, FaceSprite, SpriteRef};
pub use block_sprites::BlockFaceTexture;
