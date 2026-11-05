//! A wasm mod listening at a point whose library is refused over its own points is refused with
//! it, and keeps nothing of their group alive. CI builds `tests/groups/counter_lib` and
//! `tests/points/lib_tail` first and points `HORIZON_WASM_GROUPS`, `HORIZON_WASM_POINTS` and
//! `HORIZON_WASM_STD` at them; the test is ignored elsewhere.
use horizon_mod_wasm::{Refusal, WasmPackage};
use horizon_mods::manifest::Manifest;
use mod_api::__private::wasm_host::live_groups;

#[test]
#[ignore = "needs tests/groups/counter_lib and tests/points/lib_tail built to mod.wasm (CI's wasm-mods job)"]
fn a_listener_refused_with_its_library_keeps_nothing_of_their_group() {
    let path = |var: &str| std::path::PathBuf::from(std::env::var(var).expect(var));
    let read = |package: std::path::PathBuf| WasmPackage {
        manifest: horizon_mods::Package::read(&package).unwrap().manifest,
        module: horizon_mods::Package::read_wasm(&package).unwrap(),
    };
    let mut library =
        read(path("HORIZON_WASM_GROUPS").join("counter_lib/target/horizon-mod/counter_lib.hmod"));
    let tail = read(path("HORIZON_WASM_POINTS").join("lib_tail/target/horizon-mod/lib_tail.hmod"));
    assert!(!tail.manifest.wasm.as_ref().unwrap().points().is_empty());
    // The library lists a point this game doesn't have.
    let wasm = library.manifest.wasm.as_ref().unwrap();
    library.manifest = Manifest::parse(&format!(
        "id = \"counter_lib\"\nname = \"Counter library\"\nversion = \"1.0.0\"\napi = \"0.2\"\n\
         capabilities = [\"registries\"]\n\n[wasm]\nsha512 = \"{}\"\ntoolchain = \"{}\"\n\n\
         [[native.injection]]\nid = \"gone\"\ntarget = \"horizon_client::game::Game<'_>::gone\"\n\
         kind = \"HEAD\"\ntrait_method = \"gone\"\n",
        wasm.sha512(),
        wasm.toolchain()
    ))
    .unwrap();
    let before = live_groups();
    let loaded = horizon_mod_wasm::load(
        &horizon_mod_wasm::RuntimeConfig::default(),
        &std::fs::read(path("HORIZON_WASM_STD")).unwrap(),
        vec![library, tail],
    );
    let mut refused: Vec<_> = loaded
        .refused
        .iter()
        .map(|(id, why)| (id.as_str(), why))
        .collect();
    refused.sort_by_key(|(id, _)| *id);
    assert!(
        matches!(
            &refused[..],
            [
                ("counter_lib", Refusal::NoSuchPoint { .. }),
                ("lib_tail", Refusal::DependencyRefused { dependency }),
            ] if dependency == "counter_lib"
        ),
        "{refused:?}"
    );
    assert!(loaded.mods.is_empty() && loaded.listening.is_empty());
    // The linked group was instantiated, then let go with both its members.
    drop(loaded);
    assert_eq!(
        live_groups(),
        before,
        "the refused group's store is still alive"
    );
}
