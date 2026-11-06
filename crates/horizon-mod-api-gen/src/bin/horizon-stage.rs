//! `horizon-stage ROOT POINTS_JSON INTERNALS_JSON`: mark points and expose internals in a staged
//! source copy, then write both manifests (#589). Never run on a working tree.
use std::path::PathBuf;

fn main() {
    let args: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let discovery = args.last().is_some_and(|arg| arg == "--sync-internals");
    let positional = if discovery {
        &args[..args.len() - 1]
    } else {
        &args[..]
    };
    let [root, list, internals] = positional else {
        eprintln!("usage: horizon-stage ROOT POINTS_JSON INTERNALS_JSON [--sync-internals]");
        std::process::exit(2);
    };
    let mode = if discovery {
        horizon_mod_api_gen::staging::Mode::Discovery
    } else {
        horizon_mod_api_gen::staging::Mode::Build
    };
    match horizon_mod_api_gen::staging::stage(root, list, internals, mode) {
        Ok(horizon_mod_api_gen::staging::Staged {
            points: staged,
            internals: widened,
        }) => {
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
