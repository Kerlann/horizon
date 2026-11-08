use super::*;

#[path = "runtime_export_bench.rs"]
mod export_profile;

struct Random<F>(F);
impl<F: FnMut() -> i64> mod_api::StructureRandom for Random<F> {
    fn next_int_bound(&mut self, _: i32) -> i32 {
        0
    }
    fn next_float(&mut self) -> f32 {
        0.0
    }
    fn next_double(&mut self) -> f64 {
        0.0
    }
    fn next_boolean(&mut self) -> bool {
        false
    }
    fn next_long(&mut self) -> i64 {
        (self.0)()
    }
}

fn guest(layout: u64, body: &str) -> Vec<u8> {
    wat::parse_str(format!(
        r#"(module
      (@custom "dylink.0" "\01\04\00\00\00\00")
      (import "env" "memory" (memory 1))
      (import "env" "__indirect_function_table" (table 1 funcref))
      (import "horizon" "host" (func $host (param i32 i32 i32 i32 i32) (result i32)))
      (import "horizon" "host_reply" (func $reply (param i32)))
      (func (export "horizon_layout") (result i64) i64.const {layout})
      (global $in_len (mut i32) (i32.const 0))
      (func (export "horizon_in") (param i32) (result i32) local.get 0 global.set $in_len i32.const 512)
      (func (export "horizon_hook") (param i32) (result i64) (local i32 i32)
        i32.const 512 local.set 1 global.get $in_len local.set 2 {body})
    )"#
    ))
    .unwrap()
}
fn module(body: &str) -> WasmMod {
    let engine = engine().unwrap();
    let bytes = guest(mod_api::wire::LAYOUT, body);
    WasmMod::instantiate(
        &engine,
        &[Member {
            name: "test",
            module: &bytes,
        }],
    )
    .unwrap()
    .remove(0)
}
fn call(module: &WasmMod) -> Option<u32> {
    module.call(
        CallClass::Hook,
        Frame::default(),
        0,
        None,
        |w| w.put(&42u32),
        |r| r.take(),
    )
}
#[test]
fn hook_round_trip_and_malformed_reply_poison() {
    // Echo the four-byte input from horizon_in.
    let module = module("i64.const 2199023255556"); // 512 << 32 | 4
    assert_eq!(call(&module), Some(42));
    assert!(!module.running.poisoned.load(Ordering::Relaxed));
    let module = super::runtime_tests::module("i64.const -4294967292"); // ptr = u32::MAX
    assert_eq!(call(&module), None);
    assert!(module.running.poisoned.load(Ordering::Relaxed));
    assert_eq!(call(&module), None);
}
#[test]
fn encoding_failure_refuses_partial_arguments() {
    let module = module("i64.const 2199023255556");
    assert_eq!(
        module.call(
            CallClass::Hook,
            Frame::default(),
            0,
            None,
            |w| {
                w.put(&42u32);
                w.fail(WireError::Invalid("resource refused"));
            },
            |r| r.take::<u32>()
        ),
        None
    );
    assert!(module.is_stopped());
}

#[test]
fn nested_handler_decodes_in_scope_and_remains_callable_after_return() {
    let bounds = crate::boundary::export_id("StructurePiece", "bounding_box").unwrap();
    let random_id = crate::boundary::import_id("StructureRandom", "next_long").unwrap();
    for handler in [123, 999] {
        let module = module(&format!(
            r#"
        local.get 0 i32.const {bounds} i32.eq if (result i64)
          local.get 1 i64.load i64.const 123 i64.ne if unreachable end
          i32.const 1024 i32.const 1 i32.store
          i32.const 1028 i32.const 2 i32.store
          i32.const 1032 i32.const 3 i32.store
          i32.const 1036 i32.const 4 i32.store
          i32.const 1040 i32.const 5 i32.store
          i32.const 1044 i32.const 6 i32.store
          i64.const 4398046511128
        else local.get 0 i32.const 1 i32.eq if (result i64)
          i32.const {random_id} i32.const 0 i32.const 0 i32.const 2048 i32.const 8 call $host
          i32.const 8 i32.ne if unreachable end
          i64.const 8796093022216
        else
          i64.const 2199023255552 local.get 2 i64.extend_i32_u i64.or
        end end"#
        ));
        let found: mod_api::FoundStructure = module
            .call(
                CallClass::Hook,
                Frame::default(),
                0,
                None,
                |w| {
                    w.put(&[9i32, 10, 11]);
                    w.put(&vec![Id(handler)]);
                },
                |r| r.take(),
            )
            .unwrap();
        assert_eq!(found.position, [9, 10, 11]);
        if handler == 123 {
            let bounds = found.pieces[0].bounding_box();
            assert_eq!(bounds.min, [1, 2, 3]);
            assert_eq!(bounds.max, [4, 5, 6]);
        }
        let mut random = Random(|| i64::from(found.pieces[0].bounding_box().max[2]));
        let mut frame = Frame::default();
        frame.set_structure_random_mut(&mut random);
        let answer = module.call(CallClass::Load, frame, 1, None, |_| {}, |r| r.take::<i64>());
        if handler == 123 {
            assert_eq!(answer, Some(6));
        }
        assert_eq!(module.is_stopped(), handler != 123);
    }
}

