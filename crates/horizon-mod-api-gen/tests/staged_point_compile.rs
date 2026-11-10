//! Compile a source-staged point with the dependencies staging installs, not a handwritten attribute.
use std::{fs, path::Path};

#[test]
fn staged_point_compiles_with_explicit_platform_dependencies() {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let root = repository
        .join("target")
        .join(format!("staged-point-{}", std::process::id()));
    let put = |path: &str, source: &str| {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
    };
    put("crates/horizon-mod-api-gen/internals.txt", "");
    fs::create_dir_all(root.join("crates/horizon-mod-api-gen/slots")).unwrap();
    put(
        "crates/horizon-mod-points/Cargo.toml",
        "[package]\nname = 'horizon-mod-points'\n",
    );
    put(
        "crates/horizon-client/Cargo.toml",
        "[package]\nname = \"horizon-client\"\nversion = \"0.1.0\"\nedition = '2024'\n[workspace]\n",
    );
    put("crates/horizon-client/src/lib.rs", "mod game;\n");
    put(
        "crates/horizon-client/src/game.rs",
        "pub fn tick(value: &mut u32) { *value += 1; }\n",
    );
    put(
        "Cargo.lock",
        "version = 4\n[[package]]\nname = \"horizon-client\"\nversion = \"0.1.0\"\n",
    );
    let provider = root.join("crates/horizon-mod-points");
    fs::create_dir_all(provider.join("src")).unwrap();
    // Generate the fixture's matching records and IDs, using the production registry but
    // avoiding a second compile of the entire game's multi-megabyte point catalog.
    horizon_mod_api_gen::generate_points(&root, &provider.join("src")).unwrap();
    horizon_mod_api_gen::points::stage(&root).unwrap();
    let point_sources = repository.join("crates/horizon-mod-points/src");
    let source = fs::read_to_string(point_sources.join("lib.rs"))
        .unwrap()
        .replace(
            "include!(concat!(env!(\"OUT_DIR\"), \"/points_generated.rs\"));",
            "include!(\"points_generated.rs\");",
        );
    fs::write(provider.join("src/lib.rs"), source).unwrap();
    fs::copy(
        point_sources.join("registry.rs"),
        provider.join("src/registry.rs"),
    )
    .unwrap();
    fs::write(provider.join("Cargo.toml"), format!(
        "[package]\nname = 'horizon-mod-points'\nversion = '0.1.0'\nedition = '2024'\n[lib]\nname = 'mod_points'\n[features]\nwasm-guest = []\n[dependencies]\nhorizon-engine-slots = {{ path = {:?} }}\nhorizon-mod-api = {{ path = {:?} }}\n",
        repository.join("crates/horizon-engine-slots").canonicalize().unwrap(),
        repository.join("crates/horizon-mod-api").canonicalize().unwrap())).unwrap();
    let package = root.join("crates/horizon-client");
    let manifest = package.join("Cargo.toml");
    let mut cargo: toml::Table = toml::from_str(&fs::read_to_string(&manifest).unwrap()).unwrap();
    // Relocate only the generated relative paths to the real crates. The fixture has no
    // handwritten dependency list: omitting any staged dependency makes the compile fail.
    for (name, dependency) in cargo["dependencies"].as_table_mut().unwrap() {
        let path = if name == "horizon-mod-points" {
            provider.clone()
        } else {
            repository.join("crates").join(name)
        };
        dependency["path"] = path
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .into_owned()
            .into();
    }
    fs::write(&manifest, toml::to_string(&cargo).unwrap()).unwrap();
    let result = std::process::Command::new(env!("CARGO"))
        .args(["check", "--offline", "--manifest-path"])
        .arg(&manifest)
        // A nested Cargo must not wait on the outer test process's build lock.
        .arg("--target-dir")
        .arg(repository.join("target/staged-point-compile"))
        .env("CARGO_INCREMENTAL", "0")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    fs::remove_dir_all(root).unwrap();
}
