use super::*;

#[test]
#[ignore = "optimized hot-hook microbenchmark"]
fn subscriber_tick_benchmark() {
    use std::{hint::black_box, time::Instant};
    struct TestHost;
    impl Host for TestHost {}
    impl ClientHost for TestHost {}
    struct Quiet;
    #[mod_api::__subscriptions]
    impl Hooks for Quiet {}
    struct Listener;
    #[mod_api::__subscriptions]
    impl Hooks for Listener {
        fn client_tick_pre(&self, _: &mut dyn ClientHost, event: &mut ClientTickPreEvent) {
            black_box(event);
        }
    }
    trait Before {
        fn client_tick_pre(&self, _: &mut dyn ClientHost, _: &mut ClientTickPreEvent) {}
    }
    impl Before for Quiet {}
    impl Before for Listener {
        fn client_tick_pre(&self, _: &mut dyn ClientHost, event: &mut ClientTickPreEvent) {
            black_box(event);
        }
    }
    struct BeforeMod {
        id: &'static str,
        hooks: Box<dyn Before>,
    }
    let mut before: Vec<_> = (0..8)
        .map(|_| BeforeMod {
            id: "quiet",
            hooks: Box::new(Quiet),
        })
        .collect();
    before.push(BeforeMod {
        id: "listener",
        hooks: Box::new(Listener),
    });
    let mut installed: Vec<_> = (0..8)
        .map(|_| ModEntry::new("quiet", Box::new(Quiet)))
        .collect();
    installed.push(ModEntry::new("listener", Box::new(Listener)));
    install(installed.into());
    let mut host = TestHost;
    dispatch_client_tick_pre(&mut host); // Warm the dispatch outside the timed loop.
    let iterations = 1_000_000;
    let start = Instant::now();
    for _ in 0..iterations {
        let mut event = ClientTickPreEvent {};
        for entry in black_box(&before) {
            let _guard = mark_mod(entry.id, "client_tick_pre");
            entry.hooks.client_tick_pre(&mut host, &mut event);
        }
    }
    let baseline = start.elapsed().as_nanos() as f64 / f64::from(iterations);
    let start = Instant::now();
    for _ in 0..iterations {
        dispatch_client_tick_pre(black_box(&mut host));
    }
    let subscribed = start.elapsed().as_nanos() as f64 / f64::from(iterations);
    println!(
        "tick dispatch: before {baseline:.2} ns, subscribed {subscribed:.2} ns per call ({iterations} iterations)"
    );
}

#[test]
fn eager_subscribers_are_shared_across_dispatch_threads() {
    use std::sync::Barrier;
    struct Quiet;
    #[mod_api::__subscriptions]
    impl Hooks for Quiet {}
    struct Listener;
    #[mod_api::__subscriptions]
    impl Hooks for Listener {
        fn loot_table_load(&self, _: &dyn DataLoadContext, _: &mut LootTableLoadEvent<'_>) {}
    }
    let installed = install(
        vec![
            ModEntry::new("quiet", Box::new(Quiet)),
            ModEntry::new("listener", Box::new(Listener)),
        ]
        .into(),
    );
    assert!(
        !installed
            .subscribers
            .contains_any(HookId::BabyEntitySpawn as usize)
    );
    assert_eq!(
        installed
            .subscribers
            .indices(HookId::LootTableLoad as usize)
            .collect::<Vec<_>>(),
        [1]
    );
    let barrier = Barrier::new(2);
    let load = horizon_world::native_load_context::LoadContext::new(
        horizon_world::native_load_context::DataLoadOrigin::ServerDataLoad,
        std::sync::Arc::new(horizon_world::native_load_context::LoadInputs::new(
            Default::default(),
            std::path::PathBuf::new(),
        )),
        [],
    );
    let context = crate::OwningLoad::new(&load);
    let context = &context;
    std::thread::scope(|scope| {
        for _ in 0..2 {
            let barrier = &barrier;
            scope.spawn(move || {
                TEST_MODS.set(Some(installed));
                barrier.wait();
                dispatch_loot_table_load(context, "test", &mut serde_json::json!([]));
            });
        }
    });
    assert_eq!(
        installed
            .subscribers
            .indices(HookId::LootTableLoad as usize)
            .collect::<Vec<_>>(),
        [1]
    );
}

#[test]
fn scoped_mods_dispatch_and_restore_after_panic() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use testing::with_mods;

    struct HostDouble;
    impl Host for HostDouble {}
    impl ClientHost for HostDouble {}
    struct Counter(Arc<AtomicUsize>);
    #[mod_api::__subscriptions]
    impl Hooks for Counter {
        fn client_tick_pre(&self, _: &mut dyn ClientHost, _: &mut ClientTickPreEvent) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }

    let outer = Arc::new(AtomicUsize::new(0));
    let inner = Arc::new(AtomicUsize::new(0));
    with_mods(
        vec![ModEntry::new(
            "outer-counter",
            Box::new(Counter(outer.clone())),
        )]
        .into(),
        || {
            dispatch_client_tick_pre(&mut HostDouble);
            let result = std::panic::catch_unwind(|| {
                with_mods(
                    vec![ModEntry::new(
                        "inner-counter",
                        Box::new(Counter(inner.clone())),
                    )]
                    .into(),
                    || {
                        dispatch_client_tick_pre(&mut HostDouble);
                        panic!("test unwind");
                    },
                );
            });
            assert!(result.is_err());
            dispatch_client_tick_pre(&mut HostDouble);
        },
    );
    assert_eq!(outer.load(Ordering::Relaxed), 2);
    assert_eq!(inner.load(Ordering::Relaxed), 1);
    assert!(mods().is_empty());
}

