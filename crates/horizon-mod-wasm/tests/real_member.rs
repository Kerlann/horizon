//! The build-tool fixture is a real shared module, not a component.
// Generated fixtures exercise every shape; TODO(#2273): make the generator emit clippy-clean code.
#![allow(clippy::all, unused_comparisons, unreachable_patterns, dead_code)]
use horizon_mod_package::manifest::Package;
use horizon_mod_wasm::{WasmPackage, load};

struct Quiet;
impl mod_api::Host for Quiet {}
mod_api::impl_host_areas!(Quiet);
impl mod_api::ServerHost for Quiet {}
mod_api::impl_server_host_areas!(Quiet);

#[test]
fn dump_members() {
    for (name, path) in [
        ("std", "HORIZON_WASM_STD"),
        ("fixture", "HORIZON_WASM_FIXTURE_RAW"),
    ] {
        let Ok(path) = std::env::var(path) else {
            return;
        };
        let bytes = std::fs::read(path).unwrap();
        println!("=== {name} ===");
        for payload in wasmparser::Parser::new(0).parse_all(&bytes) {
            match payload.unwrap() {
                wasmparser::Payload::CustomSection(s) if s.name() == "dylink.0" => {
                    println!("dylink.0: {:02x?}", s.data())
                }
                wasmparser::Payload::ImportSection(s) => {
                    for i in s.into_imports() {
                        println!("import: {:?}", i.unwrap());
                    }
                }
                wasmparser::Payload::ExportSection(s) => {
                    for e in s {
                        println!("export: {:?}", e.unwrap());
                    }
                }
                _ => {}
            }
        }
    }
}

#[test]
fn real_member() {
    let (Ok(fixture), Ok(std)) = (
        std::env::var("HORIZON_WASM_FIXTURE"),
        std::env::var("HORIZON_WASM_STD"),
    ) else {
        eprintln!("skipping: HORIZON_WASM_FIXTURE and HORIZON_WASM_STD are not set");
        return;
    };
    let path = std::path::Path::new(&fixture);
    let manifest = Package::read(path).unwrap().manifest;
    let info = mod_api::PackageInfo::new(
        manifest.id.clone(),
        manifest.version.clone(),
        mod_api::PackageKind::Code,
        mod_api::Side::Both,
        vec![manifest.id.clone()],
        manifest.dependencies.keys().cloned().collect(),
    );
    let package = WasmPackage {
        manifest,
        module: Package::read_wasm(path).unwrap(),
    };
    // load checks the layout and calls Hooks::subscribed through the real host boundary.
    let loaded = load(&std::fs::read(std).unwrap(), vec![package]);
    assert!(loaded.refused.is_empty(), "{:?}", loaded.refused);
    assert_eq!(loaded.mods.len(), 1);
    loaded
        .initialize(&[info])
        .expect("guest initialization and point registration");
    mod_api::testing::with_mods(loaded.mods.into(), || {
        mod_api::__private::install_parts(
            mod_api::__private::mods(),
            &mod_api::__private::part_tests::rule_schema(),
            |_| false,
        )
        .unwrap();
        assert!(
            mod_api::__private::block_entity_types()
                .iter()
                .any(|kind| kind.id == "wasm_fixture:counter")
        );
        mod_api::__private::mark_game_thread();
        let mut entity =
            mod_api::ModBlockEntity::new("wasm_fixture:counter".into(), Default::default());
        mod_api::dispatch_block_entity_tick(&mut Quiet, [0, 64, 0], &mut entity);
        assert_eq!(entity.data, [23]);
        assert_eq!(
            entity.nbt.get(&"counter".into()),
            Some(&mod_api::Tag::Int(17))
        );
    });
    mod_points::__private::seal();
    let mut args = mod_points::TickArgs::__new();
    mod_points::dispatch::tick(mod_api::HostSide::Server(&mut Quiet), &mut args);
    assert!(
        args.cancel,
        "real wasm listener must modify the point record"
    );
}
