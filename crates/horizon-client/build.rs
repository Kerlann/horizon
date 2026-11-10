//! Generates the native mod API, and gives the Windows executable Horizon's icon and its stack
//! (`packaging/windows/exe.rs`, shared with the official executable's build script).
#[path = "../../packaging/windows/exe.rs"]
mod windows_exe;

fn main() {
    let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    let root = std::path::Path::new(&dir).join("../..");
    let out_dir = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    println!(
        "cargo:rerun-if-changed={}",
        root.join("crates/horizon-client/src/game").display()
    );
    std::fs::write(
        out_dir.join("game_modules.rs"),
        horizon_game_discovery::declarations(
            &root.join("crates/horizon-client/src/game"),
            &["native_hooks", "host"],
        )
        .expect("discovering game modules"),
    )
    .expect("writing game modules");
    horizon_mod_api_gen::generate(&root, &out_dir, horizon_mod_api_gen::Target::Client)
        .expect("generating native mod API");
    if std::env::var("CARGO_PKG_NAME").as_deref() != Ok("horizon-client") {
        windows_exe::resources("horizon-client");
    }
}
