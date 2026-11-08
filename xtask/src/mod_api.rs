//! `mod-api`: fill new native hooks' placeholder bodies in the hand-written source, and write
//! the layouts of the foreign types that cross to wasm mods from the locked source.
use std::path::{Path, PathBuf};

fn checkout() -> Result<PathBuf, String> {
    let root = std::env::current_dir().map_err(|e| e.to_string())?;
    root.ancestors()
        .find(|path| path.join("crates/horizon-mod-api/src/hooks").is_dir())
        .map(Path::to_path_buf)
        .ok_or("run xtask from within the Horizon checkout".into())
}

pub fn run() -> Result<(), String> {
    let root = checkout()?;
    horizon_mod_api_gen::fill_hooks(&root)?;
    horizon_mod_api_gen::crossing::write_foreign_layouts(&root)
}

/// Prints the hook-notes chapter for the book preprocessor, without writing a source file.
pub fn notes() -> Result<(), String> {
    print!(
        "{}",
        horizon_mod_api_gen::generate_hook_notes(&checkout()?)?
    );
    Ok(())
}
