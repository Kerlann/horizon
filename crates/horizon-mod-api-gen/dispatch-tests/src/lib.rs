#[cfg(test)]
mod tests {
    use horizon_core::game_mode::GameMode;
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
        pub(super) struct LevelRef;
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
        #[derive(Clone, Copy)]
        pub(super) struct PlayerRef;
        #[derive(Clone, Copy)]
        pub(super) enum SoundSource {}
        #[derive(Clone, Copy)]
        pub(super) struct ChunkRef;
        pub(super) struct NeighbourUpdate;
        #[derive(Clone, Copy)]
        pub(super) struct PathComputationType;
        #[derive(Clone, Copy)]
        pub(super) struct ConnectionKind;
        pub(super) struct GlobalPos;
        pub(super) struct AttributeModifier;
        #[derive(Clone, Copy)]
        pub(super) enum AttributeOperation {}
        #[derive(Clone, Copy)]
        pub(super) enum ModifierLifetime {}
        #[derive(Clone, Copy)]
        pub(super) enum EquipmentSlotGroup {}
        pub(super) struct ItemAttributeModifier;
        pub(super) struct ItemAttributeModifiers;
        pub(super) struct ItemEnchantments;
        pub(super) struct Identifier;
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
        pub(super) struct NativeSpawnReason;
        pub(super) struct NativeSpawnCheck;
        pub(super) struct NativeDespawn;
        pub(super) struct NativeDifficulty;
        pub(super) struct NativeSpawner;
        pub(super) enum Placement {
            Single,
        }
        pub(super) struct LandingWrite;
        pub(super) struct Tag;
        pub(super) type NativeDispensed = (Option<super::ItemStack>, i32);
        pub(super) struct NativeLootDrops<'a>(pub &'a ());
        pub(super) struct GameRuleValue;
        pub(super) struct FeatureFlags(pub ());
        pub(super) struct GameRuleActor<'a>(pub(super) &'a str);
        pub(super) struct NativePortalShape;
        pub(super) struct NativeNoteInstrument;
        pub(super) struct NativeNoteBlockChange;
        pub(super) struct NativeHandRender<'a>(pub &'a ());
        pub(super) struct NativeExplosionStart<'a>(pub &'a ());
        pub(super) struct NativeItemFished<'a>(pub &'a ());
        pub(super) struct NativeScreenView<'a>(pub &'a ());
        pub(super) struct NativeActiveEffect;
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub(super) struct EffectKey(pub(super) u32);
        #[derive(Debug, PartialEq)]
        pub(super) enum ScreenOpening {
            Cancel,
            ModScreen,
        }
        #[derive(Debug, PartialEq)]
        pub(super) struct Answered<T> {
            pub(super) answer: T,
            pub(super) mod_id: &'static str,
        }
        pub(super) struct StackedOnOther<'a> {
            pub(super) _lifetime: std::marker::PhantomData<&'a mut ()>,
        }
        pub(super) struct ClientPacket;
        pub(super) struct Corners;
        pub(super) struct Suggestion;
        #[derive(Default)]
        pub(super) struct HudBatch(pub Vec<u32>);
        pub(super) struct TooltipElement;
        #[derive(Clone, Copy)]
        pub(super) struct TooltipPlace;
        pub(super) struct BiomeColors;
        pub(super) struct SpawnEntry;
        pub(super) struct UseAnimation;
        #[derive(Clone, Copy)]
        pub(super) struct Hand;
        #[derive(Clone, Copy)]
        pub(super) struct BlockHit;
        pub(super) struct NativeBlockClick<'a>(pub &'a ());
        pub(super) struct NativeItemClick<'a>(pub &'a ());
        #[derive(Clone, Copy)]
        pub(super) enum InteractionKey {
            Attack,
            Use,
            PickBlock,
        }
        #[derive(Clone, Copy)]
        pub(super) enum UsePhase {
            ItemBeforeBlock,
            Block,
            ItemAfterBlock,
        }
        #[derive(Clone, Copy)]
        pub(super) enum BonemealSource {
            Interaction,
        }
        pub(super) struct TooltipDisplay<'a>(pub &'a ());
        pub(super) struct NativeTriState;
        pub(super) struct NativeInteractionResult;
        pub(super) struct NativeMobRender<'a>(pub &'a ());
        pub(super) struct LayerDefinition;
        pub(super) struct DebugEntry;
        pub(super) struct ModelPart;
        pub(super) struct NativeShake;
        pub(super) struct ShieldBlock;
        pub(super) struct HurtSource<'a> {
            pub(super) damage_type: &'a str,
            pub(super) position: Option<[f64; 3]>,
        }
        pub(super) struct Victim {
            pub(super) position: [f64; 3],
            pub(super) view: [f64; 3],
        }
        pub(super) struct DamageContainer(pub(super) f32);
        #[derive(Clone, Copy)]
        pub(super) struct PhantomSpawnResult;
        #[derive(Clone, Copy)]
        pub(super) struct SleepProblem;
        pub(super) struct Music;
        pub(super) struct MusicLayer;
        pub(super) struct NativeBlockRim;
        pub(super) struct NativeColoredLight;
        pub(super) struct NativeMinimap;
        pub(super) struct NativePanelEvent;
        pub(super) struct NativeScreen;
        #[derive(Default)]
        pub(super) struct ModBlockEntity {
            id: String,
            pub(super) data: Vec<i32>,
        }
        impl ModBlockEntity {
            pub(super) fn new(id: &str) -> Self {
                Self {
                    id: id.into(),
                    ..Default::default()
                }
            }
            pub(super) fn id(&self) -> &str {
                &self.id
            }
        }
        pub(super) struct ModBlockEntityView<'a> {
            id: &'a str,
            pub(super) data: &'a mut Vec<i32>,
        }
        impl<'a> ModBlockEntityView<'a> {
            pub(super) fn new(entity: &'a mut ModBlockEntity) -> Self {
                Self {
                    id: &entity.id,
                    data: &mut entity.data,
                }
            }
            pub(super) fn id(&self) -> &str {
                self.id
            }
        }
        pub(super) struct ModContainer;
        pub(super) struct MerchantOffer;
        pub(super) struct ModFluidTanks;
        pub(super) struct ModEnergyBuffer;
        pub(super) struct LootStack;
        pub(super) struct NativeLootContext<'a>(pub &'a ());
        pub(super) struct NativeDataMapType;
        pub(super) struct NativeConfig;
        pub(super) struct NativeConfigFile;
        pub(super) struct NativeReloadListener;
        pub(super) struct RecipeBookCategory;
        pub(super) struct RecipePropertyAddition;
        pub(super) struct NativeMapDecoration;
        pub(super) struct NativeMapInfo<'a>(pub &'a ());
        pub(super) struct NativePlayerRender<'a>(pub &'a ());
        pub(super) struct NativeCommand;
        pub(super) struct AdvancementProgress {
            pub done: bool,
        }
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
        pub(super) struct RegistrationContext(pub &'static str);
        pub(super) trait DataLoadContext {
            fn origin(&self) -> &'static str;
        }
        pub(super) struct Load(pub &'static str);
        impl DataLoadContext for Load {
            fn origin(&self) -> &'static str {
                self.0
            }
        }
        pub(super) trait LoopContext {
            fn side(&self) -> &'static str;
        }
        pub(super) struct Loop(pub &'static str);
        impl LoopContext for Loop {
            fn side(&self) -> &'static str {
                self.0
            }
        }
        pub(super) struct SoundPlay;
        pub(super) struct ChannelRef;
        pub(super) struct ToolUse;
        pub(super) struct NativeServerPlayer;
        pub(super) struct CommandInvocation {
            pub(super) amount: i32,
        }
        #[derive(Clone, Copy)]
        pub(super) struct NativePlayerFood;
        #[derive(Clone, Copy)]
        pub(super) struct LivingRef;
        pub(super) struct MobRef;
        #[derive(Clone, Copy)]
        pub(super) struct EntityRef;
        #[derive(Clone, Copy)]
        pub(super) struct TicketController;
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
        pub(super) trait ServerReloadListeners {}
        pub(super) trait ClientReloadListeners {}
    }
    use serde_json::{Map, Value};
    use stubs::*;
    struct Text;
    struct ScreenshotImage;
    #[derive(Debug, PartialEq)]
    struct NativeZoom(u32);
    enum Flow {
        Continue,
        Cancel,
    }
    trait Host {}
    trait ServerHost: Host {}
    trait ClientHost: Host {}
    trait WorldDrawContext {}
    trait DrawContext {
        fn mark(&mut self, value: u32);
    }
    impl DrawContext for HudBatch {
        fn mark(&mut self, value: u32) {
            self.0.push(value);
        }
    }
    /// Records which part drew, in order.
    trait BlockEntityDrawContext {
        fn mark(&mut self, part: u32);
    }
    impl BlockEntityDrawContext for BlockEntityDraw {
        fn mark(&mut self, part: u32) {
            self.0.push(part);
        }
    }
    enum HostSide<'a> {
        Server(&'a mut dyn ServerHost),
        Client(&'a mut dyn ClientHost),
    }
    impl HostSide<'_> {
        fn reborrow(&mut self) -> HostSide<'_> {
            match self {
                Self::Server(host) => HostSide::Server(&mut **host),
                Self::Client(host) => HostSide::Client(&mut **host),
            }
        }
    }
    enum LevelSide<'a> {
        Server(&'a dyn LevelHost),
        Client(&'a dyn LevelHost),
    }
    impl LevelSide<'_> {
        fn reborrow(&mut self) -> LevelSide<'_> {
            match self {
                Self::Server(host) => LevelSide::Server(*host),
                Self::Client(host) => LevelSide::Client(*host),
            }
        }
    }
    trait RenderHooks {
        fn subscribed(&self) -> &'static [Option<mod_api::RenderHookId>];
        #[allow(dead_code)]
        fn mob_render(&self, _: &dyn Host, _: &mut NativeMobRender<'_>) {}
        #[allow(dead_code)]
        fn block_entity_draw(&self, _: &dyn Host, _: &mut BlockEntityDraw) {}
    }
    struct TestHost;
    struct TestLevel;
    impl LevelHost for TestLevel {}
    impl LevelRandomHost for TestLevel {}
    impl Host for TestHost {}
    impl ServerHost for TestHost {}
    impl ClientHost for TestHost {}
    #[allow(dead_code)]
    mod generated {
        mod horizon_mods {
            pub mod content {
                pub struct Block;
                pub struct Item;
            }
        }
        pub(super) mod mod_api {
            pub mod camera {
                pub struct CameraEntity;
                pub struct FovPlayer;
            }
            pub struct NativeClientInfo;
            pub struct WorldRenderView;
            #[derive(Clone, Copy)]
            pub struct FluidKey;
            #[derive(Clone, Copy)]
            #[non_exhaustive]
            pub struct FluidState {
                pub fluid: FluidKey,
                pub amount: u8,
                pub falling: bool,
                pub source: bool,
            }
            #[derive(Clone, Copy)]
            pub struct RaidRef;
            #[derive(Clone, Copy, Debug, PartialEq, Eq)]
            pub enum CameraType {}
            #[derive(Clone, Copy, Debug, PartialEq, Eq)]
            pub enum ToastKind {
                Advancement,
            }
            pub struct CustomSpawners;
            #[derive(Clone)]
            pub struct SpawnerKey;
            #[derive(Clone, Copy)]
            pub enum KeyAction {}
            pub struct NativeArmorHurt;
            #[derive(Clone, Copy)]
            pub struct ArmPose;
            #[derive(Clone, Copy)]
            pub enum BlockScreenEffect {}
            #[derive(Clone, Copy, Debug, PartialEq, Eq)]
            pub enum EffectApplicable {
                Allow,
                Deny,
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
            pub mod mod_chunk_tickets {
                pub trait TicketValidation {}
            }
            pub use mod_chunk_tickets::TicketValidation;
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
                #[derive(Clone, Copy, Debug, PartialEq, Eq)]
                pub enum HeartKind {}
            }
        }
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum RenderHookId {
            MobRender,
            BlockEntityDraw,
        }
        #[allow(non_upper_case_globals)]
        pub mod render_hook_ids {
            use super::RenderHookId;
            pub const mob_render: Option<RenderHookId> = Some(RenderHookId::MobRender);
            pub const block_entity_draw: Option<RenderHookId> = Some(RenderHookId::BlockEntityDraw);
        }
        include!(concat!(env!("OUT_DIR"), "/mod_api_generated.rs"));
        pub use horizon_mod_api_macros::__subscriptions;
    }
    use generated as mod_api;
    use generated::*;
    struct Installed {
        mods: Arc<[ModEntry]>,
        subscribers: horizon_client_core::subscribers::Subscribers,
    }
    thread_local! {
        /// The hooks each dispatch entered, a mod at a time.
        static ENTERED: std::cell::RefCell<Vec<(usize, &'static str)>> = const { std::cell::RefCell::new(Vec::new()) };
        static MODS: std::cell::Cell<Option<&'static Installed>> = const { std::cell::Cell::new(None) };
    }
    fn install(mods: Arc<[ModEntry]>) {
        let subscriptions = mods.iter().enumerate().flat_map(|(index, entry)| {
            entry
                .hooks
                .subscribed()
                .iter()
                .flatten()
                .map(move |hook| (index, *hook as usize))
        });
        let subscribers = horizon_client_core::subscribers::Subscribers::new(
            HOOK_COUNT,
            mods.len(),
            subscriptions,
        );
        // Test dispatch needs the same static lifetime as the real installation.
        MODS.set(Some(Box::leak(Box::new(Installed { mods, subscribers }))));
    }
    fn listened(hook: HookId) -> bool {
        MODS.get()
            .is_some_and(|installed| installed.subscribers.contains_any(hook as usize))
    }
    fn entered(hook_id: HookId, hook: &'static str) -> impl Iterator<Item = &'static dyn Hooks> {
        entered_with_id(hook_id, hook).map(|(_, hooks)| hooks)
    }
    fn entered_with_id(
        hook_id: HookId,
        hook: &'static str,
    ) -> impl Iterator<Item = (&'static str, &'static dyn Hooks)> {
        MODS.get().into_iter().flat_map(move |installed| {
            installed
                .subscribers
                .indices(hook_id as usize)
                .map(move |index| {
                    let entry = &installed.mods[index];
                    ENTERED.with(|entered| entered.borrow_mut().push((index, hook)));
                    (entry.id, &*entry.hooks)
                })
        })
    }
    struct ModEntry {
        id: &'static str,
        hooks: Box<dyn Hooks>,
    }
    impl ModEntry {
        fn new(id: &'static str, hooks: Box<dyn Hooks>) -> Self {
            Self { id, hooks }
        }
    }
    trait Registrar {}
    /// The goal registrar of the generated `EntityPart`, as the fixture sees
    /// it: no methods, its type is all the generated trait asks of the scope.
    #[expect(dead_code)]
    struct GoalRegistrar<'a> {
        _lifetime: std::marker::PhantomData<&'a ()>,
    }
    /// The parts every thing in a scope has, whatever its key: a test's own, or none.
    #[derive(Default)]
    struct Parts {
        blocks: Vec<&'static dyn BlockPart>,
        block_entities: Vec<&'static dyn BlockEntityPart>,
        items: Vec<&'static dyn ItemPart>,
        menus: Vec<&'static dyn MenuPart>,
    }
    static NO_PARTS: Parts = Parts {
        blocks: Vec::new(),
        block_entities: Vec::new(),
        items: Vec::new(),
        menus: Vec::new(),
    };
    thread_local! {
        static PARTS: std::cell::Cell<&'static Parts> = const { std::cell::Cell::new(&NO_PARTS) };
    }
    fn install_parts(parts: Parts) {
        PARTS.set(Box::leak(Box::new(parts)));
    }
    /// `part` for the parts list of a test: parts live as long as the installation.
    fn leak<T>(part: T) -> &'static T {
        Box::leak(Box::new(part))
    }
    fn has_part_hook(_: HookId) -> bool {
        true
    }
    fn parts(_: &'static str) -> &'static Parts {
        PARTS.get()
    }
    impl Parts {
        fn for_hook(&self, _: HookId) -> Option<&Self> {
            Some(self)
        }
        fn block_state(&self, _: BlockStateId) -> &[&'static dyn BlockPart] {
            &self.blocks
        }
        fn block_entity<'k>(
            &self,
            _: impl FnOnce() -> Option<&'k str>,
        ) -> &[&'static dyn BlockEntityPart] {
            &self.block_entities
        }
        fn block_entity_kind<'k>(
            &self,
            _: impl FnOnce() -> Option<&'k str>,
        ) -> &[&'static dyn BlockEntityPart] {
            &self.block_entities
        }
        fn item<'k>(&self, _: impl FnOnce() -> Option<&'k str>) -> &[&'static dyn ItemPart] {
            &self.items
        }
        fn menu<'k>(&self, _: impl FnOnce() -> Option<&'k str>) -> &[&'static dyn MenuPart] {
            &self.menus
        }
    }
    fn entered_parts<'a, P: ?Sized>(
        parts: &'a [&'a P],
        _: &'static str,
    ) -> impl Iterator<Item = &'a P> {
        parts.iter().copied()
    }
    #[test]
    fn registration_context_reaches_collectors_in_order_without_a_game_host() {
        struct Registration(&'static str);
        #[mod_api::__subscriptions]
        impl Hooks for Registration {
            fn component_types(
                &self,
                context: &RegistrationContext,
                event: &mut ComponentTypesEvent<'_>,
            ) {
                assert_eq!(context.0, "bootstrap");
                event.types.push(self.0.into());
            }
        }
        install(
            vec![
                ModEntry::new("first", Box::new(Registration("first:component"))),
                ModEntry::new("second", Box::new(Registration("second:component"))),
            ]
            .into(),
        );
        let mut types = Vec::new();
        dispatch_component_types(&RegistrationContext("bootstrap"), &mut types);
        assert_eq!(types, ["first:component", "second:component"]);
    }

    #[test]
    fn cfg_disabled_hooks_are_not_subscribed_and_cfg_attr_stays() {
        struct Gated;
        #[mod_api::__subscriptions]
        impl Hooks for Gated {
            #[cfg(any())]
            fn loot_table_load(&self, _: &dyn DataLoadContext, _: &mut LootTableLoadEvent<'_>) {
                unreachable!()
            }
            #[cfg(not(any()))]
            fn player_logged_in(&self, _: &mut dyn ServerHost, _: &mut PlayerLoggedInEvent<'_>) {}
            #[cfg_attr(test, inline)]
            fn player_logged_out(&self, _: &mut dyn ServerHost, _: &mut PlayerLoggedOutEvent<'_>) {}
        }
        install(vec![ModEntry::new("gated", Box::new(Gated))].into());
        assert!(
            !listened(HookId::LootTableLoad),
            "a cfg-disabled hook was subscribed"
        );
        assert!(
            listened(HookId::PlayerLoggedIn),
            "a cfg-enabled hook was not subscribed"
        );
        assert!(
            listened(HookId::PlayerLoggedOut),
            "cfg_attr never unsubscribes"
        );
    }

    #[test]
    fn cfg_disabled_render_hooks_are_not_subscribed_and_cfg_attr_stays() {
        struct Renderer;
        #[mod_api::__subscriptions]
        impl RenderHooks for Renderer {
            #[cfg(any())]
            fn mob_render(&self, _: &dyn Host, _: &mut NativeMobRender<'_>) {}
            #[cfg_attr(test, inline)]
            fn block_entity_draw(&self, _: &dyn Host, _: &mut BlockEntityDraw) {}
        }
        assert_eq!(
            Renderer.subscribed(),
            &[mod_api::render_hook_ids::block_entity_draw]
        );
    }

    #[test]
    fn only_overrides_are_dispatched_from_the_first_call_across_words() {
        struct Quiet;
        #[mod_api::__subscriptions]
        impl Hooks for Quiet {}
        struct Listener(Arc<std::sync::atomic::AtomicUsize>);
        #[mod_api::__subscriptions]
        impl Hooks for Listener {
            fn loot_table_load(&self, _: &dyn DataLoadContext, _: &mut LootTableLoadEvent<'_>) {
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
        dispatch_loot_table_load(&Load("defaults"), "test", &mut table);
        dispatch_loot_table_load(&Load("defaults"), "test", &mut table);
        assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 2);
        ENTERED.with(|entered| {
            let entered = entered.borrow();
            assert_eq!(
                entered.as_slice(),
                [(66, "loot_table_load"), (66, "loot_table_load")]
            );
        });
    }

    #[test]
    fn nested_default_does_not_affect_installed_subscribers() {
        struct Other;
        #[mod_api::__subscriptions]
        impl Hooks for Other {}
        struct Active;
        #[mod_api::__subscriptions]
        impl Hooks for Active {
            fn loot_table_load(&self, _: &dyn DataLoadContext, event: &mut LootTableLoadEvent<'_>) {
                dispatch_modify_default_components(&Load("defaults"), "test", &mut Map::new());
                Other.loot_table_load(&Load("defaults"), event);
            }
        }
        install(vec![ModEntry::new("active", Box::new(Active))].into());
        ENTERED.with(|entered| entered.borrow_mut().clear());
        let mut table = serde_json::json!([]);
        dispatch_loot_table_load(&Load("defaults"), "test", &mut table);
        dispatch_loot_table_load(&Load("defaults"), "test", &mut table);
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
    fn toast_add_passes_kind_and_text_edits_and_stops_on_cancel() {
        struct Filter(bool);
        #[mod_api::__subscriptions]
        impl Hooks for Filter {
            fn toast_add(&self, _: &dyn ClientHost, event: &mut ToastAddEvent<'_>) -> Flow {
                assert_eq!(event.kind, generated::mod_api::ToastKind::Advancement);
                if self.0 {
                    assert!(event.description.is_some());
                    *event.description = None;
                    Flow::Cancel
                } else {
                    assert!(event.description.is_none());
                    *event.description = Some(Text);
                    Flow::Continue
                }
            }
        }
        install(
            vec![
                ModEntry::new("editor", Box::new(Filter(false))),
                ModEntry::new("cancel", Box::new(Filter(true))),
                ModEntry::new("late", Box::new(Filter(false))),
            ]
            .into(),
        );
        ENTERED.with(|entered| entered.borrow_mut().clear());
        let mut description = None;
        let mut cancel = false;
        dispatch_toast_add(
            &TestHost,
            generated::mod_api::ToastKind::Advancement,
            &mut Text,
            &mut description,
            &mut cancel,
        );
        assert!(cancel);
        assert!(description.is_none());
        ENTERED.with(|entered| {
            assert_eq!(
                entered.borrow().as_slice(),
                [(0, "toast_add"), (1, "toast_add")]
            );
        });
    }

    #[test]
    fn command_dispatch_forwards_typed_input_to_player_and_console_listeners() {
        struct Observer(Arc<Mutex<Vec<i32>>>);
        #[mod_api::__subscriptions]
        impl Hooks for Observer {
            fn perform_command(
                &self,
                _: &mut dyn ServerHost,
                event: &mut PerformCommandEvent<'_>,
            ) -> Flow {
                self.0.lock().unwrap().push(event.command.amount);
                Flow::Cancel
            }
            fn perform_console_command(
                &self,
                _: &mut dyn ServerHost,
                event: &mut PerformConsoleCommandEvent<'_>,
            ) -> Flow {
                self.0.lock().unwrap().push(-event.command.amount);
                Flow::Cancel
            }
        }
        let calls = Arc::new(Mutex::new(Vec::new()));
        install(vec![ModEntry::new("commands", Box::new(Observer(calls.clone())))].into());
        let command = CommandInvocation { amount: 37 };
        let mut ran = false;
        dispatch_perform_command(&mut TestHost, &NativeServerPlayer, &command, &mut ran);
        assert!(ran);
        ran = false;
        dispatch_perform_console_command(&mut TestHost, &command, &mut ran);
        assert!(ran);
        assert_eq!(*calls.lock().unwrap(), vec![37, -37]);
    }

    #[test]
    fn advancement_progress_reports_grant_and_revoke_identity() {
        struct Observer(Arc<Mutex<Vec<NativeProgressType>>>);
        #[mod_api::__subscriptions]
        impl Hooks for Observer {
            fn player_advancement_progress(
                &self,
                _: &mut dyn ServerHost,
                event: &mut PlayerAdvancementProgressEvent<'_>,
            ) {
                assert_eq!(event.player, 123);
                assert_eq!(event.player_entity_id, 42);
                assert_eq!(event.name, "Alex");
                assert_eq!(event.advancement, "example:advancement");
                assert_eq!(event.criterion, "criterion");
                assert_eq!(
                    event.progress.done,
                    event.progress_type == NativeProgressType::Grant
                );
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
                calls::PlayerAdvancementProgressCall {
                    player: 123,
                    player_entity_id: 42,
                    name: "Alex",
                    advancement: "example:advancement",
                    criterion: "criterion",
                    progress_type,
                    progress: &AdvancementProgress {
                        done: progress_type == NativeProgressType::Grant,
                    },
                },
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
    impl BlockPart for Mod {
        fn block_subscribed(&self) -> &'static [Option<HookId>] {
            &[]
        }
    }
    impl BlockEntityPart for Mod {
        fn block_entity_subscribed(&self) -> &'static [Option<HookId>] {
            &[]
        }
        fn menu_button(&self, _: &mut dyn ServerHost, event: &mut MenuButtonEvent<'_>) {
            assert_eq!(event.context.player, 123);
            assert_eq!(event.context.player_entity_id, 42);
            assert_eq!(event.context.pos, [1, 2, 3]);
            assert_eq!(event.context.id, 9);
            assert_eq!(event.entity.id(), "machine");
            event.entity.data.push(9);
            *event.carried = Some(ItemStack {
                item: "bucket".into(),
            });
        }
        fn block_entity_submit(&self, _: &dyn ClientHost, event: &mut BlockEntitySubmitEvent<'_>) {
            assert_eq!(event.pos, [1, 2, 3]);
            assert_eq!(event.block, "example:machine");
            event.draw.mark(self.id);
        }
    }
    impl MenuPart for Mod {
        fn menu_subscribed(&self) -> &'static [Option<HookId>] {
            &[]
        }
        fn menu_slot_rules(&self, context: &dyn LoopContext, event: &mut MenuSlotRulesEvent<'_>) {
            assert_eq!(context.side(), "loop");
            event.may_place = false;
            event.max_stack = self.id;
        }
    }
    impl ItemPart for Mod {
        fn item_subscribed(&self) -> &'static [Option<HookId>] {
            &[]
        }
        fn tooltip(&self, _: &dyn ClientHost, event: &mut TooltipEvent<'_>) {
            event.lines.push((Text, self.id));
        }
        fn use_on(&self, _: &mut dyn ServerHost, _: &mut UseOnEvent<'_>) -> Flow {
            self.calls.lock().unwrap().push(self.id);
            if self.id == 1 {
                Flow::Cancel
            } else {
                Flow::Continue
            }
        }
    }
    #[mod_api::__subscriptions]
    impl Hooks for Mod {
        fn living_entity_use_item_finish(
            &self,
            _: &mut dyn ServerHost,
            event: &mut LivingEntityUseItemFinishEvent<'_>,
        ) {
            *event.result = None;
        }
        fn mouse_button_pre(&self, _: &mut dyn ClientHost, _: &mut MouseButtonPreEvent) -> Flow {
            self.calls.lock().unwrap().push(self.id);
            if self.id == 1 {
                Flow::Cancel
            } else {
                Flow::Continue
            }
        }
        fn living_entity_use_item_tick(
            &self,
            _: HostSide,
            event: &mut LivingEntityUseItemTickEvent<'_>,
        ) -> Flow {
            event.duration -= 1;
            if self.id == 1 {
                Flow::Cancel
            } else {
                Flow::Continue
            }
        }
        fn loot_table_load(&self, _: &dyn DataLoadContext, event: &mut LootTableLoadEvent<'_>) {
            event.table.as_array_mut().unwrap().push(self.id.into());
        }
        fn modify_default_components(
            &self,
            context: &dyn DataLoadContext,
            event: &mut ModifyDefaultComponentsEvent<'_>,
        ) {
            assert_eq!(context.origin(), "defaults");
            let value = event
                .components
                .entry("order")
                .or_insert_with(|| serde_json::json!([]));
            value.as_array_mut().unwrap().push(self.id.into());
        }
        fn level_tick_pre(&self, _: &mut dyn ServerHost, _: &mut LevelTickPreEvent<'_>) {
            self.calls.lock().unwrap().push(self.id);
        }
        fn zoom(&self, _: &dyn ClientHost, _: &mut ZoomEvent) -> Option<NativeZoom> {
            self.calls.lock().unwrap().push(self.id);
            Some(NativeZoom(self.id))
        }
        fn should_apply_effect_tick_this_tick(
            &self,
            _: &dyn Host,
            event: &mut ShouldApplyEffectTickThisTickEvent,
        ) {
            assert_eq!(
                (event.effect, event.tick_count, event.amplifier),
                (EffectKey(7), 19, 2)
            );
            event.apply = !event.apply;
            self.calls.lock().unwrap().push(self.id);
        }
        fn living_incoming_damage(
            &self,
            _: &mut dyn LevelRandomHost,
            event: &mut LivingIncomingDamageEvent<'_>,
        ) -> Flow {
            event.container.0 = event.container.0 * 2.0 + self.id as f32;
            if self.id == 2 {
                Flow::Cancel
            } else {
                Flow::Continue
            }
        }
    }
    fn part(id: u32, calls: &Arc<Mutex<Vec<u32>>>) -> &'static Mod {
        leak(Mod {
            id,
            calls: calls.clone(),
        })
    }
    #[test]
    fn menu_button_mutates_entity_and_cursor_once() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        install_parts(Parts {
            block_entities: vec![part(1, &calls)],
            ..Parts::default()
        });
        let context = generated::mod_api::menu::MenuButtonContext {
            player: 123,
            player_entity_id: 42,
            pos: [1, 2, 3],
            id: 9,
        };
        let mut entity = ModBlockEntity::new("machine");
        let mut carried = None;
        dispatch_menu_button(&mut TestHost, &context, &mut entity, &mut carried);
        assert_eq!(entity.id(), "machine");
        assert_eq!(entity.data, [9]);
        assert_eq!(carried.unwrap().item.as_ref(), "bucket");
    }

    #[test]
    fn block_entity_submit_dispatches_in_order_with_shared_draw() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        install_parts(Parts {
            block_entities: vec![part(1, &calls), part(2, &calls)],
            ..Parts::default()
        });
        let mut draw = BlockEntityDraw(Vec::new());
        dispatch_block_entity_submit(
            &TestHost,
            [1, 2, 3],
            "example:machine",
            BlockStateId,
            &ModBlockEntity::default(),
            &mut draw,
        );
        assert_eq!(draw.0, [1, 2]);
    }

    #[test]
    fn foreground_gui_preserves_draws_and_dispatches_with_visibility_flags() {
        struct Indicator(u32);
        #[mod_api::__subscriptions]
        impl Hooks for Indicator {
            fn gui_render_post(&self, _: &dyn ClientHost, event: &mut GuiRenderPostEvent<'_>) {
                // A persistent indicator opts into F1 and hides on focus loss.
                if event.focused {
                    event.batch.mark(self.0 + u32::from(event.gui_hidden) * 10);
                }
            }
        }
        install(
            vec![
                ModEntry::new("first", Box::new(Indicator(1))),
                ModEntry::new("second", Box::new(Indicator(2))),
            ]
            .into(),
        );
        for (hidden, focused, expected) in [
            (false, true, vec![99, 1, 2]),
            (true, true, vec![99, 11, 12]),
            (true, false, vec![99]),
        ] {
            let mut batch = HudBatch(vec![99]);
            dispatch_gui_render_post(&TestHost, &mut batch, hidden, focused);
            assert_eq!(batch.0, expected);
        }
        let mut batch = HudBatch(vec![99]);
        install(Vec::<ModEntry>::new().into());
        dispatch_gui_render_post(&TestHost, &mut batch, true, true);
        assert_eq!(batch.0, [99]);
    }

    #[test]
    fn slot_rules_mutate_and_item_tick_can_cancel() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mods: Vec<ModEntry> = vec![ModEntry::new(
            "mod-2",
            Box::new(Mod {
                id: 1,
                calls: calls.clone(),
            }),
        )];
        let mods: Arc<[ModEntry]> = mods.into();
        install(mods.clone());
        install_parts(Parts {
            menus: vec![part(1, &calls)],
            ..Parts::default()
        });
        let (mut may_place, mut max_stack) = (true, 64);
        dispatch_menu_slot_rules(
            &Loop("loop"),
            "test:menu",
            0,
            &ItemStack::default(),
            &mut may_place,
            &mut max_stack,
        );
        assert_eq!((may_place, max_stack), (false, 1));
        let (mut duration, mut canceled) = (10, false);
        dispatch_living_entity_use_item_tick(
            HostSide::Server(&mut TestHost),
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
        impl BlockPart for Refuse {
            fn block_subscribed(&self) -> &'static [Option<HookId>] {
                &[]
            }
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
        install_parts(Parts {
            blocks: vec![
                leak(Refuse(calls.clone(), 1)),
                leak(Refuse(calls.clone(), 2)),
            ],
            ..Parts::default()
        });
        let mut answer = None;
        dispatch_state_for_placement(
            &Level,
            LevelDimension,
            BlockStateId,
            &PlacementContext,
            generated::mod_api::FluidState {
                fluid: generated::mod_api::FluidKey,
                amount: 0,
                falling: false,
                source: false,
            },
            &mut answer,
        );
        assert_eq!(answer, Some(None));
        assert_eq!(*calls.lock().unwrap(), vec![1]);
        dispatch_state_for_placement(
            &Level,
            LevelDimension,
            BlockStateId,
            &PlacementContext,
            generated::mod_api::FluidState {
                fluid: generated::mod_api::FluidKey,
                amount: 0,
                falling: false,
                source: false,
            },
            &mut answer,
        );
        assert_eq!(*calls.lock().unwrap(), vec![1]);
    }

    #[test]
    fn item_model_property_dispatches_borrowed_stack_and_preserves_unhandled_answers() {
        struct Property;
        #[mod_api::__subscriptions]
        impl Hooks for Property {
            fn item_model_property(
                &self,
                _: &dyn ClientHost,
                event: &mut ItemModelPropertyEvent<'_>,
            ) {
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
        #[mod_api::__subscriptions]
        impl Hooks for Backstab {
            fn living_incoming_damage(
                &self,
                _: &mut dyn LevelRandomHost,
                event: &mut LivingIncomingDamageEvent<'_>,
            ) -> Flow {
                assert_eq!(event.source.damage_type, "minecraft:player_attack");
                let (from, at, look) = (
                    event.source.position.unwrap(),
                    event.victim.position,
                    event.victim.view,
                );
                let behind = (0..3)
                    .map(|axis| (from[axis] - at[axis]) * look[axis])
                    .sum::<f64>();
                if behind < 0.0 && event.weapon.is_some() {
                    event.container.0 *= 3.0;
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
            let source = HurtSource {
                damage_type: "minecraft:player_attack",
                position: Some([0.0, 0.0, source_z]),
            };
            let victim = Victim {
                position: [0.0; 3],
                view: [0.0, 0.0, 1.0],
            };
            let (mut container, mut canceled) = (DamageContainer(4.0), false);
            dispatch_living_incoming_damage(
                &mut TestLevel,
                calls::LivingIncomingDamageCall {
                    entity: 1,
                    kind: "minecraft:zombie",
                    source: &source,
                    victim: &victim,
                    weapon,
                    container: &mut container,
                    canceled: &mut canceled,
                },
            );
            assert_eq!((container.0, canceled), (expected, false));
        }
    }

    #[test]
    fn an_answer_names_the_mod_that_gave_it() {
        struct Passer;
        #[mod_api::__subscriptions]
        impl Hooks for Passer {
            fn screen_opening(
                &self,
                _: &dyn ClientHost,
                _: &mut ScreenOpeningEvent<'_>,
            ) -> Option<ScreenOpening> {
                None
            }
        }
        struct Opener;
        #[mod_api::__subscriptions]
        impl Hooks for Opener {
            fn screen_opening(
                &self,
                _: &dyn ClientHost,
                _: &mut ScreenOpeningEvent<'_>,
            ) -> Option<ScreenOpening> {
                Some(ScreenOpening::ModScreen)
            }
        }
        let mods: Vec<ModEntry> = vec![
            ModEntry::new("passer", Box::new(Passer)),
            ModEntry::new("opener", Box::new(Opener)),
            ModEntry::new("late", Box::new(Opener)),
        ];
        install(mods.into());
        let view = NativeScreenView(&());
        let mut outcome = None;
        dispatch_screen_opening(&TestHost, &view, &view, &mut outcome);
        assert_eq!(
            outcome,
            Some(Answered {
                answer: ScreenOpening::ModScreen,
                mod_id: "opener"
            })
        );
    }

    #[test]
    fn beacon_dispatch_preserves_optional_effect_keys() {
        struct Beacon(Arc<Mutex<Vec<[Option<EffectKey>; 2]>>>);
        #[mod_api::__subscriptions]
        impl Hooks for Beacon {
            fn player_change_beacon_effect(
                &self,
                _: &mut dyn ServerHost,
                event: &mut PlayerChangeBeaconEffectEvent<'_>,
            ) {
                self.0
                    .lock()
                    .unwrap()
                    .push([event.primary, event.secondary]);
            }
        }
        let calls = Arc::new(Mutex::new(Vec::new()));
        install(vec![ModEntry::new("beacon", Box::new(Beacon(calls.clone())))].into());
        dispatch_player_change_beacon_effect(
            &mut TestHost,
            42,
            "player",
            [1, 2, 3],
            Some(EffectKey(7)),
            None,
        );
        dispatch_player_change_beacon_effect(
            &mut TestHost,
            43,
            "other",
            [4, 5, 6],
            None,
            Some(EffectKey(11)),
        );
        assert_eq!(
            *calls.lock().unwrap(),
            [[Some(EffectKey(7)), None], [None, Some(EffectKey(11))]],
        );
    }

    #[test]
    fn dispatch_contracts() {
        struct Weapon;
        #[mod_api::__subscriptions]
        impl Hooks for Weapon {
            fn living_knockback(
                &self,
                _: &mut dyn LevelRandomHost,
                event: &mut LivingKnockbackEvent<'_>,
            ) -> Flow {
                assert_eq!(
                    (event.entity, event.kind, event.attacker),
                    (12, "minecraft:armor_stand", 7)
                );
                assert_eq!(event.source.damage_type, "minecraft:player_attack");
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
        let source = HurtSource {
            damage_type: "minecraft:player_attack",
            position: None,
        };
        dispatch_living_knockback(
            &mut TestLevel,
            calls::LivingKnockbackCall {
                entity: 12,
                kind: "minecraft:armor_stand",
                source: &source,
                attacker: 7,
                attacker_main: None,
                attacker_off: None,
                strength: &mut strength,
                x: &mut x,
                z: &mut z,
                canceled: &mut canceled,
            },
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
        dispatch_loot_table_load(&Load("defaults"), "table", &mut table);
        assert_eq!(table, serde_json::json!([1, 2]));
        let mut components = Map::new();
        dispatch_modify_default_components(&Load("defaults"), "item", &mut components);
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
        install_parts(Parts {
            items: vec![part(1, &calls), part(2, &calls)],
            ..Parts::default()
        });
        let mut lines = Vec::new();
        dispatch_tooltip(&host, &ItemStack::default(), &mut lines);
        assert_eq!(lines.iter().map(|line| line.1).collect::<Vec<_>>(), [1, 2]);
        let mut apply = false;
        calls.lock().unwrap().clear();
        dispatch_should_apply_effect_tick_this_tick(&host, EffectKey(7), 19, 2, &mut apply);
        assert!(!apply);
        assert_eq!(*calls.lock().unwrap(), [1, 2]);
        let source = HurtSource {
            damage_type: "damage",
            position: None,
        };
        let victim = Victim {
            position: [0.0; 3],
            view: [0.0; 3],
        };
        let mut container = DamageContainer(1.0);
        let mut canceled = false;
        dispatch_living_incoming_damage(
            &mut TestLevel,
            calls::LivingIncomingDamageCall {
                entity: 0,
                kind: "kind",
                source: &source,
                victim: &victim,
                weapon: None,
                container: &mut container,
                canceled: &mut canceled,
            },
        );
        // Each mod sees the one before: (1 × 2 + 1) × 2 + 2, kept though mod 2 cancels.
        assert_eq!(container.0, 8.0);
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
        let mut container = DamageContainer(1.0);
        let mut canceled = false;
        dispatch_living_incoming_damage(
            &mut TestLevel,
            calls::LivingIncomingDamageCall {
                entity: 0,
                kind: "kind",
                source: &source,
                victim: &victim,
                weapon: None,
                container: &mut container,
                canceled: &mut canceled,
            },
        );
        assert_eq!(container.0, 9.0);
        assert!(!canceled);
        install(mods.clone());
        calls.lock().unwrap().clear();
        let mut claimed = false;
        dispatch_use_on(
            &mut host,
            calls::UseOnCall {
                stack: &ItemStack::default(),
                pos: [0; 3],
                face: [0; 3],
                hand: Hand,
                hit: BlockHit,
                player: 0,
                player_entity_id: 0,
                claimed: &mut claimed,
            },
        );
        assert!(claimed);
        assert_eq!(*calls.lock().unwrap(), [1]);
        calls.lock().unwrap().clear();
        dispatch_use_on(
            &mut host,
            calls::UseOnCall {
                stack: &ItemStack::default(),
                pos: [0; 3],
                face: [0; 3],
                hand: Hand,
                hit: BlockHit,
                player: 0,
                player_entity_id: 0,
                claimed: &mut claimed,
            },
        );
        assert!(calls.lock().unwrap().is_empty());
    }
}
