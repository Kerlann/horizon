//! Engine-owned injection activity.
use std::sync::atomic::AtomicBool;

/// Each place's slot: set once, at the seal, where a mod listens. A point that nothing listens at
/// costs one load of it.
pub static POINTS: [AtomicBool; POINT_COUNT] = [const { AtomicBool::new(false) }; POINT_COUNT];
/// Call-expression slots, in the same table as the points.
pub static SITES: [AtomicBool; SITE_COUNT] = [const { AtomicBool::new(false) }; SITE_COUNT];
