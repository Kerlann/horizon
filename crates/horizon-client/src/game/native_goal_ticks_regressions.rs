//! Production control, authority, target and read-view regressions for the carried EntityPart.
use super::super::native_entity_refs::tests::with_game;
use super::{GoalCx, GoalCxBackend};
use crate::{
    chunk::Chunk,
    mobs::{self, Mob, Who},
};
use horizon_core::geometry::Vec3;
use mod_api::{
    EntityPart, Goal, GoalContext, GoalFlags, GoalRegistrar, Hooks, LivingRef, ModEntry, Registrar,
    RegistrationContext,
};
use std::sync::Arc;

fn mob(game: &super::Game<'_>, id: u32, kind: &str) -> Mob {
    let mut mob = Mob::new(
        id,
        game.res.mob_types().get(kind).unwrap().clone(),
        Vec3::new(3.5, 65.0, 3.5),
        0.0,
    );
    mob.on_ground = true;
    mob.persistent = true;
    mob
}

fn floor(game: &mut super::Game<'_>) {
    game.world
        .chunks
        .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
    let stone = game
        .pack
        .assets
        .registry
        .default_state("minecraft:stone")
        .unwrap();
    for x in 0..16 {
        for z in 0..16 {
            game.world.set(x, 64, z, stone).unwrap();
        }
    }
}

#[derive(Default)]
struct Controls {
    requested_path: bool,
}
impl Goal for Controls {
    fn flags(&self) -> GoalFlags {
        GoalFlags::MOVE
            .union(GoalFlags::JUMP)
            .union(GoalFlags::LOOK)
    }
    fn can_use(&mut self, _: &mut GoalContext<'_>) -> bool {
        true
    }
    fn tick(&mut self, cx: &mut GoalContext<'_>) {
        if !self.requested_path {
            assert!(cx.move_to([10.5, 65.0, 3.5], 1.25));
            self.requested_path = true;
        }
        cx.look_at([10.5, 66.0, 3.5]);
        cx.jump();
    }
    fn requires_update_every_tick(&self) -> bool {
        true
    }
}
struct Part;
#[mod_api::horizon_part]
impl EntityPart for Part {
    fn register_goals(&self, registrar: &mut GoalRegistrar<'_>) {
        registrar.add_goal(0, Controls::default());
    }
}
struct Mod;
#[mod_api::__subscriptions]
impl Hooks for Mod {
    fn register(&self, registrar: &mut dyn Registrar, _: &RegistrationContext) {
        registrar.entity_part("minecraft:zombie", Part);
    }
}
fn install(game: &mut super::Game<'_>) {
    mod_api::__private::install(vec![ModEntry::new("controls", Box::new(Mod))].into());
    mod_api::__private::install_parts(&[ModEntry::new("controls", Box::new(Mod))]).unwrap();
    game.resolve_parts().unwrap();
}

#[test]
#[ignore = "needs prepared resources"]
fn goal_requests_reach_production_navigation_jump_and_physics() {
    with_game(|game| {
        floor(game);
        install(game);
        let mut mob = mob(game, 81, "minecraft:zombie");
        mob.ai.goals.clear();
        mob.ai.targets.clear();
        mobs::part_goals::attach(&mut mob);
        let world = game.mob_world_base(&[]);
        assert!(world.goal_host.is_some());
        let mut level = crate::health::TestLevel::new(0);
        let mut events = Vec::new();
        assert!(mobs::ai::tick(
            &mut mob,
            &world,
            &mut level.level(),
            &mut events
        ));
        assert!(
            mob.position.y > 65.0,
            "JumpControl must not overwrite the goal's request: {:?}",
            mob.position
        );
        assert!(mob.velocity.y > 0.0);
        assert!(!mob.ai.nav.is_done());
        assert_eq!(mob.ai.nav.speed_modifier, 1.25);
        assert!(mob.ai.zza > 0.0, "navigation must reach MoveControl");
        let before = mob.position;
        for _ in 0..8 {
            assert!(mobs::ai::tick(
                &mut mob,
                &world,
                &mut level.level(),
                &mut events
            ));
        }
        assert!(
            mob.position.x > before.x,
            "the actual travel step must advance the path"
        );
        assert!(mob.position.x.is_finite() && mob.position.y.is_finite());
    });
}

