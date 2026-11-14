//! `side = "server"`: what a mod may not register, and the data-only mod that loads alone.
use crate::content::{Content, server_side_refusal};
use crate::manifest::Manifest;
use std::path::PathBuf;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "horizon-content-side-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn read(label: &str, side: &str, files: &[(&str, &[u8])]) -> (Manifest, Content) {
    let dir = scratch(label);
    for (name, bytes) in files {
        let path = dir.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, bytes).unwrap();
    }
    let manifest = Manifest::parse(&format!(
        "id = 'ruby'\nname = 'Ruby'\nversion = '0.1.0'\napi = '0.2'\nside = '{side}'\ncapabilities = ['registries']\n"
    ))
    .unwrap();
    let content = Content::read(&dir, &manifest).unwrap();
    std::fs::remove_dir_all(dir).unwrap();
    (manifest, content)
}

const BLOCK: &[u8] = br#"{"template": "minecraft:stone"}"#;

#[test]
fn a_server_mod_is_refused_by_name_for_each_kind_of_client_content() {
    let (manifest, registry) = read(
        "registry",
        "server",
        &[("data/ruby/horizon/block/a.json", BLOCK)],
    );
    let refusal = server_side_refusal(&manifest, &registry, false).unwrap();
    assert!(refusal.contains("ruby"), "{refusal}");
    assert!(refusal.contains("registry content"), "{refusal}");

    let (manifest, assets) = read(
        "assets",
        "server",
        &[(
            "assets/ruby/textures/block/a.png",
            b"not read, only counted",
        )],
    );
    let refusal = server_side_refusal(&manifest, &assets, false).unwrap();
    assert!(refusal.contains("ruby"), "{refusal}");
    assert!(refusal.contains("assets"), "{refusal}");

    let (manifest, data_only) = read("hooks", "server", &[("data/ruby/tags/block/a.json", b"{}")]);
    let refusal = server_side_refusal(&manifest, &data_only, true).unwrap();
    assert!(refusal.contains("ruby"), "{refusal}");
    assert!(refusal.contains("client hooks"), "{refusal}");
}

#[test]
fn a_server_only_data_mod_loads_alone() {
    let (manifest, content) = read(
        "data-only",
        "server",
        &[("data/ruby/tags/block/a.json", b"{}")],
    );
    assert_eq!(server_side_refusal(&manifest, &content, false), None);
    assert_eq!(content.client_requirement(), None);
}

#[test]
fn a_client_or_both_mod_may_register_what_a_client_needs() {
    for side in ["client", "both"] {
        let (manifest, content) = read(
            side,
            side,
            &[("assets/ruby/textures/block/a.png", b"a client draws it")],
        );
        assert_eq!(
            server_side_refusal(&manifest, &content, true),
            None,
            "{side}"
        );
    }
}
