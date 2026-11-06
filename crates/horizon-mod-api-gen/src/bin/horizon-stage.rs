//! `horizon-stage ROOT POINTS_JSON INTERNALS_JSON`: mark points and expose internals in a staged
//! source copy, then write both manifests (#589). Never run on a working tree.
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

/// One file per mod; sorting and deduplication make the union independent of directory order.
fn declarations(directory: &Path) -> Result<Vec<String>, String> {
    let files = match std::fs::read_dir(directory) {
        Ok(files) => files,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("{}: {error}", directory.display())),
    };
    let mut paths = BTreeSet::new();
    for file in files {
        let file = file
            .map_err(|e| format!("{}: {e}", directory.display()))?
            .path();
        if file.extension().is_none_or(|ext| ext != "txt") {
            continue;
        }
        let text =
            std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        for line in text.lines() {
            let path = line.split('#').next().unwrap_or_default().trim();
            if !path.is_empty() {
                paths.insert(path.to_owned());
            }
        }
    }
    Ok(paths.into_iter().collect())
}

/// Keep unsafe forbidden everywhere except the generated table, in the staged copy only.
fn with_internals_table(source: &str) -> String {
    source.replace("#![forbid(unsafe_code)]", "#![deny(unsafe_code)]")
}

/// Daily prebuilt builds use the checkout's declarations. A launcher build supplies only the
/// installed mods' directory, or `None` to leave the committed empty table untouched.
fn stage_table(
    root: &Path,
    entries: &[horizon_mod_api_gen::internals::Internal],
    points: &[horizon_mod_api_gen::points::GamePoint],
    directory: Option<&Path>,
) -> Result<(), String> {
    let Some(directory) = directory else {
        return Ok(());
    };
    let declared = declarations(directory)?;
    let (table, counts) =
        horizon_mod_api_gen::internals_table::table_with_points(entries, &declared, points);
    let lib = root.join("crates/horizon-client/src/lib.rs");
    let text = std::fs::read_to_string(&lib).map_err(|e| format!("{}: {e}", lib.display()))?;
    std::fs::write(&lib, with_internals_table(&text))
        .map_err(|e| format!("{}: {e}", lib.display()))?;
    let module = root.join("crates/horizon-client/src/__internals.rs");
    std::fs::write(&module, table).map_err(|e| format!("{}: {e}", module.display()))?;
    println!(
        "internals table: {} types, {} thunks from {} declared paths ({} skipped)",
        counts.types, counts.thunks, counts.declared, counts.skipped
    );
    for (path, reason) in &counts.refused {
        println!("internals table: skipped {path}: {reason}");
    }
    Ok(())
}

fn main() {
    let args: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let usage = || {
        eprintln!("usage: horizon-stage ROOT POINTS_JSON INTERNALS_JSON [--wasm-internals DIRECTORY | --no-wasm-internals | --sync-internals]");
        std::process::exit(2)
    };
    let [root, list, internals, options @ ..] = &args[..] else { usage() };
    let (directory, mode) = match options {
        [] => (Some(root.join("crates/horizon-mod-api-gen/wasm-internals")), horizon_mod_api_gen::staging::Mode::Build),
        [flag] if flag == "--no-wasm-internals" => (None, horizon_mod_api_gen::staging::Mode::Build),
        [flag] if flag == "--sync-internals" => (None, horizon_mod_api_gen::staging::Mode::Discovery),
        [flag, directory] if flag == "--wasm-internals" => (Some(directory.clone()), horizon_mod_api_gen::staging::Mode::Build),
        _ => usage(),
    };
    let staged = horizon_mod_api_gen::staging::stage(root, list, internals, mode).and_then(|staged| {
        let points = horizon_mod_api_gen::points::game_points(root)?;
        stage_table(root, &staged.internals.entries, &points, directory.as_deref())?;
        Ok(staged)
    });
    match staged {
        Ok(horizon_mod_api_gen::staging::Staged { points: staged, internals: widened }) => {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declaration_files_form_a_deterministic_union() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target")
            .join(format!("wasm-declarations-{}", std::process::id()));
        let directory = root.join("crates/horizon-mod-api-gen/wasm-internals");
        assert!(declarations(&directory).unwrap().is_empty());
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join("first.txt"),
            "# whole comment\nz\na # inline\n\n",
        )
        .unwrap();
        std::fs::write(directory.join("second.txt"), "a\nb\n").unwrap();
        std::fs::write(directory.join("README.md"), "not a path").unwrap();
        assert_eq!(declarations(&directory).unwrap(), ["a", "b", "z"]);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_from_source_build_keeps_the_stub_unless_a_directory_is_supplied() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target")
            .join(format!("wasm-table-gate-{}", std::process::id()));
        let game = root.join("crates/horizon-client/src");
        std::fs::create_dir_all(&game).unwrap();
        let stub = "pub static TABLE: I::Table = I::Table::EMPTY;\n";
        std::fs::write(game.join("__internals.rs"), stub).unwrap();
        std::fs::write(game.join("lib.rs"), "#![forbid(unsafe_code)]\n").unwrap();
        stage_table(&root, &[], &[], None).unwrap();
        assert_eq!(
            std::fs::read_to_string(game.join("__internals.rs")).unwrap(),
            stub
        );
        assert_eq!(
            std::fs::read_to_string(game.join("lib.rs")).unwrap(),
            "#![forbid(unsafe_code)]\n"
        );
        let directory = root.join("installed");
        std::fs::create_dir_all(&directory).unwrap();
        let path = "horizon_core::random::hash::seed_from_hash";
        std::fs::write(directory.join("example.txt"), path).unwrap();
        stage_table(&root, &[], &[], Some(&directory)).unwrap();
        let generated = std::fs::read_to_string(game.join("__internals.rs")).unwrap();
        assert_ne!(generated, stub);
        assert!(generated.contains("native_only"));
        assert!(generated.contains(path));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_staged_root_allows_only_the_table_to_override_unsafe() {
        let source = "#![forbid(unsafe_code)]\npub mod game;";
        let staged = with_internals_table(source);
        assert!(staged.starts_with("#![deny(unsafe_code)]\npub mod game;"));
        assert!(!staged.contains("forbid(unsafe_code)"));
        assert!(!staged.contains("pub mod __internals"));
        assert!(source.contains("forbid(unsafe_code)"));
    }
}
