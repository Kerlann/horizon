mod goals;
pub use goals::{Goal, GoalContext, GoalFlags, GoalRegistrar, RegistrarMob};
// hidden: the game's goal plumbing, not part of the mod surface
#[doc(hidden)]
pub use goals::{GoalCxBackend, GoalRegistrarBackend};
