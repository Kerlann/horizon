#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dimension {
    Overworld,
    Nether,
    End,
}

use crate::LevelKey;
impl Dimension {
    /// The level key of this dimension.
    pub fn key(self) -> LevelKey {
        match self {
            Self::Overworld => LevelKey::OVERWORLD,
            Self::Nether => LevelKey::NETHER,
            Self::End => LevelKey::END,
        }
    }

    /// The dimension of a level key; custom levels map to the Overworld.
    pub fn of_level(key: LevelKey) -> Self {
        match key {
            LevelKey::NETHER => Self::Nether,
            LevelKey::END => Self::End,
            _ => Self::Overworld,
        }
    }
}
