//! `horizon-stage ROOT POINTS_JSON INTERNALS_JSON`: mark points and expose internals in a staged
//! source copy, then write both manifests (#589). Never run on a working tree.
use std::path::PathBuf;

fn main() {
    let args: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let [root, list, internals] = &args[..] else {
        eprintln!("usage: horizon-stage ROOT POINTS_JSON INTERNALS_JSON");
        std::process::exit(2);
    };
    // Internals first, so the manifest lists the game's own items and not the points' raw types.
    let staged = horizon_mod_api_gen::internals::widen(root).and_then(|widened| {
        let text = serde_json::to_string_pretty(&widened.entries).map_err(|e| e.to_string())?;
        std::fs::write(internals, text + "\n")
            .map_err(|e| format!("{}: {e}", internals.display()))?;
        let staged = horizon_mod_api_gen::points::stage(root)?;
        let points = horizon_mod_api_gen::points::game_points(root)?;
        let text = serde_json::to_string_pretty(&points).map_err(|e| e.to_string())?;
        std::fs::write(list, text + "\n").map_err(|e| format!("{}: {e}", list.display()))?;
        Ok((staged, widened))
    });
    match staged {
        Ok((staged, widened)) => {
            println!(
                "staged {} points ({} with a head only, {} with a raw form) in {} files",
                staged.points, staged.head_only, staged.raw, staged.files
            );
            let shapes = widened
                .entries
                .iter()
                .filter(|entry| entry.shape_only)
                .count();
            println!(
                "exposed {} internals and listed {} type shapes in {} changed files: {:?}",
                widened.entries.len() - shapes,
                shapes,
                widened.files,
                widened.changed
            );
        }
        Err(error) => {
            eprintln!("horizon-stage: {error}");
            std::process::exit(1);
        }
    }
}