#[test]
#[ignore = "needs prepared resources"]
fn goal_numeric_admission_preserves_the_existing_path_and_look() {
    with_game(|game| {
        floor(game);
        let mut mob = mob(game, 82, "minecraft:zombie");
        let world = game.mob_world_base(&[]);
        let mut cx = GoalCx {
            m: &mut mob,
            w: &world,
            host: game,
            backend: game,
        };
        assert!(cx.move_to([10.5, 65.0, 3.5], 1.25));
        cx.look_at([8.0, 66.0, 3.5]);
        let path = cx.m.ai.nav.path.clone();
        let look = cx.m.ai.look.wanted;
        for number in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(!cx.move_to([10.5, 65.0, 3.5], number));
            assert!(!cx.move_to([number, 65.0, 3.5], 1.0));
            cx.look_at([8.0, number, 3.5]);
        }
        assert!(!cx.move_to([2_000_000_001.0, 65.0, 3.5], 1.0));
        cx.look_at([8.0, 66.0, -2_000_000_001.0]);
        assert_eq!(cx.m.ai.nav.path, path);
        assert_eq!(cx.m.ai.nav.speed_modifier, 1.25);
        assert_eq!(cx.m.ai.look.wanted, look);
    });
}

#[test]
#[ignore = "needs prepared resources"]
fn goal_target_admission_matches_handles_and_preserves_refused_targets() {
    with_game(|game| {
        for (source, target, accepted) in [
            ("minecraft:iron_golem", "minecraft:creeper", false),
            ("minecraft:zombie", "minecraft:ghast", false),
            ("minecraft:creeper", "minecraft:goat", false),
            ("minecraft:fox", "minecraft:zombie", false),
            ("minecraft:enderman", "minecraft:zombie", false),
            ("minecraft:zombified_piglin", "minecraft:zombie", false),
            ("minecraft:warden", "minecraft:zombie", false),
            ("minecraft:zombie", "minecraft:cow", true),
        ] {
            game.mobs = vec![mob(game, 83, source), mob(game, 84, target)].into();
            let handle = game
                .native_entity(83)
                .and_then(|e| game.native_mob_ref(e))
                .unwrap();
            let other = LivingRef::new(game.native_entity(84).unwrap());
            game.mobs[0].ai.target = Some(Who::Mob(999));
            assert_eq!(
                handle.set_target(game, Some(other)),
                accepted,
                "handle: {source} -> {target}"
            );
            let mut mobs = std::mem::take(&mut game.mobs);
            mobs[0].ai.target = Some(Who::Mob(999));
            let snapshot = [mobs[1].seen(None)];
            let mut world = game.mob_world_base(&snapshot);
            world.goal_host = Some(game);
            let mut cx = GoalCx {
                m: &mut mobs[0],
                w: &world,
                host: game,
                backend: game,
            };
            assert_eq!(
                cx.set_target(Some(other)),
                accepted,
                "goal: {source} -> {target}"
            );
            assert_eq!(
                cx.m.ai.target,
                Some(Who::Mob(if accepted { 84 } else { 999 }))
            );
            if accepted {
                assert!(cx.set_target(None));
                assert_eq!(cx.m.ai.target, None);
            }
        }
        for alive in [true, false] {
            let mut own = mob(game, 85, "minecraft:zombie");
            let mut target = mob(game, 86, "minecraft:cow");
            target.invulnerable_flag = alive;
            if !alive {
                target.health = 0.0;
            }
            let snapshot = [target.seen(None)];
            let mut world = game.mob_world_base(&snapshot);
            world.goal_host = Some(game);
            let epoch = game.native_lifecycle.epoch().unwrap();
            let other = LivingRef::new(mod_api::EntityRef::new(epoch, 86));
            let mut cx = GoalCx {
                m: &mut own,
                w: &world,
                host: game,
                backend: game,
            };
            assert!(
                !cx.set_target(Some(other)),
                "dead and invulnerable targets both refuse"
            );
        }
    });
}

