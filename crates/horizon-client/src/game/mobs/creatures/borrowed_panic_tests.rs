use super::*;
use horizon_client::{
    chunk::Chunk,
    join_seq::residency::{
        Change, Journal, Kind, Owner,
        table::{EntityLookup, EntityTable, SourceEntity},
    },
};
use horizon_core::random::LegacyRandom;
use horizon_mod_runtime::attachments::{AttachmentType, Holder};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Family {
    Damage,
    Shield,
    Totem,
    Movement,
    Breathing,
    Goal,
}
struct Probe {
    family: Family,
    calls: AtomicUsize,
    armed: AtomicBool,
}
impl Probe {
    fn hear(&self, family: Family, target: bool) {
        if self.family == family && target {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.armed.swap(false, Ordering::SeqCst) {
                eprintln!("BORROWED_NATIVE_REACHED:{family:?}");
                panic!("actual borrowed mob callback panicked");
            }
        }
    }
}
struct NativeGoal(Arc<Probe>);
impl mod_api::Goal for NativeGoal {
    fn name(&self) -> &'static str {
        "BorrowedProbe"
    }
    fn flags(&self) -> mod_api::GoalFlags {
        mod_api::GoalFlags::MOVE
    }
    fn can_use(&mut self, _: &mut mod_api::GoalContext<'_>) -> bool {
        true
    }
    fn tick(&mut self, cx: &mut mod_api::GoalContext<'_>) {
        assert_eq!(
            cx.mob().entity().position(cx.host()),
            Some([2.0, 80.0, 0.5])
        );
        self.0.hear(Family::Goal, cx.mob().entity().id() == 72);
    }
}
struct GoalPart(Arc<Probe>);
#[mod_api::horizon_part]
impl mod_api::EntityPart for GoalPart {
    fn register_goals(&self, goals: &mut mod_api::GoalRegistrar<'_>) {
        goals.add_goal(0, NativeGoal(self.0.clone()));
    }
}
struct Hooks(Arc<Probe>);
#[mod_api::__subscriptions]
impl mod_api::Hooks for Hooks {
    fn register(&self, r: &mut dyn mod_api::Registrar, _: &mod_api::RegistrationContext) {
        if self.0.family == Family::Goal {
            r.entity_part("minecraft:zombie", GoalPart(self.0.clone()));
        }
    }
    fn living_incoming_damage(
        &self,
        _: &mut dyn mod_api::LevelRandomHost,
        event: &mut mod_api::LivingIncomingDamageEvent<'_>,
    ) -> mod_api::Flow {
        self.0.hear(Family::Damage, event.entity == 72);
        mod_api::Flow::Continue
    }
    fn living_shield_block(
        &self,
        _: &mut dyn mod_api::LevelRandomHost,
        event: &mut mod_api::LivingShieldBlockEvent<'_>,
    ) {
        self.0.hear(Family::Shield, event.entity == 72);
    }
    fn living_speed(
        &self,
        _: &dyn mod_api::LoopContext,
        event: &mut mod_api::LivingSpeedEvent<'_>,
    ) {
        self.0.hear(Family::Movement, event.entity == 72);
    }
    fn living_breathe(
        &self,
        _: &dyn mod_api::LoopContext,
        event: &mut mod_api::LivingBreatheEvent<'_>,
    ) {
        self.0.hear(Family::Breathing, event.entity == 72);
    }
    fn living_use_totem(
        &self,
        _: &mut dyn mod_api::LevelRandomHost,
        event: &mut mod_api::LivingUseTotemEvent<'_>,
    ) -> mod_api::Flow {
        self.0.hear(Family::Totem, event.entity == 72);
        mod_api::Flow::Continue
    }
}
const VALUE: AttachmentType<i32> = AttachmentType {
    namespace: "borrowed-mob-test",
    name: "retained",
    default: || 0,
    codec: None,
    copy_on_death: false,
    sync: false,
};
struct Scratch(std::path::PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn run(family: Family) {
    crate::entry::install_client_hooks();
    eprintln!("BORROWED_ASSERTIONS_DISABLED:{}", !cfg!(debug_assertions));
    for dim in [Dim::Overworld, Dim::Nether] {
        eprintln!("BORROWED_CASE:{family:?}:{dim:?}");
        let scratch = Scratch(std::env::temp_dir().join(format!(
            "horizon-borrowed-{}-{:?}-{family:?}-{dim:?}",
            std::process::id(),
            std::thread::current().id()
        )));
        let probe = Arc::new(Probe {
            family,
            calls: AtomicUsize::new(0),
            armed: AtomicBool::new(true),
        });
        mod_api::testing::with_mods(
            vec![mod_api::ModEntry::new(
                "borrowed-panic",
                Box::new(Hooks(probe.clone())),
            )]
            .into(),
            || {
                crate::game::api_values::install_parts(&[mod_api::ModEntry::new(
                    "borrowed-panic",
                    Box::new(Hooks(probe.clone())),
                )])
                .unwrap();
                crate::game::native_entity_refs::tests::with_game(|game| {
                    game.world_root = scratch.0.clone();
                    game.enter_level(dim).unwrap();
                    game.options.dedicated = Some(Vec::new());
                    game.world
                        .chunks
                        .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
                    assert!(game.entity_ticking_area()(1, 0));
                    let journal = Journal::default();
                    let owner = Owner {
                        dimension: game.dim.key(),
                        kind: Kind::Mob,
                    };
                    game.mobs
                        .bind_residency(&journal, owner, |m| (m.id, m.join));
                    for id in [71, 72] {
                        let kind = game
                            .res
                            .mob_types()
                            .get(if id == 71 {
                                "minecraft:pig"
                            } else {
                                "minecraft:zombie"
                            })
                            .unwrap()
                            .clone();
                        let mut mob =
                            Mob::new(id, kind, Vec3::new(f64::from(id - 70), 80.0, 0.5), 0.0);
                        mob.no_ai = true;
                        if id == 72 {
                            mob.air = 100;
                            if matches!(family, Family::Damage | Family::Shield | Family::Totem) {
                                mob.fire_ticks = 20;
                            }
                            if family == Family::Goal {
                                mob.no_ai = false;
                                assert!(mob.ai.goals.has("BorrowedProbe"));
                            }
                            if family == Family::Movement {
                                // NoAI skips aiStep's travel: this must be a
                                // real moving mob, not just a grounded corpse.
                                mob.no_ai = false;
                                mob.on_ground = true;
                                mob.ai.speed = 0.2;
                            }
                            if family == Family::Totem {
                                mob.health = 0.5;
                                mob.extra
                                    .set_main_hand(Some("minecraft:totem_of_undying".into()));
                            }
                        }
                        game.mobs.push(mob);
                    }
                    let rows: Vec<_> = game.mobs.iter().map(|m| (m.id, m.join)).collect();
                    VALUE
                        .set(&mut game.world.attachments, Holder::Level, 42)
                        .unwrap();
                    game.world.attachment_syncs.insert(Holder::Level);
                    let syncs = game.world.attachment_syncs.clone();
                    game.world.path_writes = Some(Vec::new());
                    let stone = game
                        .pack
                        .assets
                        .registry
                        .default_state("minecraft:stone")
                        .unwrap();
                    game.world.set(4, 80, 4, stone).unwrap();
                    if family == Family::Movement {
                        game.world.set(2, 79, 0, stone).unwrap();
                    }
                    let writes = game.world.path_writes.clone();
                    game.world.random = LegacyRandom::new(0x5eed);
                    let draw = game.world.random.clone().next_long();
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        game.tick_creature_entities()
                    }));
                    assert_eq!(
                        probe.calls.load(Ordering::SeqCst),
                        1,
                        "real native listener was not reached once"
                    );
                    let payload = result.expect_err("native callback panic was swallowed");
                    assert_eq!(
                        payload.downcast_ref::<&str>().copied(),
                        Some("actual borrowed mob callback panicked")
                    );
                    let mut pending = Vec::new();
                    journal.drain_into(&mut pending);
                    let (mut depth, mut lifts) = (0usize, 0usize);
                    for e in pending {
                        assert_eq!(e.owner, owner);
                        match e.change {
                            Change::Lifted => {
                                depth += 1;
                                lifts += 1;
                            }
                            Change::Restored => depth = depth.checked_sub(1).unwrap(),
                            _ => {}
                        }
                        journal.push(e);
                    }
                    assert!(lifts > 0);
                    assert_eq!(
                        depth, 0,
                        "borrowed native callback abandoned actual mob pass"
                    );
                    assert_eq!(
                        game.mobs.iter().map(|m| (m.id, m.join)).collect::<Vec<_>>(),
                        rows
                    );
                    assert!(
                        game.creatures.ticked.contains(&71),
                        "earlier actual mob progress was lost"
                    );
                    assert_eq!(
                        VALUE
                            .get_existing(&mut game.world.attachments, Holder::Level)
                            .unwrap(),
                        Some(42)
                    );
                    assert_eq!(game.world.attachment_syncs, syncs);
                    assert_eq!(game.world.path_writes, writes);
                    assert_eq!(game.world.random.clone().next_long(), draw);
                    let mut table = EntityTable::new(journal);
                    for &(id, join) in &rows {
                        let found = table.entity_lookup(&owner.dimension, id, |at| {
                            let m = &game.mobs[game.mobs.locate(at.join)?];
                            Some(SourceEntity {
                                id: m.id,
                                value: m.join,
                            })
                        });
                        assert!(
                            matches!(found, EntityLookup::Found { location, value } if location.owner == owner && value == join)
                        );
                    }
                    table.end_tick().unwrap();
                    if family == Family::Goal {
                        assert!(
                            game.mobs[1].ai.goals.is_running("BorrowedProbe"),
                            "actual native goal selector was lost"
                        );
                    }
                    game.tick_creature_entities().unwrap();
                    table.end_tick().unwrap();
                })
            },
        );
    }
}
macro_rules! cases {
    ($($name:ident => $family:ident),* $(,)?) => {$(
        #[test]
        #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
        fn $name() { run(Family::$family); }
    )*};
}
cases!(damage => Damage, shield => Shield, totem => Totem, movement => Movement, breathing => Breathing, goal => Goal);
