//! Temporary phase/allocation profiling of the two shared wasm benchmark workloads.
use horizon_mod_package::manifest::Package;
use horizon_mod_wasm::{WasmPackage, load};
use mod_api::{Host, ModBlockEntity, ModEntry, ServerHost};
use std::time::Instant;
#[global_allocator]
static ALLOCATOR: horizon_mod_wasm::profile::Allocator = horizon_mod_wasm::profile::Allocator;
#[path = "fixture/src/lib.rs"]
mod fixture;
struct World;
impl Host for World {}
mod_api::impl_host_areas!(World, except(levels));
impl mod_api::prelude::LevelsHost for World {
    fn is_loaded(&self, id: &str) -> bool {
        id == "wasm_fixture"
    }
}
impl ServerHost for World {}
mod_api::impl_server_host_areas!(World);
fn measure(label: &str, mods: Vec<ModEntry>) {
    mod_api::testing::with_mods(mods.into(), || {
        mod_api::__private::install_parts(
            mod_api::__private::mods(),
            &mod_api::__private::part_tests::rule_schema(),
        )
        .unwrap();
        mod_api::__private::mark_game_thread();
        let _phase = mod_api::__private::PhaseOwner::scope(mod_api::__private::ModPhase::World);
        for (kind, expected) in [("counter", 23), ("counter_reads", 25)] {
            assert!(
                mod_api::__private::block_entity_types()
                    .iter()
                    .any(|decl| decl.id == format!("wasm_fixture:{kind}"))
            );
            let mut entities: Vec<_> = (0..1000)
                .map(|_| ModBlockEntity::new(format!("wasm_fixture:{kind}"), Default::default()))
                .collect();
            mod_api::dispatch_block_entity_tick(&mut World, [0, 64, 0], &mut entities[0]);
            assert_eq!(entities[0].data, [expected]);
            assert_eq!(
                entities[0].nbt.get(&"counter".into()),
                Some(&mod_api::Tag::Int(17))
            );
            let mut samples = Vec::with_capacity(100);
            for round in 0..120 {
                if round == 20 {
                    horizon_mod_wasm::profile::start();
                }
                let now = Instant::now();
                for entity in &mut entities {
                    mod_api::dispatch_block_entity_tick(&mut World, [0, 64, 0], entity);
                }
                let elapsed = now.elapsed().as_nanos();
                if round >= 20 {
                    samples.push(elapsed);
                }
            }
            let (times, allocations, bytes) = horizon_mod_wasm::profile::finish();
            println!(
                "{label} {kind} phase ns per call: {:?}",
                times.map(|n| n as f64 / 100000.0)
            );
            println!(
                "{label} {kind} allocations per call: {:?}",
                allocations.map(|n| n as f64 / 100000.0)
            );
            println!(
                "{label} {kind} allocated bytes per call: {:?}",
                bytes.map(|n| n as f64 / 100000.0)
            );
            assert!(entities.iter().all(|entity| entity.data == [expected]
                && entity.nbt.get(&"counter".into()) == Some(&mod_api::Tag::Int(17))));
            samples.sort_unstable();
            let mean = samples.iter().sum::<u128>() / samples.len() as u128;
            println!(
                "{label} {kind}: 1000 entities mean={mean} ns p95={} ns",
                samples[94]
            );
        }
    });
}
#[test]
#[ignore = "measurement; requires a built fixture and shared std"]
fn block_entity_ticks() {
    let path = std::env::var("HORIZON_WASM_FIXTURE").unwrap();
    let path = std::path::Path::new(&path);
    let std = std::fs::read(std::env::var("HORIZON_WASM_STD").unwrap()).unwrap();
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
    let loaded = load(&std, vec![package]);
    assert!(loaded.refused.is_empty(), "{:?}", loaded.refused);
    assert_eq!(loaded.mods.len(), 1);
    loaded
        .initialize(&[info])
        .expect("wasm initialization failed");
    measure(
        "native",
        vec![ModEntry::new("wasm_fixture", Box::new(fixture::Mod))],
    );
    measure("wasm", loaded.mods);
    horizon_mod_wasm::profile::start();
    for i in 0..1000000 {
        horizon_mod_wasm::profile::mark(i % 16);
    }
    let (times, _, _) = horizon_mod_wasm::profile::finish();
    println!(
        "clock calibration: {} ns per transition",
        times.iter().sum::<u64>() as f64 / 1000000.0
    );
}
