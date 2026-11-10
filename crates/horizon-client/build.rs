//! Generates the native mod API, and gives the Windows executable Horizon's icon and its stack
//! (`packaging/windows/exe.rs`, shared with the official executable's build script).
#[path = "../../packaging/windows/exe.rs"]
mod windows_exe;

fn main() {
    let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    let root = std::path::Path::new(&dir).join("../..");
    let out_dir = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    horizon_mod_api_gen::generate(&root, &out_dir, horizon_mod_api_gen::Target::Client)
        .expect("generating native mod API");
    windows_exe::resources("horizon-client");
}