#[test]
fn native_switch_skips_and_resumes_subscribed_hooks() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct HostDouble;
    impl Host for HostDouble {}
    impl ClientHost for HostDouble {}
    struct Counter(Arc<AtomicUsize>);
    #[mod_api::__subscriptions]
    impl Hooks for Counter {
        fn client_tick_pre(&self, _: &mut dyn ClientHost, _: &mut ClientTickPreEvent) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }
    let count = Arc::new(AtomicUsize::new(0));
    testing::with_mods(
        vec![ModEntry::new("counter", Box::new(Counter(count.clone())))].into(),
        || {
            assert_eq!(__private::mod_index("counter"), Some(0));
            dispatch_client_tick_pre(&mut HostDouble);
            assert!(__private::set_mod_enabled("counter", false));
            dispatch_client_tick_pre(&mut HostDouble);
            assert!(__private::set_mod_enabled("counter", true));
            dispatch_client_tick_pre(&mut HostDouble);
            assert!(!__private::set_mod_enabled("missing", false));
        },
    );
    assert_eq!(count.load(Ordering::Relaxed), 2);
}

#[test]
fn movement_input_writes_back_all_fields() {
    struct TestHost;
    impl Host for TestHost {}
    impl ServerHost for TestHost {}
    impl ClientHost for TestHost {}
    struct Movement;
    #[mod_api::__subscriptions]
    impl Hooks for Movement {
        fn movement_input(&self, _host: &dyn ClientHost, event: &mut MovementInputEvent) {
            event.forward = -0.5;
            event.strafe = 0.25;
            event.jump = true;
            event.sneak = true;
        }
    }
    let mods = [ModEntry::new("movement", Box::new(Movement))];
    install(mods.into());
    let (mut forward, mut strafe, mut jump, mut sneak) = (1.0, 0.0, false, false);
    dispatch_movement_input(&TestHost, &mut forward, &mut strafe, &mut jump, &mut sneak);
    assert_eq!((forward, strafe, jump, sneak), (-0.5, 0.25, true, true));
}

#[test]
fn recipe_book_categories_accept_mod_declarations() {
    use mod_api::{RecipeBookCategoriesEvent, RecipeBookCategory, dispatch_recipe_book_categories};
    struct TestHost;
    impl Host for TestHost {}
    impl ServerHost for TestHost {}
    struct Categories;
    #[mod_api::__subscriptions]
    impl Hooks for Categories {
        fn recipe_book_categories(
            &self,
            _host: crate::HostSide,
            event: &mut RecipeBookCategoriesEvent<'_>,
        ) {
            event.categories.push(RecipeBookCategory {
                id: "mymod:crushing".into(),
                recipe_type: "mymod:crusher".into(),
                icon: "minecraft:stone".into(),
            });
        }
    }
    let mut declarations = Vec::new();
    dispatch_recipe_book_categories(HostSide::Server(&mut TestHost), &mut declarations);
    assert!(declarations.is_empty());
    let mods = [ModEntry::new("categories", Box::new(Categories))];
    install(mods.into());
    dispatch_recipe_book_categories(HostSide::Server(&mut TestHost), &mut declarations);
    assert_eq!(declarations.len(), 1);
    assert_eq!(declarations[0].id, "mymod:crushing");
    assert_eq!(declarations[0].recipe_type, "mymod:crusher");
    assert_eq!(declarations[0].icon, "minecraft:stone");
}

