use super::*;

#[test]
fn scoped_mods_dispatch_and_restore_after_panic() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use testing::with_mods;

    struct HostDouble;
    impl Host for HostDouble {}
    struct Counter(Arc<AtomicUsize>);
    impl Hooks for Counter {
        fn tick(&self, _: &mut dyn Host, _: &mut TickEvent) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }

    let outer = Arc::new(AtomicUsize::new(0));
    let inner = Arc::new(AtomicUsize::new(0));
    with_mods(
        vec![Box::new(Counter(outer.clone())) as Box<dyn Hooks>].into(),
        || {
            dispatch_tick(&mut HostDouble);
            let result = std::panic::catch_unwind(|| {
                with_mods(
                    vec![Box::new(Counter(inner.clone())) as Box<dyn Hooks>].into(),
                    || {
                        dispatch_tick(&mut HostDouble);
                        panic!("test unwind");
                    },
                );
            });
            assert!(result.is_err());
            dispatch_tick(&mut HostDouble);
        },
    );
    assert_eq!(outer.load(Ordering::Relaxed), 2);
    assert_eq!(inner.load(Ordering::Relaxed), 1);
    assert!(mods().is_empty());
}

#[test]
fn movement_input_writes_back_all_fields() {
    struct TestHost;
    impl Host for TestHost {}
    struct Movement;
    impl Hooks for Movement {
        fn movement_input(&self, _host: &dyn Host, event: &mut MovementInputEvent) {
            event.forward = -0.5;
            event.strafe = 0.25;
            event.jump = true;
            event.sneak = true;
        }
    }
    let mods: Vec<Box<dyn Hooks>> = vec![Box::new(Movement)];
    install(mods.into());
    let (mut forward, mut strafe, mut jump, mut sneak) = (1.0, 0.0, false, false);
    dispatch_movement_input(&TestHost, &mut forward, &mut strafe, &mut jump, &mut sneak);
    assert_eq!((forward, strafe, jump, sneak), (-0.5, 0.25, true, true));
}

#[test]
fn recipe_book_categories_accept_mod_declarations() {
    use horizon_client::mod_api::{
        RecipeBookCategoriesEvent, RecipeBookCategory, dispatch_recipe_book_categories,
    };
    struct TestHost;
    impl Host for TestHost {}
    struct Categories;
    impl Hooks for Categories {
        fn recipe_book_categories(
            &self,
            _host: &dyn Host,
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
    dispatch_recipe_book_categories(&TestHost, &mut declarations);
    assert!(declarations.is_empty());
    let mods: Vec<Box<dyn Hooks>> = vec![Box::new(Categories)];
    install(mods.into());
    dispatch_recipe_book_categories(&TestHost, &mut declarations);
    assert_eq!(declarations.len(), 1);
    assert_eq!(declarations[0].id, "mymod:crushing");
    assert_eq!(declarations[0].recipe_type, "mymod:crusher");
    assert_eq!(declarations[0].icon, "minecraft:stone");
}
