//! Temporary direct-export baseline: no guest or production transport shortcuts.
use super::super::*;
use horizon_mod_package::manifest::Package;
use mod_api::{Host, ServerHost};

struct World;
impl Host for World {}
mod_api::impl_host_areas!(World);
impl ServerHost for World {}
mod_api::impl_server_host_areas!(World);

#[test]
#[ignore = "direct export release measurement; needs matching real fixture and noop export"]
fn direct_exports() {
    let path = std::env::var("HORIZON_WASM_FIXTURE").unwrap();
    let path = std::path::Path::new(&path);
    let manifest = Package::read(path).unwrap().manifest;
    let info = mod_api::PackageInfo::new(
        manifest.id.clone(),
        manifest.version.clone(),
        mod_api::PackageKind::Code,
        mod_api::Side::Both,
        vec![manifest.id.clone()],
        manifest.dependencies.keys().cloned().collect(),
    );
    let std = std::fs::read(std::env::var("HORIZON_WASM_STD").unwrap()).unwrap();
    let mut loaded = crate::load(
        &std,
        vec![crate::WasmPackage {
            manifest,
            module: Package::read_wasm(path).unwrap(),
        }],
    );
    assert!(loaded.refused.is_empty(), "{:?}", loaded.refused);
    loaded.initialize(&[info]).unwrap();
    let module = loaded.modules[0].clone();
    assert!(
        module
            .subscribed
            .iter()
            .flatten()
            .any(|hook| *hook == mod_api::HookId::ServerStopped),
        "fixture must register the normal empty-body server_stopped hook"
    );
    // Capture the actual registered handler and encoded arguments via the normal API.
    mod_api::testing::with_mods(std::mem::take(&mut loaded.mods).into(), || {
        mod_api::__private::install_parts(
            mod_api::__private::mods(),
            &mod_api::__private::part_tests::rule_schema(),
        )
        .unwrap();
        assert!(
            mod_api::__private::block_entity_types()
                .iter()
                .any(|kind| kind.id == "wasm_fixture:counter")
        );
        mod_api::__private::mark_game_thread();
        let _phase = mod_api::__private::PhaseOwner::scope(mod_api::__private::ModPhase::World);
        let mut entity =
            mod_api::ModBlockEntity::new("wasm_fixture:counter".into(), Default::default());
        // Twice: capture steady-state populated snapshot, not the initially empty one.
        for _ in 0..2 {
            mod_api::dispatch_block_entity_tick(&mut World, [0, 64, 0], &mut entity);
        }
        assert_eq!(entity.data, [23]);
        assert_eq!(
            entity.nbt.get(&"counter".into()),
            Some(&mod_api::Tag::Int(17))
        );
    });
    assert!(!module.is_stopped());
    let noop_id = crate::boundary::export_id("Hooks", "server_stopped").unwrap();
    let counter_id = crate::boundary::export_id("BlockEntityPart", "block_entity_tick").unwrap();
    // One group lock outside every timing. No native host imports occur in these hooks.
    let mut group = module.running.group.lock().unwrap();
    let config = group.store.engine();
    println!(
        "engine debug={} signals={} reservation={} guard={} growth={} parallel={} opt={:?} regalloc={:?}",
        config.get_debug_info(),
        config.get_signals_based_traps(),
        config.get_memory_reservation(),
        config.get_memory_guard_size(),
        config.get_memory_reservation_for_growth(),
        config.get_parallel_compilation(),
        config.get_cranelift_opt_level(),
        config.get_cranelift_regalloc_algorithm()
    );
    let input = group.store.data().inputs[1].bytes().to_vec();
    assert!(
        input.len() > 8,
        "normal dispatch must have captured handler plus populated snapshot"
    );
    let instance = group
        .members
        .iter()
        .find(|(name, _)| name == "wasm_fixture")
        .unwrap()
        .1;
    let layout = instance
        .get_typed_func::<(), u64>(&mut group.store, "horizon_layout")
        .unwrap();
    let memory = group.store.data().memory.unwrap();
    for pass in 0..3 {
        let phases = if pass % 2 == 0 {
            [0, 1, 2, 3, 4, 5]
        } else {
            [5, 4, 3, 2, 1, 0]
        };
        for phase in phases {
            group.store.set_epoch_deadline(30_000);
            let memory_before = memory.data_size(&group.store);
            let mut samples = Vec::with_capacity(100);
            for round in 0..120 {
                let mut nanos = 0;
                if phase < 2 || phase == 4 {
                    let now = Instant::now();
                    for _ in 0..1000 {
                        match phase {
                            0 => assert_eq!(
                                std::hint::black_box(layout.call(&mut group.store, ()).unwrap()),
                                mod_api::wire::LAYOUT
                            ),
                            1 => {
                                std::hint::black_box(
                                    module
                                        .input
                                        .call(&mut group.store, input.len() as u32)
                                        .unwrap(),
                                );
                            }
                            4 => {
                                let ptr = module
                                    .input
                                    .call(&mut group.store, input.len() as u32)
                                    .unwrap();
                                memory
                                    .write(&mut group.store, ptr as usize, &input)
                                    .unwrap();
                                let answer =
                                    module.hook.call(&mut group.store, counter_id).unwrap();
                                assert_ne!(answer as u32, u32::MAX);
                                std::hint::black_box(answer);
                            }
                            _ => unreachable!(),
                        }
                    }
                    nanos = now.elapsed().as_nanos();
                } else {
                    for _ in 0..1000 {
                        // Refill the consumed pending buffer outside hook-only timing.
                        let bytes = if phase == 2 {
                            &[][..]
                        } else {
                            input.as_slice()
                        };
                        let input_now = (phase == 5).then(Instant::now);
                        let ptr = module
                            .input
                            .call(&mut group.store, bytes.len() as u32)
                            .unwrap();
                        if let Some(now) = input_now {
                            nanos += now.elapsed().as_nanos();
                        }
                        memory.write(&mut group.store, ptr as usize, bytes).unwrap();
                        let hook_now = (phase != 5).then(Instant::now);
                        let answer = module
                            .hook
                            .call(
                                &mut group.store,
                                if phase == 2 { noop_id } else { counter_id },
                            )
                            .unwrap();
                        if let Some(now) = hook_now {
                            nanos += now.elapsed().as_nanos();
                        }
                        let len = answer as u32;
                        assert_ne!(len, u32::MAX, "guest hook refused");
                        let ptr = (answer >> 32) as usize;
                        let output = memory
                            .data(&group.store)
                            .get(ptr..ptr.checked_add(len as usize).unwrap())
                            .expect("valid reply range");
                        let mut reader = Reader::new(output);
                        reader.take::<()>().unwrap();
                        if phase != 2 {
                            assert_eq!(reader.take::<[i32; 3]>().unwrap(), [0, 64, 0]);
                            let entity = mod_api::ModBlockEntity::from(
                                reader
                                    .take::<mod_api::containers::ModBlockEntitySnapshot>()
                                    .unwrap(),
                            );
                            assert_eq!(entity.data, [23]);
                            assert_eq!(
                                entity.nbt.get(&"counter".into()),
                                Some(&mod_api::Tag::Int(17))
                            );
                        }
                        assert!(reader.rest().is_empty());
                    }
                }
                if round >= 20 {
                    samples.push(nanos);
                }
            }
            samples.sort_unstable();
            let mean = samples.iter().sum::<u128>() as f64 / samples.len() as f64 / 1000.0;
            println!(
                "memory pass={pass} phase={phase} before={memory_before} after={}",
                memory.data_size(&group.store)
            );
            println!(
                "direct pass={pass} phase={phase} mean={mean:.3} ns/call p95={:.3} ns/call input={} bytes",
                samples[94] as f64 / 1000.0,
                input.len()
            );
        }
    }
    let now = Instant::now();
    for _ in 0..1000000 {
        std::hint::black_box(Instant::now());
    }
    println!(
        "Instant calibration {:.3} ns/read",
        now.elapsed().as_nanos() as f64 / 1000000.0
    );
}
