//! One source staging pass for the launcher and `horizon-stage`.
use crate::{internals, points};
use std::path::Path;

/// Discovery instrumentation must never be used to build or publish a game.
#[derive(Clone, Copy)]
pub enum Mode {
    /// Widen visibility and generate injection points, without diagnostic attributes.
    Build,
    /// Add internal-use diagnostics for an isolated metadata-only mod check.
    Discovery,
}

/// Counts and catalog produced by source staging.
pub struct Staged {
    /// Injection point counts.
    pub points: points::Staged,
    /// Exposed internal signatures and changed visibility counts.
    pub internals: internals::Widened,
}

/// Mark points, expose internals and write their manifests in a disposable source copy.
/// Internals precede points so their catalog contains only the original game items.
pub fn stage(
    root: &Path,
    points_json: &Path,
    internals_json: &Path,
    mode: Mode,
) -> Result<Staged, String> {
    let internals = match mode {
        Mode::Build => internals::widen(root)?,
        Mode::Discovery => internals::discover(root)?,
    };
    write(internals_json, &internals.entries)?;
    let points = points::stage(root)?;
    write(points_json, &points::game_points(root)?)?;
    Ok(Staged { points, internals })
}

fn write(path: &Path, value: &impl serde::Serialize) -> Result<(), String> {
    let text = serde_json::to_string_pretty(value).map_err(|error| error.to_string())?;
    std::fs::write(path, text + "\n").map_err(|error| format!("{}: {error}", path.display()))
}
