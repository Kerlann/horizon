// hidden: a module of game-side types; mods use their re-exports
#[doc(hidden)]
pub mod containers;
pub use crate::containers::{ModBlockEntity, ModContainer, ModEnergyBuffer, ModFluidTanks};
mod __private {
    mod game {
        pub use self::modblockentityviewnew::ModBlockEntityViewNew;
    }
}