#[test]
#[ignore = "needs prepared resources"]
fn goal_view_uses_the_selected_connected_player_and_refuses_fake_targets() {
    with_game(|game| {
        super::super::bossbars::audience_tests::add_real_and_fake(game);
        assert!(
            game.player.is_none(),
            "a dedicated/integrated server has no console player"
        );
        let fake = game.native_player_ref(2).unwrap();
        for guest in game.lan.as_mut().unwrap().hub.guests.iter_mut() {
            guest.loaded = true;
        }
        let mut selected = game.lan_guest_views();
        assert_eq!(
            selected.len(),
            1,
            "fake actors are not material player views"
        );
        let (_, player) = selected.pop().unwrap();
        assert_eq!(horizon_client::lan::playerdata::ints_uuid(player.uuid), 1);
        let entity_id = player
            .entity_id
            .expect("the guest swap carries its actual entity id");
        let position = [player.position.x, player.position.y, player.position.z];
        let mut own = mob(game, 87, "minecraft:zombie");
        let mut world = game.mob_world_base(&[]);
        world.player = Some(player);
        world.goal_host = Some(game);
        let mut cx = GoalCx {
            m: &mut own,
            w: &world,
            host: game,
            backend: game,
        };
        let player = cx
            .host()
            .player(1)
            .expect("the selected player resolves by UUID");
        assert_eq!(player.entity().id(), entity_id);
        assert_eq!(cx.host().entity(entity_id), Some(player.entity()));
        assert!(cx.host().living(player.entity()).is_some());
        assert!(cx.host().mob(player.entity()).is_none());
        assert_eq!(player.entity().position(cx.host()), Some(position));
        assert!(cx.host().player(2).is_none());
        assert!(!cx.set_target(Some(LivingRef::new(fake.entity()))));
        assert!(cx.set_target(Some(LivingRef::new(player.entity()))));
        assert_eq!(cx.m.ai.target, Some(Who::Player));
    });
}

#[test]
#[ignore = "needs prepared resources"]
fn goal_view_forwards_loaded_world_and_block_entity_reads() {
    with_game(|game| {
        floor(game);
        game.world.day_time = 1234;
        let level = game.native_level("minecraft:overworld").unwrap();
        let state = game
            .pack
            .assets
            .registry
            .default_state("minecraft:chest")
            .unwrap();
        assert!(game.native_level_set_block(
            level,
            [2, 65, 2],
            state,
            mod_api::NativeBlockUpdateFlags::NEIGHBORS
        ));
        let block_entity = game.native_block_entity_ref(level, [2, 65, 2]).unwrap();
        let mut own = mob(game, 88, "minecraft:zombie");
        let world = game.mob_world_base(&[]);
        let cx = GoalCx {
            m: &mut own,
            w: &world,
            host: game,
            backend: game,
        };
        assert_eq!(level.day_time(cx.host()), Some(1234));
        assert!(level.is_loaded(cx.host(), [2, 65, 2]));
        assert_eq!(level.block_state(cx.host(), [2, 65, 2]), Some(state));
        assert_eq!(block_entity.position(cx.host()), Some([2, 65, 2]));
        assert!(block_entity.data(cx.host()).is_some());
    });
}

#[test]
#[ignore = "needs prepared resources"]
fn only_authoritative_spawn_and_load_register_mod_goals() {
    with_game(|game| {
        install(game);
        let kind = game
            .res
            .mob_types()
            .get("minecraft:zombie")
            .unwrap()
            .clone();
        let client = Mob::client_copy(89, kind.clone(), Vec3::ZERO, 0.0);
        assert!(
            !client
                .ai
                .goals
                .entries()
                .any(|(_, name, _)| name == "Controls")
        );
        let server = Mob::new(90, kind, Vec3::ZERO, 0.0);
        assert!(
            server
                .ai
                .goals
                .entries()
                .any(|(_, name, _)| name == "Controls")
        );
        // Loading uses the same authoritative constructor before restoring saved state.
        let saved = horizon_world::nbt::Tag::Compound(horizon_world::nbt::Compound::from([
            (
                "id".into(),
                horizon_world::nbt::Tag::String("minecraft:zombie".into()),
            ),
            (
                "Pos".into(),
                horizon_world::nbt::Tag::List(vec![horizon_world::nbt::Tag::Double(0); 3]),
            ),
        ]));
        let types =
            std::collections::HashMap::from([("minecraft:zombie".to_owned(), server.kind.clone())]);
        let loaded = crate::entity_nbt::mob_from_nbt(&saved, &types, 91, None).unwrap();
        assert!(
            loaded
                .ai
                .goals
                .entries()
                .any(|(_, name, _)| name == "Controls")
        );
    });
}