#[test]
fn client_tick_pre_and_post_hear_their_own_moment() {
    use std::sync::Mutex;
    struct TestHost;
    impl Host for TestHost {}
    impl ClientHost for TestHost {}
    struct Ticks(Arc<Mutex<Vec<&'static str>>>);
    #[mod_api::__subscriptions]
    impl Hooks for Ticks {
        fn client_tick_pre(&self, _: &mut dyn ClientHost, _: &mut ClientTickPreEvent) {
            self.0.lock().unwrap().push("pre");
        }
        fn client_tick_post(&self, _: &mut dyn ClientHost, _: &mut ClientTickPostEvent) {
            self.0.lock().unwrap().push("post");
        }
    }
    let heard = Arc::new(Mutex::new(Vec::new()));
    testing::with_mods(
        vec![ModEntry::new("ticks", Box::new(Ticks(heard.clone())))].into(),
        || {
            dispatch_client_tick_pre(&mut TestHost);
            dispatch_client_tick_post(&mut TestHost);
        },
    );
    assert_eq!(*heard.lock().unwrap(), ["pre", "post"]);
}

#[test]
fn render_frame_pre_and_post_hear_their_own_moment_with_the_frames_time() {
    use std::{sync::Mutex, time::Duration};
    struct TestHost;
    impl Host for TestHost {}
    impl ClientHost for TestHost {}
    struct Frames(Arc<Mutex<Vec<(&'static str, Duration)>>>);
    #[mod_api::__subscriptions]
    impl Hooks for Frames {
        fn render_frame_pre(&self, _: &mut dyn ClientHost, event: &mut RenderFramePreEvent) {
            self.0.lock().unwrap().push(("pre", event.elapsed));
        }
        fn render_frame_post(&self, _: &mut dyn ClientHost, event: &mut RenderFramePostEvent) {
            self.0.lock().unwrap().push(("post", event.elapsed));
        }
    }
    let heard = Arc::new(Mutex::new(Vec::new()));
    testing::with_mods(
        vec![ModEntry::new("frames", Box::new(Frames(heard.clone())))].into(),
        || {
            let frame = Duration::from_millis(16);
            dispatch_render_frame_pre(&mut TestHost, frame);
            dispatch_render_frame_post(&mut TestHost, frame);
        },
    );
    let frame = Duration::from_millis(16);
    assert_eq!(*heard.lock().unwrap(), [("pre", frame), ("post", frame)]);
}

#[test]
fn render_gui_pre_cancel_reaches_the_game() {
    use std::sync::Mutex;
    struct TestHost;
    impl Host for TestHost {}
    impl ClientHost for TestHost {}
    struct Gui(Arc<Mutex<Vec<&'static str>>>, bool);
    #[mod_api::__subscriptions]
    impl Hooks for Gui {
        fn render_gui_pre(&self, _: &dyn ClientHost, _: &mut RenderGuiPreEvent) -> Flow {
            self.0.lock().unwrap().push("pre");
            if self.1 { Flow::Cancel } else { Flow::Continue }
        }
    }
    let heard = Arc::new(Mutex::new(Vec::new()));
    let mut canceled = false;
    testing::with_mods(
        vec![ModEntry::new("gui", Box::new(Gui(heard.clone(), true)))].into(),
        || dispatch_render_gui_pre(&TestHost, &mut canceled),
    );
    assert!(canceled, "a mod's cancel reaches the game");
    assert_eq!(*heard.lock().unwrap(), ["pre"]);
}

#[test]
fn window_resize_delivers_physical_and_gui_dimensions() {
    use std::sync::Mutex;
    struct TestHost;
    impl Host for TestHost {}
    impl ClientHost for TestHost {}
    struct Listener(Arc<Mutex<Vec<(u32, u32, u32, u32)>>>);
    #[mod_api::__subscriptions]
    impl Hooks for Listener {
        fn window_resize(&self, _: &mut dyn ClientHost, event: &mut WindowResizeEvent) {
            self.0.lock().unwrap().push((
                event.width,
                event.height,
                event.gui_width,
                event.gui_height,
            ));
        }
    }
    let heard = Arc::new(Mutex::new(Vec::new()));
    testing::with_mods(
        vec![ModEntry::new("resize", Box::new(Listener(heard.clone())))].into(),
        || {
            dispatch_window_resize(&mut TestHost, 1200, 900, 400, 300);
        },
    );
    assert_eq!(*heard.lock().unwrap(), [(1200, 900, 400, 300)]);
}
