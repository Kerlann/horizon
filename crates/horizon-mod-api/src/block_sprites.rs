//! Moved here from the game so mods can name it.
#[allow(unused_imports)]
use crate::*;
/// A sprite: its array (0 for every vanilla texture on most GPUs; past the first array's layers in
/// 1 to 7) and its layer in that array.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
// exhaustive: the game builds it with a literal; mods only read it
pub struct SpriteRef {
    pub array: u32,
    pub layer: u32,
    /// It animates (water, lava, fire, sea lanterns...): the arrays always hold the frame being shown.
    pub animated: bool,
    /// Some of its pixels are clear (alpha 0): leaves, glass, flowers. Draw those as holes.
    pub cutout: bool,
}
/// One face of a block's model on one of its sides.
#[derive(Clone, Copy, Debug, PartialEq)]
// exhaustive: the game builds it with a literal; mods only read it
pub struct FaceSprite {
    pub sprite: SpriteRef,
    /// Where the face lies on the side, in the side's two axes (`side_axes`), from 0 to 1 across the block:
    /// `[a0, b0, a1, b1]`. `[0, 0, 1, 1]` for a whole face.
    pub rect: [f32; 4],
    /// How deep into the block the face lies from the side's own plane (0 for a cube's face, 0.5 for a slab's top).
    pub inset: f32,
    /// The texture across the side: the point (a, b) on it shows `uv = [u0, v0] + a * [du_u, du_v] + b * [dv_u,
    /// dv_v]` of the sprite, `[u0, v0, du_u, du_v, dv_u, dv_v]`. Rotations and mirrors (a log on its side, a
    /// model's `rotation` and `uvlock`) are in it.
    pub uv: [f32; 6],
    /// Its colour source, none for an untinted face: multiply the texture by the source's colour.
    pub tint: Option<TintSource>,
    /// It has partly clear pixels: drawn sorted and blended.
    pub translucent: bool,
}
/// What a block draws on each side, in [`Direction::ALL`]'s order (down, up, north, south, west, east).
#[derive(Clone, Debug, PartialEq)]
// exhaustive: the game builds it with a literal; mods only read it
pub struct BlockSprites {
    /// The faces on each side, in the model's order: the one drawn first first (a grass block's side: its dirt
    /// and grass, then the grass overlay tinted over it). A side with none is empty. Faces that do not lie flat
    /// on a side (a flower's crossed planes, a rotated element) are left out.
    pub sides: [Vec<FaceSprite>; 6],
    /// The model's particle sprite (what breaking it scatters, a flower's whole picture), if it has one.
    pub particle: Option<SpriteRef>,
    /// The model is a plain cube: one whole face on each side, at its plane.
    pub cube: bool,
}

/// A model face's texture name and tint selector, as block drawing helpers accept them.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct BlockFaceTexture {
    /// Texture name, for example `block/oak_planks` or a mod's namespaced texture.
    pub texture: String,
    /// Model tint index; -1 means no tint.
    pub tint_index: i32,
}

impl BlockFaceTexture {
    /// Describe a face texture without exposing the renderer's atlas or model types.
    pub fn new(texture: String, tint_index: i32) -> Self {
        Self {
            texture,
            tint_index,
        }
    }
}

/// An atlas the client stitches its sprites into (vanilla's `TextureAtlas`), as
/// `texture_atlas_stitched` names it. Horizon packs vanilla's many atlases into these two.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TextureAtlas {
    /// The block sprite arrays: every texture a block model uses, one layer each. Look a sprite up
    /// with `texture_sprites`, which answers a [`SpriteRef`].
    Blocks,
    /// The GUI atlas: GUI sprites, particles and the item and block textures drawn flat. Look a
    /// sprite up with `gui_sprite`, which answers an [`AtlasRegion`].
    Gui,
}

/// A mod-generated PNG to stitch into the next client resource generation.
#[derive(Clone, Debug, PartialEq, Eq)]
// exhaustive: mods submit declarations by value
pub struct RuntimeSprite {
    /// The target client atlas.
    pub atlas: TextureAtlas,
    /// A namespaced texture id including the `block/` or `gui/` path prefix.
    pub id: Identifier,
    /// A static PNG. Images must be at most 256 pixels wide and high.
    pub png: Vec<u8>,
}

/// Where a sprite lies in the GUI atlas, as `gui_sprite` answers it: plain data, a snapshot of the
/// atlas built last. It stays right until the next `texture_atlas_stitched` for
/// [`TextureAtlas::Gui`], when a mod reads it again.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct AtlasRegion {
    /// `[u0, v0, u1, v1]`: the sprite's corners, from 0 to 1 across the atlas.
    pub uv: [f32; 4],
    /// The sprite's size in its own pixels: the default resources' texture's size whatever the
    /// resolution of a pack that replaced it (the units vanilla's GUI code draws it in), the
    /// texture's own size for one the default resources lack.
    pub size: [u32; 2],
}

impl AtlasRegion {
    /// A region, from the game's atlas.
    pub fn new(uv: [f32; 4], size: [u32; 2]) -> Self {
        Self { uv, size }
    }
}
