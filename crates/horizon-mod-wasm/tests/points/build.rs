use horizon_mod_api_gen::points::{GuestInject, guest_exports};
use horizon_mod_package::manifest::{
    Manifest,
    inject::{Access, Kind},
};
use std::{env, fs, path::PathBuf};

fn main() {
    let dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let manifest = dir.join("horizon-mod.toml");
    println!("cargo:rerun-if-changed={}", manifest.display());
    let manifest = Manifest::parse(&fs::read_to_string(manifest).unwrap()).unwrap();
    let entries = manifest
        .inject
        .iter()
        .map(|entry| GuestInject {
            id: &entry.id,
            at: &entry.at,
            with: &entry.with,
            at_return: match (entry.access, entry.kind) {
                (Access::Api, Kind::Head) => false,
                (Access::Api, Kind::Return) => true,
                _ => panic!(
                    "{}: only API head/return guest injects are supported",
                    entry.id
                ),
            },
        })
        .collect::<Vec<_>>();
    let root = dir
        .ancestors()
        .find(|root| root.join("crates/horizon-mod-api-gen").is_dir())
        .expect("fixture in a Horizon checkout");
    for path in horizon_mod_api_gen::points::tracked(root).unwrap() {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    let code = guest_exports(root, &entries).unwrap();
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("guest_injects.rs"),
        code,
    )
    .unwrap();
}
