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
        pub(super) type NativeDispensed = (Option<super::ItemStack>, i32);
        pub(super) struct NativeLootDrops<'a>(pub &'a ());
        pub(super) struct NativeScreenView<'a>(pub &'a ());
        pub(super) struct StackedOnOther<'a> {
            pub(super) _lifetime: std::marker::PhantomData<&'a mut ()>,
        }
        pub(super) struct Corners;
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
        pub(super) struct ItemEnergyBuffer;
        pub(super) struct Suggestion;
        pub(super) struct HudBatch;
        pub(super) struct BiomeColors;
        pub(super) struct UseAnimation;
        #[derive(Clone, Copy)]
        pub(super) struct Hand;
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
        pub(super) struct NativeLifecycleContext;
        pub(super) struct NativeServerPlayer;
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
        mod horizon_client {
            pub mod camera {
                pub struct CameraEntity;
                pub struct FovPlayer;
            }
            pub mod menu {
                pub struct AnvilCraft;
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
    thread_local! {
        /// The hooks each dispatch entered, a mod at a time.
        static ENTERED: std::cell::RefCell<Vec<(usize, &'static str)>> = const { std::cell::RefCell::new(Vec::new()) };
    }
    fn entered<'a>(
        mods: &'a [Box<dyn Hooks>],
        hook: &'static str,
    ) -> impl Iterator<Item = &'a Box<dyn Hooks>> {
        mods.iter().enumerate().map(move |(index, hooks)| {
            ENTERED.with(|entered| entered.borrow_mut().push((index, hook)));
            hooks
        })
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
    struct Mod {
        id: u32,
        calls: Arc<Mutex<Vec<u32>>>,
    }
    impl Hooks for Mod {
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
        fn use_item_on(&self, _: &mut dyn Host, _: &mut UseItemOnEvent<'_>) -> Flow {
            self.calls.lock().unwrap().push(self.id);
            if self.id == 1 {
                Flow::Cancel
            } else {
                Flow::Continue
            }
        }
    }
    #[test]
    fn block_entity_submit_dispatches_in_order_with_shared_draw() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mods: Vec<Box<dyn Hooks>> = vec![
            Box::new(Mod {
                id: 1,
                calls: calls.clone(),
            }),
            Box::new(Mod { id: 2, calls }),
        ];
        let mut draw = BlockEntityDraw(Vec::new());
        dispatch_block_entity_submit(
            &TestHost,
            &mods,
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
        let mods: Vec<Box<dyn Hooks>> = vec![Box::new(Mod {
            id: 1,
            calls: Arc::new(Mutex::new(Vec::new())),
        })];
        let (mut may_place, mut max_stack) = (true, 64);
        dispatch_menu_slot_rules(
            &mods,
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
            &mods,
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
        let mods: Vec<Box<dyn Hooks>> = vec![Box::new(Mod {
            id: 1,
            calls: Arc::new(Mutex::new(Vec::new())),
        })];
        let mut result = Some(ItemStack::default());
        dispatch_living_entity_use_item_finish(
            &mut TestHost,
            &mods,
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
        let mods: Vec<Box<dyn Hooks>> = vec![
            Box::new(Mod {
                id: 1,
                calls: calls.clone(),
            }),
            Box::new(Mod {
                id: 2,
                calls: calls.clone(),
            }),
        ];
        let mut canceled = false;
        dispatch_mouse_button_pre(&mut TestHost, &mods, 0, true, 0, &mut canceled);
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
        let mods: Vec<Box<dyn Hooks>> = vec![
            Box::new(Refuse(calls.clone(), 1)),
            Box::new(Refuse(calls.clone(), 2)),
        ];
        let mut answer = None;
        dispatch_state_for_placement(
            &Level,
            &mods,
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
            &mods,
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
        let mods: Vec<Box<dyn Hooks>> = vec![Box::new(Property)];
        let host = TestHost;
        let mut value = false;
        dispatch_item_model_property(
            &host,
            &mods,
            "example:charged",
            &StackView { count: 2 },
            &mut value,
        );
        assert!(value);
        dispatch_item_model_property(
            &host,
            &mods,
            "other:property",
            &StackView { count: 1 },
            &mut value,
        );
        assert!(value);
        dispatch_item_model_property(
            &host,
            &mods,
            "example:charged",
            &StackView { count: 1 },
            &mut value,
        );
        assert!(!value);
        dispatch_item_model_property(
            &host,
            &[],
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
            fn living_incoming_damage(&self, event: &mut LivingIncomingDamageEvent<'_>) -> Flow {
                assert_eq!((event.attacker, event.causing), (7, 0));
                let (from, at, look) = (
                    event.source_position.unwrap(),
                    event.victim_position.unwrap(),
                    event.victim_view.unwrap(),
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
        let mods: Vec<Box<dyn Hooks>> = vec![Box::new(Backstab)];
        let held = ItemStack::default();
        for (source_z, weapon, expected) in [
            (-1.0, Some(&held), 12.0),
            (1.0, Some(&held), 4.0),
            (-1.0, None, 4.0),
        ] {
            let (mut amount, mut canceled) = (4.0, false);
            dispatch_living_incoming_damage(
                &mods,
                1,
                "minecraft:zombie",
                "minecraft:player_attack",
                0,
                0,
                7,
                weapon,
                Some([0.0, 0.0, source_z]),
                Some([0.0; 3]),
                Some([0.0, 0.0, 1.0]),
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
            fn living_knockback(&self, event: &mut LivingKnockbackEvent<'_>) -> Flow {
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
        let weapon_mods: Vec<Box<dyn Hooks>> = vec![Box::new(Weapon)];
        let (mut strength, mut x, mut z, mut canceled) = (0.4, 1.0, 0.0, false);
        dispatch_living_knockback(
            &weapon_mods,
            12,
            "minecraft:armor_stand",
            "minecraft:player_attack",
            0,
            0,
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
        let mods: Vec<Box<dyn Hooks>> = (1..=2)
            .map(|id| {
                Box::new(Mod {
                    id,
                    calls: calls.clone(),
                }) as Box<dyn Hooks>
            })
            .collect();
        let mut table = serde_json::json!([]);
        dispatch_loot_table_load(&mods, "table", &mut table);
        assert_eq!(table, serde_json::json!([1, 2]));
        let mut components = Map::new();
        dispatch_modify_default_components(&mods, "item", &mut components);
        assert_eq!(components["order"], serde_json::json!([1, 2]));
        let mut host = TestHost;
        dispatch_level_tick_pre(&mut host, &mods, "overworld");
        assert_eq!(*calls.lock().unwrap(), [1, 2]);
        calls.lock().unwrap().clear();
        let mut zoom = None;
        dispatch_zoom(&host, &mods, &mut zoom);
        assert_eq!(zoom, Some(NativeZoom(1)));
        assert_eq!(*calls.lock().unwrap(), [1]);
        calls.lock().unwrap().clear();
        dispatch_zoom(&host, &mods, &mut zoom);
        assert!(calls.lock().unwrap().is_empty());
        let mut lines = Vec::new();
        dispatch_tooltip(&host, &mods, &ItemStack::default(), &mut lines);
        assert_eq!(lines.iter().map(|line| line.1).collect::<Vec<_>>(), [1, 2]);
        let mut apply = false;
        calls.lock().unwrap().clear();
        dispatch_should_apply_effect_tick_this_tick(&host, &mods, "effect", 0, 0, &mut apply);
        assert!(!apply);
        assert_eq!(*calls.lock().unwrap(), [1, 2]);
        let mut amount = 1.0;
        let mut canceled = false;
        dispatch_living_incoming_damage(
            &mods,
            0,
            "kind",
            "damage",
            0,
            0,
            0,
            None,
            None,
            None,
            None,
            &mut amount,
            &mut canceled,
        );
        // Each mod sees the one before: (1 × 2 + 1) × 2 + 2, kept though mod 2 cancels.
        assert_eq!(amount, 8.0);
        assert!(canceled);
        let chained: Vec<Box<dyn Hooks>> = [1, 3]
            .map(|id| {
                Box::new(Mod {
                    id,
                    calls: calls.clone(),
                }) as Box<dyn Hooks>
            })
            .into();
        let mut amount = 1.0;
        let mut canceled = false;
        dispatch_living_incoming_damage(
            &chained,
            0,
            "kind",
            "damage",
            0,
            0,
            0,
            None,
            None,
            None,
            None,
            &mut amount,
            &mut canceled,
        );
        assert_eq!(amount, 9.0);
        assert!(!canceled);
        calls.lock().unwrap().clear();
        let mut claimed = false;
        dispatch_use_item_on(
            &mut host,
            &mods,
            &ItemStack::default(),
            [0; 3],
            [0; 3],
            &mut claimed,
        );
        assert!(claimed);
        assert_eq!(*calls.lock().unwrap(), [1]);
        calls.lock().unwrap().clear();
        dispatch_use_item_on(
            &mut host,
            &mods,
            &ItemStack::default(),
            [0; 3],
            [0; 3],
            &mut claimed,
        );
        assert!(calls.lock().unwrap().is_empty());
    }
}
