//! Installed mod code loading above the engine (docs/layers.md, #2534).
//! The platform prepares the engine, loads mods here, then makes one engine start call.
#![forbid(unsafe_code)]

mod loading;
pub use loading::main_with;

/// A native function slot, checked against its place's exact type before erasure.
pub use mod_points::__private::SlotListener as NativeSlot;

/// The places raw listeners register at, named by the generated `main`.
#[doc(hidden)]
pub use mod_points::PointId;

