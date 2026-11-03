#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    #[derive(Default)]
    struct ItemStack {
        item: Arc<str>,
    }
    struct StackView {
        count: i32,
    }
    struct BlockEntityDraw(Vec<u32>);
    #[allow(dead_code)]
    mod stubs {
        #[derive(Clone, Copy, Debug, PartialEq)]
        pub(super) struct BlockStateId;
        #[derive(Clone, Copy)]
        pub(super) struct FluidState;
        pub(super) struct PlacementContext;
        #[derive(Clone, Copy)]
        pub(super) struct BlockPos;
        #[derive(Clone, Copy)]
        pub(super) struct BlockId;
        #[derive(Clone, Copy)]
        pub(super) struct Direction;
        #[derive(Clone, Copy)]
        pub(super) struct LevelDimension;
        pub(super) struct Ticks;
        pub(super) trait LevelHost {
            fn state_block(&self, _: BlockStateId) -> Option<&str> {
                None
            }
        }
        pub(super) trait LevelTicksHost: LevelHost {}
        pub(super) trait LevelRandomHost: LevelHost {}
        pub(super) trait LevelWriteHost: LevelTicksHost + LevelRandomHost {}
        pub(super) struct Compound;
        pub(super) struct Tag;
        pub(super) type NativeDispensed = (Option<super::ItemStack>, i32);
        pub(super) struct NativeLootDrops<'a>(pub &'a ());
        pub(super) struct GameRuleValue;
        pub(super) struct GameRuleActor<'a>(pub(super) &'a str);
        pub(super) struct NativePortalShape;
        pub(super) struct NativeNoteInstrument;
        pub(super) struct NativeNoteBlockChange;
        pub(super) struct NativeHandRender<'a>(pub &'a ());
        pub(super) struct NativeExplosionStart<'a>(pub &'a ());
        pub(super) struct NativeItemFished<'a>(pub &'a ());
        pub(super) struct NativeScreenView<'a>(pub &'a ());
        pub(super) struct StackedOnOther<'a> {
            pub(super) _lifetime: std::marker::PhantomData<&'a mut ()>,
        }
        pub(super) struct Corners;
        pub(super) struct Suggestion;
        pub(super) struct HudBatch;
        pub(super) struct BiomeColors;
        pub(super) struct SpawnEntry;
        pub(super) struct UseAnimation;
        #[derive(Clone, Copy)]
        pub(super) struct Hand;
        #[derive(Clone, Copy)]
        pub(super) struct BlockHit;
        pub(super) struct NativeBlockClick<'a>(pub &'a ());
        pub(super) struct NativeTriState;
        pub(super) struct NativeInteractionResult;
        pub(super) struct NativeMobRender<'a>(pub &'a ());
        pub(super) struct NativeShake;
        pub(super) struct ShieldBlock;
        pub(super) struct HurtSource<'a>(pub &'a ());
        #[derive(Clone, Copy)]
        pub(super) struct PhantomSpawnResult;
        #[derive(Clone, Copy)]
        pub(super) struct SleepProblem;
        pub(super) struct Music;
        pub(super) struct MusicLayer;
        pub(super) struct NativeBlockRim;
        pub(super) struct NativeColoredLight;
        pub(super) struct NativeMinimap;
        pub(super) struct NativeMod;
        pub(super) struct NativePanelEvent;
        pub(super) struct NativeScreen;
        pub(super) struct ModBlockEntity {
            pub(super) id: String,
        }
        pub(super) struct ModContainer;
        pub(super) struct MerchantOffer;
        pub(super) struct ModFluidTanks;
        pub(super) struct ModEnergyBuffer;
        pub(super) struct LootStack;
        pub(super) struct NativeLootContext<'a>(pub &'a ());
        pub(super) struct NativeDataMapType;
        pub(super) struct RecipeBookCategory;
        pub(super) struct NativeMapDecoration;
        pub(super) struct NativeMapInfo<'a>(pub &'a ());
        pub(super) struct NativePlayerRender<'a>(pub &'a ());
        pub(super) struct NativeCommand;
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub(super) enum NativeProgressType {
            Grant,
            Revoke,
        }
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub(super) enum NativePermissionLevel {
            All,
            Owners,
        }
        pub(super) struct NativeLifecycleContext;
        pub(super) struct ToolUse;
        pub(super) struct NativeServerPlayer;
        #[derive(Clone, Copy)]
        pub(super) struct NativePlayerFood;
        #[derive(Clone, Copy)]
        pub(super) struct LivingRef;
        #[derive(Clone, Copy)]
        pub(super) struct MinecartRef;
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub(super) enum NativeMinecartKind {
            Rideable,
        }
        pub(super) struct EffectTickChange;
        pub(super) struct NativeItemDrop;
        #[derive(Clone, Copy)]
        pub(super) struct EquipmentSlot;
        #[derive(Clone, Copy)]
        pub(super) struct ServerTickPhase;
        #[derive(Clone, Copy)]
        pub(super) struct WorldPhase;
    }
    use serde_json::{Map, Value};
    use stubs::*;
    struct Text;
    #[derive(Debug, PartialEq)]
    struct NativeZoom(u32);
    enum Flow {
        Continue,
        Cancel,
    }
    trait Host {}
    trait RenderHooks {}
    struct TestHost;
    impl Host for TestHost {}
    mod winit {
        pub mod event {
            pub struct KeyEvent;
        }
    }
    #[allow(dead_code)]
    mod generated {
        mod horizon_mods {
            pub mod content {
                pub struct Block;
                pub struct Item;
            }
        }
        pub(super) mod horizon_client {
            pub mod camera {
                pub struct CameraEntity;
                pub struct FovPlayer;
            }
            pub mod mod_api {
                pub struct NativeClientInfo;
                pub struct NativeArmorHurt;
            }
            pub mod menu {
                pub struct MenuButtonContext {
                    pub player: u128,
                    pub player_entity_id: i32,
                    pub pos: [i32; 3],
                    pub id: i32,
                }
                pub struct AnvilCraft;
                pub struct AnvilUpdate;
                pub struct GrindstonePlace;
                pub struct GrindstoneTake;
            }
            pub mod stations {
                pub struct EnchantmentRow<'a>(std::marker::PhantomData<&'a ()>);
            }
            pub mod fog {
                #[derive(Clone, Copy)]
                pub struct FogType;
                pub struct FogData;
            }
            pub mod first_person {
                #[derive(Clone, Copy)]
                pub struct Hand;
            }
            pub mod hud_layout {
                pub struct HealthInputs;
                pub struct GuiDraw;
            }
        }
        include!(concat!(env!("OUT_DIR"), "/mod_api_generated.rs"));
    }
    use generated::*;
    struct Installed {
        mods: Arc<[ModEntry]>,
        subscribers: horizon_client_core::subscribers::Subscribers,
        types: Box<[std::any::TypeId]>,
    }
    thread_local! {
        /// The hooks each dispatch entered, a mod at a time.
        static ENTERED: std::cell::RefCell<Vec<(usize, &'static str)>> = const { std::cell::RefCell::new(Vec::new()) };
        static MODS: std::cell::Cell<Option<&'static Installed>> = const { std::cell::Cell::new(None) };
    }
    fn install(mods: Arc<[ModEntry]>) {
        let types = mods
            .iter()
            .map(|entry| (&*entry.hooks as &dyn std::any::Any).type_id())
            .collect();
        let subscribers = horizon_client_core::subscribers::Subscribers::new(
            horizon_client_core::subscribers::Domain::Gameplay,
            HOOK_COUNT,
            mods.len(),
        );
        // Test dispatch needs the same static lifetime as the real installation.
        MODS.set(Some(Box::leak(Box::new(Installed {
            mods,
            subscribers,
            types,
        }))));
    }
    fn listened(hook: HookId) -> bool {
        MODS.get()
            .is_some_and(|installed| installed.subscribers.contains_any(hook as usize))
    }
    fn entered(hook_id: HookId, hook: &'static str) -> impl Iterator<Item = &'static dyn Hooks> {
        MODS.get().into_iter().flat_map(move |installed| {
            let mut guard = None;
            installed
                .subscribers
                .indices(hook_id as usize)
                .map(move |index| {
                    drop(guard.take());
                    let hooks = &*installed.mods[index].hooks;
                    guard = Some(horizon_client_core::subscribers::enter(
                        &installed.subscribers,
                        hook_id as usize,
                        index,
                        (hooks as *const dyn Hooks).cast(),
                        installed.types[index],
                    ));
                    ENTERED.with(|entered| entered.borrow_mut().push((index, hook)));
                    hooks
                })
        })
    }
    struct ModEntry {
        _id: &'static str,
        hooks: Box<dyn Hooks>,
    }
    impl ModEntry {
        fn new(id: &'static str, hooks: Box<dyn Hooks>) -> Self {
            Self { _id: id, hooks }
        }
    }
    struct Registrar;
    /// No parts: the scoped dispatch calls the mods only.
    struct Parts;
    fn parts(_: &'static str) -> Parts {
        Parts
    }
    impl Parts {
        fn block<'k>(
            &self,
            _: impl FnOnce() -> Option<&'k str>,
        ) -> &'static [&'static dyn BlockPart] {
            &[]
        }
        fn block_entity<'k>(
            &self,
            _: impl FnOnce() -> Option<&'k str>,
        ) -> &'static [&'static dyn BlockEntityPart] {
            &[]
        }
        fn block_entity_kind<'k>(
            &self,
            _: impl FnOnce() -> Option<&'k str>,
        ) -> &'static [&'static dyn BlockEntityPart] {
            &[]
        }
        fn item<'k>(
            &self,
            _: impl FnOnce() -> Option<&'k str>,
        ) -> &'static [&'static dyn ItemPart] {
            &[]
        }
        fn menu<'k>(
            &self,
            _: impl FnOnce() -> Option<&'k str>,
        ) -> &'static [&'static dyn MenuPart] {
            &[]
        }
    }
    fn entered_parts<'a, P: ?Sized>(
        parts: &'a [&'a P],
        _: &'static str,
    ) -> impl Iterator<Item = &'a P> {
        parts.iter().copied()
    }
    #[test]
    fn only_overrides_remain_after_first_dispatch_across_words() {
        struct Quiet;
        impl Hooks for Quiet {}
        struct Listener(Arc<std::sync::atomic::AtomicUsize>);
        impl Hooks for Listener {
            fn loot_table_load(&self, _: &mut LootTableLoadEvent<'_>) {
                self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        }
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mods: Vec<ModEntry> = (0..70)
            .map(|i| {
                if i == 66 {
                    ModEntry::new("listener", Box::new(Listener(calls.clone())))
                } else {
                    ModEntry::new("quiet", Box::new(Quiet))
                }
            })
            .collect();
        install(mods.into());
        ENTERED.with(|entered| entered.borrow_mut().clear());
        let mut table = serde_json::json!([]);
        dispatch_loot_table_load("test", &mut table);
        dispatch_loot_table_load("test", &mut table);
        assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 2);
        ENTERED.with(|entered| {
            let entered = entered.borrow();
            assert_eq!(entered.len(), 71);
            assert_eq!(entered[70], (66, "loot_table_load"));
        });
    }

    #[test]
    fn nested_default_on_another_zero_sized_type_does_not_prune_override() {
        struct Other;
        impl Hooks for Other {}
        struct Active;
        impl Hooks for Active {
            fn loot_table_load(&self, event: &mut LootTableLoadEvent<'_>) {
                dispatch_modify_default_components("test", &mut Map::new());
                Other.loot_table_load(event);
            }
        }
        install(vec![ModEntry::new("active", Box::new(Active))].into());
        ENTERED.with(|entered| entered.borrow_mut().clear());
        let mut table = serde_json::json!([]);
        dispatch_loot_table_load("test", &mut table);
        dispatch_loot_table_load("test", &mut table);
        ENTERED.with(|entered| {
            assert_eq!(
                entered
                    .borrow()
                    .iter()
                    .filter(|(_, hook)| *hook == "loot_table_load")
                    .count(),
                2
            );
        });
    }

    #[test]
    fn empty_dispatch_checks_before_event_construction() {
        let generated = include_str!(concat!(env!("OUT_DIR"), "/mod_api_generated.rs"));
        let function = generated
            .split("pub fn dispatch_loot_table_load(")
            .nth(1)
            .unwrap();
        let guard = function.find("super::listened").unwrap();
        let event = function.find("let mut event").unwrap();
        assert!(guard < event);
    }

    #[test]
    fn advancement_progress_reports_grant_and_revoke_identity() {
        struct Observer(Arc<Mutex<Vec<NativeProgressType>>>);
        impl Hooks for Observer {
            fn player_advancement_progress(
                &self,
                _: &mut dyn Host,
                event: &mut PlayerAdvancementProgressEvent<'_>,
            ) {
                assert_eq!(event.player, 123);
                assert_eq!(event.player_entity_id, 42);
                assert_eq!(event.name, "Alex");
                assert_eq!(event.advancement, "example:advancement");
                assert_eq!(event.criterion, "criterion");
                self.0.lock().unwrap().push(event.progress_type);
            }
        }
        let observed = Arc::new(Mutex::new(Vec::new()));
        let mods = [ModEntry::new(
            "observer",
            Box::new(Observer(observed.clone())),
        )];
        let mods: Arc<[ModEntry]> = mods.into();
        install(mods.clone());
        for progress_type in [NativeProgressType::Grant, NativeProgressType::Revoke] {
            dispatch_player_advancement_progress(
                &mut TestHost,
                123,
                42,
                "Alex",
                "example:advancement",
                "criterion",
                progress_type,
            );
        }
        assert_eq!(
            *observed.lock().unwrap(),
            vec![NativeProgressType::Grant, NativeProgressType::Revoke]
        );
    }
    struct Mod {
        id: u32,
        calls: Arc<Mutex<Vec<u32>>>,
    }
    impl Hooks for Mod {
        fn menu_button(&self, _: &mut dyn Host, event: &mut MenuButtonEvent<'_>) {
            assert_eq!(event.context.player, 123);
            assert_eq!(event.context.player_entity_id, 42);
            assert_eq!(event.context.pos, [1, 2, 3]);
            assert_eq!(event.context.id, 9);
            event.entity.id.push_str("-changed");
            *event.carried = Some(ItemStack {
                item: "bucket".into(),
            });
        }
        fn block_entity_submit(&self, _: &dyn Host, event: &mut BlockEntitySubmitEvent<'_>) {
            assert_eq!(event.pos, [1, 2, 3]);
            assert_eq!(event.block, "example:machine");
            event.draw.0.push(self.id);
        }
        fn living_entity_use_item_finish(
            &self,
            _: &mut dyn Host,
            event: &mut LivingEntityUseItemFinishEvent<'_>,
        ) {
            *event.result = None;
        }
        fn mouse_button_pre(&self, _: &mut dyn Host, _: &mut MouseButtonPreEvent) -> Flow {
            self.calls.lock().unwrap().push(self.id);
            if self.id == 1 {
                Flow::Cancel
            } else {
                Flow::Continue
            }
        }
        fn menu_slot_rules(&self, event: &mut MenuSlotRulesEvent<'_>) {
            event.may_place = false;
            event.max_stack = self.id;
        }
        fn living_entity_use_item_tick(
            &self,
            _: &mut dyn Host,
            event: &mut LivingEntityUseItemTickEvent<'_>,
        ) -> Flow {
            event.duration -= 1;
            if self.id == 1 {
                Flow::Cancel
            } else {
                Flow::Continue
            }
        }
        fn loot_table_load(&self, event: &mut LootTableLoadEvent<'_>) {
            event.table.as_array_mut().unwrap().push(self.id.into());
        }
        fn modify_default_components(&self, event: &mut ModifyDefaultComponentsEvent<'_>) {
            let value = event
                .components
                .entry("order")
                .or_insert_with(|| serde_json::json!([]));
            value.as_array_mut().unwrap().push(self.id.into());
        }
        fn level_tick_pre(&self, _: &mut dyn Host, _: &mut LevelTickPreEvent<'_>) {
            self.calls.lock().unwrap().push(self.id);
        }
        fn tooltip(&self, _: &dyn Host, event: &mut TooltipEvent<'_>) {
            event.lines.push((Text, self.id));
        }
        fn zoom(&self, _: &dyn Host, _: &mut ZoomEvent) -> Option<NativeZoom> {
            self.calls.lock().unwrap().push(self.id);
            Some(NativeZoom(self.id))
        }
        fn should_apply_effect_tick_this_tick(
            &self,
            _: &dyn Host,
            event: &mut ShouldApplyEffectTickThisTickEvent<'_>,
        ) {
            event.apply = !event.apply;
            self.calls.lock().unwrap().push(self.id);
        }
        fn living_incoming_damage(&self, event: &mut LivingIncomingDamageEvent<'_>) -> Flow {
            event.amount = event.amount * 2.0 + self.id as f32;
            if self.id == 2 {
                Flow::Cancel
            } else {
                Flow::Continue
            }
        }
        fn use_on(&self, _: &mut dyn Host, _: &mut UseOnEvent<'_>) -> Flow {
            self.calls.lock().unwrap().push(self.id);
            if self.id == 1 {
                Flow::Cancel
            } else {
                Flow::Continue
            }
        }
    }
    #[test]
    fn menu_button_mutates_entity_and_cursor_once() {
        let mods = [ModEntry::new(
            "menu-button",
            Box::new(Mod {
                id: 1,
                calls: Arc::new(Mutex::new(Vec::new())),
            }),
        )];
        let mods: Arc<[ModEntry]> = mods.into();
        install(mods.clone());
        let context = generated::horizon_client::menu::MenuButtonContext {
            player: 123,
            player_entity_id: 42,
            pos: [1, 2, 3],
            id: 9,
        };
        let mut entity = ModBlockEntity {
            id: "machine".into(),
        };
        let mut carried = None;
        dispatch_menu_button(&mut TestHost, &context, &mut entity, &mut carried);
        assert_eq!(entity.id, "machine-changed");
        assert_eq!(carried.unwrap().item.as_ref(), "bucket");
    }

    #[test]
    fn block_entity_submit_dispatches_in_order_with_shared_draw() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mods: Vec<ModEntry> = vec![
            ModEntry::new(
                "mod-0",
                Box::new(Mod {
                    id: 1,
                    calls: calls.clone(),
                }),
            ),
            ModEntry::new("mod-1", Box::new(Mod { id: 2, calls })),
        ];
        let mods: Arc<[ModEntry]> = mods.into();
        install(mods.clone());
        let mut draw = BlockEntityDraw(Vec::new());
        dispatch_block_entity_submit(
            &TestHost,
            [1, 2, 3],
            "example:machine",
            BlockStateId,
            &ModBlockEntity { id: String::new() },
            &mut draw,
        );
        assert_eq!(draw.0, [1, 2]);
    }

    #[test]
    fn slot_rules_mutate_and_item_tick_can_cancel() {
        let mods: Vec<ModEntry> = vec![ModEntry::new(
            "mod-2",
            Box::new(Mod {
                id: 1,
                calls: Arc::new(Mutex::new(Vec::new())),
            }),
        )];
        let mods: Arc<[ModEntry]> = mods.into();
        install(mods.clone());
        let (mut may_place, mut max_stack) = (true, 64);
        dispatch_menu_slot_rules(
            "test:menu",
            0,
            &ItemStack::default(),
            &mut may_place,
            &mut max_stack,
        );
        assert_eq!((may_place, max_stack), (false, 1));
        let (mut duration, mut canceled) = (10, false);
        dispatch_living_entity_use_item_tick(
            &mut TestHost,
            &ItemStack::default(),
            None,
            Hand,
            &mut duration,
            &mut canceled,
        );
        assert_eq!((duration, canceled), (9, true));
    }

    #[test]
    fn item_finish_can_clear_vanilla_result() {
        let mods: Vec<ModEntry> = vec![ModEntry::new(
            "mod-3",
            Box::new(Mod {
                id: 1,
                calls: Arc::new(Mutex::new(Vec::new())),
            }),
        )];
        let mods: Arc<[ModEntry]> = mods.into();
        install(mods.clone());
        let mut result = Some(ItemStack::default());
        dispatch_living_entity_use_item_finish(
            &mut TestHost,
            &ItemStack::default(),
            None,
            Hand,
            0,
            &mut result,
        );
        assert!(result.is_none());
    }

    #[test]
    fn fragment_cancel_stops_later_mods() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mods: Vec<ModEntry> = vec![
            ModEntry::new(
                "mod-4",
                Box::new(Mod {
                    id: 1,
                    calls: calls.clone(),
                }),
            ),
            ModEntry::new(
                "mod-5",
                Box::new(Mod {
                    id: 2,
                    calls: calls.clone(),
                }),
            ),
        ];
        let mods: Arc<[ModEntry]> = mods.into();
        install(mods.clone());
        let mut canceled = false;
        dispatch_mouse_button_pre(&mut TestHost, 0, true, 0, &mut canceled);
        assert!(canceled);
        assert_eq!(*calls.lock().unwrap(), [1]);
        // Each call is entered under the hook's name; the cancelled one's is the last.
        let entered = ENTERED.with(|entered| entered.take());
        assert_eq!(entered, [(0, "mouse_button_pre")]);
    }

    #[test]
    fn placement_replacement_can_refuse_and_stops_later_mods() {
        struct Level;
        impl LevelHost for Level {}
        struct Refuse(Arc<Mutex<Vec<u32>>>, u32);
        impl Hooks for Refuse {
            fn state_for_placement(
                &self,
                _: &dyn LevelHost,
                _: &mut StateForPlacementEvent<'_>,
            ) -> Option<Option<BlockStateId>> {
                self.0.lock().unwrap().push(self.1);
                Some(None)
            }
        }
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mods: Vec<ModEntry> = vec![
            ModEntry::new("refuse-6", Box::new(Refuse(calls.clone(), 1))),
            ModEntry::new("refuse-7", Box::new(Refuse(calls.clone(), 2))),
        ];
        let mods: Arc<[ModEntry]> = mods.into();
        install(mods.clone());
        let mut answer = None;
        dispatch_state_for_placement(
            &Level,
            LevelDimension,
            BlockStateId,
            &PlacementContext,
            FluidState,
            &mut answer,
        );
        assert_eq!(answer, Some(None));
        assert_eq!(*calls.lock().unwrap(), vec![1]);
        dispatch_state_for_placement(
            &Level,
            LevelDimension,
            BlockStateId,
            &PlacementContext,
            FluidState,
            &mut answer,
        );
        assert_eq!(*calls.lock().unwrap(), vec![1]);
    }

    #[test]
    fn item_model_property_dispatches_borrowed_stack_and_preserves_unhandled_answers() {
        struct Property;
        impl Hooks for Property {
            fn item_model_property(&self, _: &dyn Host, event: &mut ItemModelPropertyEvent<'_>) {
                if event.property == "example:charged" {
                    event.value = event.stack.count > 1;
                }
            }
        }
        let mods: Vec<ModEntry> = vec![ModEntry::new("property-8", Box::new(Property))];
        let mods: Arc<[ModEntry]> = mods.into();
        install(mods.clone());
        let host = TestHost;
        let mut value = false;
        dispatch_item_model_property(
            &host,
            "example:charged",
            &StackView { count: 2 },
            &mut value,
        );
        assert!(value);
        dispatch_item_model_property(&host, "other:property", &StackView { count: 1 }, &mut value);
        assert!(value);
        dispatch_item_model_property(
            &host,
            "example:charged",
            &StackView { count: 1 },
            &mut value,
        );
        assert!(!value);
        install(Arc::from([]));
        dispatch_item_model_property(
            &host,
            "example:charged",
            &StackView { count: 2 },
            &mut value,
        );
        assert!(!value);
    }

    #[test]
    fn incoming_damage_carries_the_attack_data() {
        struct Backstab;
        impl Hooks for Backstab {
            fn living_incoming_damage_v2(
                &self,
                _: &mut dyn Host,
                event: &mut LivingIncomingDamageV2Event<'_>,
            ) -> Flow {
                assert_eq!(event.causing, 7);
                let (from, at, look) = (
                    event.source_position.unwrap(),
                    event.victim_position,
                    event.victim_view,
                );
                let behind = (0..3)
                    .map(|axis| (from[axis] - at[axis]) * look[axis])
                    .sum::<f64>();
                if behind < 0.0 && event.weapon.is_some() {
                    event.amount *= 3.0;
                }
                Flow::Continue
            }
        }
        let mods: Vec<ModEntry> = vec![ModEntry::new("backstab-9", Box::new(Backstab))];
        let mods: Arc<[ModEntry]> = mods.into();
        install(mods.clone());
        let held = ItemStack::default();
        for (source_z, weapon, expected) in [
            (-1.0, Some(&held), 12.0),
            (1.0, Some(&held), 4.0),
            (-1.0, None, 4.0),
        ] {
            let (mut amount, mut canceled) = (4.0, false);
            dispatch_living_incoming_damage_v2(
                &mut TestHost,
                1,
                "minecraft:zombie",
                "minecraft:player_attack",
                7,
                Some([0.0, 0.0, source_z]),
                [0.0; 3],
                [0.0, 0.0, 1.0],
                weapon,
                &mut amount,
                &mut canceled,
            );
            assert_eq!((amount, canceled), (expected, false));
        }
    }

    #[test]
    fn dispatch_contracts() {
        struct Weapon;
        impl Hooks for Weapon {
            fn living_knockback_v2(&self, event: &mut LivingKnockbackV2Event<'_>) -> Flow {
                assert_eq!(
                    (event.entity, event.kind, event.attacker),
                    (12, "minecraft:armor_stand", 7)
                );
                assert!(event.attacker_main.is_none() && event.attacker_off.is_none());
                assert_eq!(event.strength, 0.4);
                event.strength *= 2.0;
                event.x = -event.x;
                Flow::Cancel
            }
        }
        let weapon_mods: Vec<ModEntry> = vec![ModEntry::new("weapon-10", Box::new(Weapon))];
        let weapon_mods: Arc<[ModEntry]> = weapon_mods.into();
        install(weapon_mods.clone());
        let (mut strength, mut x, mut z, mut canceled) = (0.4, 1.0, 0.0, false);
        dispatch_living_knockback_v2(
            12,
            "minecraft:armor_stand",
            7,
            None,
            None,
            &mut strength,
            &mut x,
            &mut z,
            &mut canceled,
        );
        assert_eq!((strength, x, z), (0.8, -1.0, 0.0));
        assert!(canceled);
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mods: Vec<ModEntry> = (1..=2)
            .map(|id| {
                ModEntry::new(
                    "mod-11",
                    Box::new(Mod {
                        id,
                        calls: calls.clone(),
                    }),
                )
            })
            .collect();
        let mods: Arc<[ModEntry]> = mods.into();
        install(mods.clone());
        let mut table = serde_json::json!([]);
        dispatch_loot_table_load("table", &mut table);
        assert_eq!(table, serde_json::json!([1, 2]));
        let mut components = Map::new();
        dispatch_modify_default_components("item", &mut components);
        assert_eq!(components["order"], serde_json::json!([1, 2]));
        let mut host = TestHost;
        dispatch_level_tick_pre(&mut host, "overworld");
        assert_eq!(*calls.lock().unwrap(), [1, 2]);
        calls.lock().unwrap().clear();
        let mut zoom = None;
        dispatch_zoom(&host, &mut zoom);
        assert_eq!(zoom, Some(NativeZoom(1)));
        assert_eq!(*calls.lock().unwrap(), [1]);
        calls.lock().unwrap().clear();
        dispatch_zoom(&host, &mut zoom);
        assert!(calls.lock().unwrap().is_empty());
        let mut lines = Vec::new();
        dispatch_tooltip(&host, &ItemStack::default(), &mut lines);
        assert_eq!(lines.iter().map(|line| line.1).collect::<Vec<_>>(), [1, 2]);
        let mut apply = false;
        calls.lock().unwrap().clear();
        dispatch_should_apply_effect_tick_this_tick(&host, "effect", 0, 0, &mut apply);
        assert!(!apply);
        assert_eq!(*calls.lock().unwrap(), [1, 2]);
        let mut amount = 1.0;
        let mut canceled = false;
        dispatch_living_incoming_damage(0, "kind", "damage", 0, 0, &mut amount, &mut canceled);
        // Each mod sees the one before: (1 × 2 + 1) × 2 + 2, kept though mod 2 cancels.
        assert_eq!(amount, 8.0);
        assert!(canceled);
        let chained: Vec<ModEntry> = [1, 3]
            .map(|id| {
                ModEntry::new(
                    "mod-12",
                    Box::new(Mod {
                        id,
                        calls: calls.clone(),
                    }),
                )
            })
            .into();
        let chained: Arc<[ModEntry]> = chained.into();
        install(chained.clone());
        let mut amount = 1.0;
        let mut canceled = false;
        dispatch_living_incoming_damage(0, "kind", "damage", 0, 0, &mut amount, &mut canceled);
        assert_eq!(amount, 9.0);
        assert!(!canceled);
        install(mods.clone());
        calls.lock().unwrap().clear();
        let mut claimed = false;
        dispatch_use_on(
            &mut host,
            &ItemStack::default(),
            [0; 3],
            [0; 3],
            Hand,
            BlockHit,
            0,
            0,
            &mut claimed,
        );
        assert!(claimed);
        assert_eq!(*calls.lock().unwrap(), [1]);
        calls.lock().unwrap().clear();
        dispatch_use_on(
            &mut host,
            &ItemStack::default(),
            [0; 3],
            [0; 3],
            Hand,
            BlockHit,
            0,
            0,
            &mut claimed,
        );
        assert!(calls.lock().unwrap().is_empty());
    }
}
