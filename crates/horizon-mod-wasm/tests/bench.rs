//! Real shared member versus the same source natively. Run in release with the fixture and std
//! built first; this measures 1,000 dispatches into fake, in-memory block entities, not a world tick.
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
mod_api::impl_host_areas!(World);
impl ServerHost for World {}
mod_api::impl_server_host_areas!(World);

fn measure(label: &str, mods: Vec<ModEntry>) {
    mod_api::testing::with_mods(mods.into(), || {
        mod_api::__private::install_parts(
            mod_api::__private::mods(),
            &mod_api::__private::part_tests::rule_schema(),
        )
        .unwrap();
        assert!(
            mod_api::__private::block_entity_types()
                .iter()
                .any(|kind| kind.id == "wasm_fixture:counter"),
            "{label}: register did not declare wasm_fixture:counter"
        );
        mod_api::__private::mark_game_thread();
        let mut entities: Vec<_> = (0..1000)
            .map(|_| ModBlockEntity::new("wasm_fixture:counter".into(), Default::default()))
            .collect();
        mod_api::dispatch_block_entity_tick(&mut World, [0, 64, 0], &mut entities[0]);
        assert_eq!(
            entities[0].data,
            [23],
            "{label}: registered tick did not write entity data"
        );
        assert_eq!(
            entities[0].nbt.get(&"counter".into()),
            Some(&mod_api::Tag::Int(17)),
            "{label}: registered tick did not write entity NBT"
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
            "{label} phase ns per call: {:?}",
            times.map(|n| n as f64 / 100000.0)
        );
        println!(
            "{label} allocations per call: {:?}",
            allocations.map(|n| n as f64 / 100000.0)
        );
        println!(
            "{label} allocated bytes per call: {:?}",
            bytes.map(|n| n as f64 / 100000.0)
        );
        assert!(entities.iter().all(|entity| entity.data == [23]
            && entity.nbt.get(&"counter".into()) == Some(&mod_api::Tag::Int(17))));
        samples.sort_unstable();
        let mean = samples.iter().sum::<u128>() / samples.len() as u128;
        println!(
            "{label}: 1000 entities mean={mean} ns p95={} ns",
            samples[94]
        );
    });
}

#[test]
#[ignore = "measurement; requires a built fixture and shared std"]
fn block_entity_ticks() {
    struct TestLog;
    impl log::Log for TestLog {
        fn enabled(&self, _: &log::Metadata<'_>) -> bool {
            true
        }
        fn log(&self, record: &log::Record<'_>) {
            eprintln!("{}: {}", record.level(), record.args());
        }
        fn flush(&self) {}
    }
    log::set_logger(&TestLog).unwrap();
    log::set_max_level(log::LevelFilter::Warn);
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
