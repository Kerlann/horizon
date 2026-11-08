// hidden: a module of game-side types; mods use their re-exports
#[doc(hidden)]
pub mod health;
pub use crate::health::DamageContainer;
pub use crate::health::{
    DamageValues, HurtSource, Reduction, ReductionChange, ShieldBlock, Victim,
};