#[test]
fn guest_panic_text_is_logged_with_mod_id_and_no_reply() {
    struct Capture(Mutex<Vec<String>>);
    impl log::Log for Capture {
        fn enabled(&self, _: &log::Metadata<'_>) -> bool {
            true
        }
        fn log(&self, record: &log::Record<'_>) {
            self.0.lock().unwrap().push(record.args().to_string());
        }
        fn flush(&self) {}
    }
    static CAPTURE: Capture = Capture(Mutex::new(Vec::new()));
    log::set_logger(&CAPTURE).unwrap();
    log::set_max_level(log::LevelFilter::Error);
    for trailing in [false, true] {
        let module = module(&format!(
            "i32.const {} local.get 1 local.get 2 i32.const -1 i32.const 0 call $host i32.const 0 i32.ne if unreachable end i64.const 0",
            mod_api::wire::HOST_LOG
        ));
        let answer = module.call(
            CallClass::Hook,
            Frame::default(),
            0,
            None,
            |w| {
                w.put(&"guest panic at register");
                if trailing {
                    w.put(&1u8);
                }
            },
            |r| r.take::<()>(),
        );
        assert_eq!(answer, if trailing { None } else { Some(()) });
        assert_eq!(module.is_stopped(), trailing);
    }
    let messages = CAPTURE.0.lock().unwrap();
    assert_eq!(
        messages
            .iter()
            .filter(|message| message.as_str() == "wasm mod test: guest panic at register")
            .count(),
        1
    );
}

#[test]
fn calls_do_not_leave_an_active_module_ownership_cycle() {
    let module = module("i64.const 2199023255556");
    let running = Arc::downgrade(&module.running);
    assert_eq!(call(&module), Some(42));
    drop(module);
    assert!(running.upgrade().is_none());
}

