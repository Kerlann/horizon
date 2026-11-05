//! Plain-data structure-template snapshots and transforms.
use crate::{BlockStateId, Compound};

/// Rotation of a template about its origin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum StructureRotation {
    /// No rotation.
    None,
    /// A clockwise quarter turn.
    Clockwise90,
    /// A half turn.
    Clockwise180,
    /// A counterclockwise quarter turn.
    Counterclockwise90,
}

/// Mirror applied before rotating a template.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum StructureMirror {
    /// No mirror.
    None,
    /// Mirror north and south.
    LeftRight,
    /// Mirror east and west.
    FrontBack,
}

/// The transformed template size and blocks in vanilla placement order.
#[derive(Clone, Debug, PartialEq, Default)]
#[non_exhaustive]
pub struct StructureTemplateSnapshot {
    /// Size after rotation.
    pub size: [i32; 3],
    /// Blocks in vanilla placement order.
    pub blocks: Vec<StructureTemplateBlock>,
}

/// A transformed block and its optional block-entity data.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct StructureTemplateBlock {
    /// Position in the level.
    pub pos: [i32; 3],
    /// State after mirroring and rotation.
    pub state: BlockStateId,
    /// Saved block-entity data, without its position.
    pub nbt: Option<Compound>,
}

impl StructureTemplateSnapshot {
    /// Build a snapshot from its size and blocks.
    pub fn new(size: [i32; 3], blocks: Vec<StructureTemplateBlock>) -> Self {
        Self { size, blocks }
    }
}

impl StructureTemplateBlock {
    /// Build a block snapshot.
    pub fn new(pos: [i32; 3], state: BlockStateId, nbt: Option<Compound>) -> Self {
        Self { pos, state, nbt }
    }
}
