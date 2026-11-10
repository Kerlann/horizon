//! The game's slot table, in the one registry every point table shares
//! (`mod_api::__private::points`).
use crate::{POINT_COUNT, PointId, SITE_COUNT, SiteId};
use mod_api::__private::points::{Place, Table};

#[cfg(not(target_arch = "wasm32"))]
pub use mod_api::__private::points::register;
pub use mod_api::__private::points::{RegisterError, seal, stop};

/// The game's table: its points, then its call sites.
pub static TABLE: Table = Table::engine(POINT_COUNT, SITE_COUNT);

impl PointId {
    /// This place in the game's table, for a listener's constructor.
    pub const fn place(self) -> Place {
        Place::new(&TABLE, self as usize)
    }
}

impl SiteId {
    /// This call site in the game's table, after its points.
    pub const fn place(self) -> Place {
        Place::new(&TABLE, POINT_COUNT + self as usize)
    }
}