#[test]
fn trap_and_deadline_stop_the_group() {
    let module = module("unreachable");
    assert_eq!(call(&module), None);
    assert!(module.running.poisoned.load(Ordering::Relaxed));
    let module = super::runtime_tests::module("(loop br 0) unreachable");
    let started = Instant::now();
    assert_eq!(call(&module), None);
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(module.running.poisoned.load(Ordering::Relaxed));
}
#[test]
fn a_member_cannot_bypass_group_limits_with_private_memories() {
    let bytes =
        wat::parse_str(r#"(module (@custom "dylink.0" "\01\04\00\00\00\00") (memory 1))"#).unwrap();
    let refusal = WasmMod::instantiate(
        &engine().unwrap(),
        &[Member {
            name: "private",
            module: &bytes,
        }],
    )
    .err()
    .unwrap();
    assert!(refusal.to_string().contains("must import the group memory"));
}

#[test]
fn real_members_link_when_built() {
    let Some(dir) = std::env::var_os("HORIZON_WASM_TEST_MEMBERS") else {
        return;
    };
    let dir = std::path::Path::new(&dir);
    let (Ok(std), Ok(fixture)) = (
        std::fs::read(dir.join("std.wasm")),
        std::fs::read(dir.join("fixture.wasm")),
    ) else {
        return;
    };
    let engine = engine().unwrap();
    let modules = WasmMod::instantiate(
        &engine,
        &[
            Member {
                name: "std",
                module: &std,
            },
            Member {
                name: "fixture",
                module: &fixture,
            },
        ],
    );
    let modules = modules.unwrap_or_else(|why| panic!("real shared members did not load: {why}"));
    assert_eq!(modules.len(), 1);
    let subscriptions: Vec<String> = modules[0]
        .call(
            CallClass::Load,
            Frame::default(),
            crate::boundary::SUBSCRIBED,
            None,
            |_| {},
            |r| r.take(),
        )
        .expect("real guest subscription hook failed");
    assert!(
        subscriptions
            .iter()
            .all(|name| crate::boundary::hook_id(name).is_some())
    );
    let event: mod_api::ClientTickPostEvent = Reader::new(&[]).take().unwrap();
    let tick = crate::boundary::export_id("Hooks", "client_tick_post").unwrap();
    assert_eq!(
        modules[0].call(
            CallClass::Hook,
            Frame::default(),
            tick,
            None,
            |w| w.put(&event),
            |r| {
                r.take::<()>()?;
                r.take::<mod_api::ClientTickPostEvent>()?;
                Ok(())
            }
        ),
        Some(())
    );
}

#[test]
fn unresolved_namespaced_functions_are_not_stubbed() {
    let bytes = wat::parse_str(r#"(module (@custom "dylink.0" "\01\04\00\00\00\00") (import "env" "fixture:missing" (func)))"#).unwrap();
    let error = Group::link(
        &wasmtime::Engine::default(),
        &[Member {
            name: "test",
            module: &bytes,
        }],
    )
    .err()
    .unwrap();
    assert!(
        matches!(error, crate::link::LinkError::Unresolved { symbol, .. } if symbol == "env.fixture:missing")
    );
}

#[test]
fn linker_heap_symbols_follow_all_members_static_data() {
    let first = wat::parse_str(r#"(module (@custom "dylink.0" "\01\04\10\00\00\00"))"#).unwrap();
    let second = wat::parse_str(
        r#"(module (@custom "dylink.0" "\01\04\20\00\00\00")
        (import "GOT.mem" "__heap_base" (global $heap (mut i32)))
        (import "GOT.mem" "__data_end" (global $end (mut i32)))
        (import "GOT.mem" "__heap_end" (global $limit (mut i32)))
        (func (export "heap") (result i32) global.get $heap)
        (func (export "data") (result i32) global.get $end)
        (func (export "limit") (result i32) global.get $limit))"#,
    )
    .unwrap();
    let engine = wasmtime::Engine::default();
    let mut group = Group::link(
        &engine,
        &[
            Member {
                name: "first",
                module: &first,
            },
            Member {
                name: "second",
                module: &second,
            },
        ],
    )
    .unwrap();
    for symbol in ["heap", "data"] {
        let mut output = [wasmtime::Val::I32(0)];
        group.call("second", symbol, &[], &mut output).unwrap();
        assert_eq!(output[0].i32(), Some(262192)); // 256 KiB stack + 16 + 32 bytes.
    }
    let mut output = [wasmtime::Val::I32(0)];
    group.call("second", "limit", &[], &mut output).unwrap();
    assert_eq!(output[0].i32(), Some(327680)); // Five initial memory pages.
}

#[test]
fn boundary_exports_are_per_member_but_other_horizon_symbols_are_not() {
    let bytes = guest(mod_api::wire::LAYOUT, "i64.const 0");
    let engine = engine().unwrap();
    let modules = WasmMod::instantiate(
        &engine,
        &[
            Member {
                name: "first",
                module: &bytes,
            },
            Member {
                name: "second",
                module: &bytes,
            },
        ],
    )
    .unwrap();
    assert_eq!(modules.len(), 2);
    let bytes = wat::parse_str(
        r#"(module (@custom "dylink.0" "\01\04\00\00\00\00") (func (export "horizon_shared")))"#,
    )
    .unwrap();
    let refusal = WasmMod::instantiate(
        &engine,
        &[
            Member {
                name: "first",
                module: &bytes,
            },
            Member {
                name: "second",
                module: &bytes,
            },
        ],
    )
    .err()
    .unwrap();
    assert!(
        refusal
            .to_string()
            .contains("exports horizon_shared, which first already does")
    );
}

#[test]
fn loader_initializes_and_installs_a_wasm_entry() {
    use mod_api::__private::game::Fields;
    let std = wat::parse_str(
        r#"(module (@custom "dylink.0" "\01\04\00\00\00\00") (import "env" "memory" (memory 1)))"#,
    )
    .unwrap();
    let module = guest(
        mod_api::wire::LAYOUT,
        r#"local.get 0 i32.const -256 i32.eq if (result i64)
            i32.const 4096 local.get 2 i32.store
            i32.const 2048 local.get 1 local.get 2 memory.copy
            i64.const 0
          else local.get 0 i32.const 2 i32.eq if (result i64)
            i64.const 8796093022208 i32.const 4096 i32.load i64.extend_i32_u i64.or
          else i64.const 4398046511108 end end"#,
    );
    let manifest = horizon_mod_package::manifest::Manifest::parse(&format!("id = 'roundtrip'\nname = 'Roundtrip'\nversion = '1.0.0'\napi = '0.2'\ncapabilities = ['registries']\n[wasm]\nsha512 = '{}'\ntoolchain = 'test'\nbuilt_with = 'test'\n", "a".repeat(128))).unwrap();
    let loaded = crate::load(&std, vec![crate::WasmPackage { manifest, module }]);
    assert!(loaded.refused.is_empty(), "{:?}", loaded.refused);
    assert_eq!(loaded.mods.len(), 1);
    assert_eq!(loaded.mods[0].fields().id, "roundtrip");
    assert_eq!(loaded.manifests[0].id, "roundtrip");
    let inventory: Vec<_> = ["native", "roundtrip", "content"]
        .into_iter()
        .map(|id| {
            mod_api::PackageInfo::new(
                id.into(),
                "1.0.0".into(),
                mod_api::PackageKind::Code,
                mod_api::Side::Both,
                vec![id.into()],
                Vec::new(),
            )
        })
        .collect();
    crate::initialize_modules(&loaded.modules, &inventory);
    let echoed: Vec<mod_api::PackageInfo> = loaded.modules[0]
        .call(
            CallClass::Hook,
            Frame::default(),
            2,
            None,
            |_| {},
            |r| r.take(),
        )
        .unwrap();
    assert_eq!(
        echoed.iter().map(|p| p.id()).collect::<Vec<_>>(),
        ["native", "roundtrip", "content"]
    );
}

#[test]
fn invalid_import_input_is_a_trap_not_a_host_panic() {
    let module = module(
        "i32.const -1 i32.const -1 i32.const 16 i32.const 0 i32.const 0 call $host drop i64.const 0",
    );
    assert_eq!(call(&module), None);
    assert!(module.running.poisoned.load(Ordering::Relaxed));
}

#[test]
fn layout_mismatch_is_a_mods_page_refusal() {
    let engine = engine().unwrap();
    let bytes = guest(mod_api::wire::LAYOUT ^ 1, "i64.const 0");
    let refusal = WasmMod::instantiate(
        &engine,
        &[Member {
            name: "test",
            module: &bytes,
        }],
    )
    .err()
    .unwrap();
    assert!(matches!(refusal, Refusal::Layout { .. }));
    assert_eq!(
        refusal.to_string(),
        "built for another version of the mod API"
    );
}
#[test]
fn point_record_layout_mismatch_is_refused() {
    let bytes = wat::parse_str(format!(
        r#"(module (@custom "dylink.0" "\01\04\00\00\00\00")
        (func (export "horizon_layout") (result i64) i64.const {})
        (func (export "horizon_layout_ext") (result i64) i64.const {})
        (func (export "horizon_in") (param i32) (result i32) i32.const 0)
        (func (export "horizon_hook") (param i32) (result i64) i64.const 0))"#,
        mod_api::wire::LAYOUT,
        mod_points::POINT_LAYOUT ^ 1
    ))
    .unwrap();
    let refusal = WasmMod::instantiate(
        &engine().unwrap(),
        &[Member {
            name: "points",
            module: &bytes,
        }],
    )
    .err()
    .unwrap();
    assert!(matches!(refusal, Refusal::PointLayout { .. }));
}

#[test]
fn callback_borrows_only_the_lent_context_and_restores_depth() {
    let module = module("i64.const 2199023255556");
    let mut group = module.running.group.lock().unwrap();
    group.store.set_epoch_deadline(100);
    group.store.data_mut().depth = 1;
    group.store.data_mut().cx.frames.push(Frame::default());
    let mut context = group.store.as_context_mut();
    let lending = context.data().lending.clone();
    lending.fetch_add(1, Ordering::Relaxed);
    let loan = Loan {
        lending,
        group: Arc::as_ptr(&module.running) as usize,
        context: Cell::new((&mut context as *mut StoreContextMut<'_, State>).cast()),
        previous: LENT.get(),
    };
    LENT.set(&loan);
    assert_eq!(
        module.call(
            CallClass::Callback,
            Frame::default(),
            0,
            None,
            |w| w.put(&99u32),
            |r| r.take::<u32>()
        ),
        Some(99)
    );
    drop(loan);
    assert_eq!(context.data().depth, 1);
    assert_eq!(context.data().cx.frames.len(), 1);
}

#[test]
fn import_reply_retry_survives_a_nested_callback() {
    let id = crate::boundary::import_id("StructureRandom", "next_long").unwrap();
    let module = module(&format!(
        r#"
        local.get 0 i32.const 1 i32.eq
        if (result i64)
          i64.const 2199023255560
        else
          i32.const {id} i32.const 0 i32.const 0 i32.const 0 i32.const 0 call $host
          i32.const 8 i32.ne if unreachable end
          i32.const 0 i64.load i64.const 0 i64.ne if unreachable end
          i32.const 1024 call $reply
          i64.const 4398046511112
        end"#
    ));
    let mut random = Random(|| {
        module
            .call(
                CallClass::Hook,
                Frame::default(),
                1,
                None,
                |w| w.put(&70i64),
                |r| r.take::<i64>(),
            )
            .unwrap_or(-1)
            + 1
    });
    let mut frame = Frame::default();
    frame.set_structure_random_mut(&mut random);
    assert_eq!(
        module.call(CallClass::Hook, frame, 0, None, |_| {}, |r| r.take::<i64>()),
        Some(71)
    );
}

#[test]
fn another_threads_hook_fails_fast_while_an_import_lends_the_group() {
    let id = crate::boundary::import_id("StructureRandom", "next_long").unwrap();
    let module = module(&format!(
        "i32.const {id} i32.const 0 i32.const 0 i32.const 2048 i32.const 8 call $host drop i64.const 8796093022216"
    ));
    let mut random = Random(|| {
        let module = module.clone();
        let (send, recv) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || send.send(call(&module)).unwrap());
        assert_eq!(
            recv.recv_timeout(Duration::from_secs(1))
                .expect("a Hook must not wait for the importing thread"),
            None
        );
        thread.join().unwrap();
        7
    });
    let mut frame = Frame::default();
    frame.set_structure_random_mut(&mut random);
    assert_eq!(
        module.call(CallClass::Load, frame, 0, None, |_| {}, |r| r.take::<i64>()),
        Some(7)
    );
    assert!(!module.is_stopped());
}

#[test]
fn nested_load_calls_do_not_extend_the_outer_deadline() {
    let id = crate::boundary::import_id("StructureRandom", "next_long").unwrap();
    let module = module(&format!(
        "local.get 0 i32.const 1 i32.eq if (result i64) i64.const 0 else i32.const {id} i32.const 0 i32.const 0 i32.const 2048 i32.const 8 call $host drop i64.const 8796093022216 end"
    ));
    let epoch = module.running.group.lock().unwrap().store.engine().clone();
    let entered = Cell::new(false);
    let mut random = Random(|| {
        entered.set(true);
        for _ in 0..1000 {
            epoch.increment_epoch();
        }
        assert_eq!(
            module.call(
                CallClass::Load,
                Frame::default(),
                1,
                None,
                |_| {},
                |r| r.take::<()>()
            ),
            None
        );
        7
    });
    let mut frame = Frame::default();
    frame.set_structure_random_mut(&mut random);
    let _ = module.call(CallClass::Hook, frame, 0, None, |_| {}, |r| r.take::<i64>());
    assert!(entered.get());
    assert!(module.is_stopped());
}

#[test]
fn dropping_a_handler_releases_its_id_on_the_next_call() {
    let module = module(
        r#"
        local.get 0 i32.const -255 i32.eq
        if (result i64)
          local.get 1 i32.load i32.const 1 i32.ne if unreachable end
          local.get 1 i64.load offset=4 i64.const 123 i64.ne if unreachable end
          i32.const 1024 i32.const 1 i32.store
          i64.const 0
        else
          i64.const 4398046511108
        end"#,
    );
    assert_eq!(call(&module), Some(0));
    drop(WasmHandler {
        module: module.clone(),
        index: Id(123),
    });
    assert_eq!(call(&module), Some(1));
    assert!(!module.running.poisoned.load(Ordering::Relaxed));
}

#[test]
fn memory_growth_limit_traps_and_poison_is_shared_by_proxies() {
    let module = module("i32.const 4096 memory.grow drop i64.const 0");
    let other = module.clone();
    assert_eq!(call(&module), None);
    assert!(other.running.poisoned.load(Ordering::Relaxed));
    assert_eq!(call(&other), None);
    let module =
        super::runtime_tests::module("ref.null func i32.const 1048576 table.grow drop i64.const 0");
    assert_eq!(call(&module), None);
    assert!(module.running.poisoned.load(Ordering::Relaxed));
}
