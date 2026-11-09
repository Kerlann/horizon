use super::*;
use crate::game::native_guest_turn_tests::with_guests;
use horizon_client::{chunk::Chunk, mobs::Mob, save::uuid_text};
use horizon_core::geometry::Vec3;
use std::sync::{
    Arc,
    atomic::{AtomicI32, Ordering},
};

#[derive(Clone, Copy, Debug)]
enum Change {
    None,
    Silent,
    Member,
    Rule,
    NoopRule,
    FailedRule,
    Tame,
    Root,
    Spectator,
    Teleport,
    FailedTeleport,
}
struct BeforeFirst {
    change: Change,
    result: Arc<AtomicI32>,
    member: String,
    root: String,
}
#[mod_api::__subscriptions]
impl mod_api::Hooks for BeforeFirst {
    fn entity_tick_listened(
        &self,
        _: &mut dyn mod_api::ServerHost,
        event: &mut mod_api::EntityTickListenedEvent,
    ) {
        event.listened = true;
    }
    fn entity_tick_pre(
        &self,
        host: &mut dyn mod_api::ServerHost,
        event: &mut mod_api::EntityTickPreEvent<'_>,
    ) -> mod_api::Flow {
        if event.entity == 71 {
            let ok = match self.change {
                Change::None => true,
                Change::Silent => matches!(
                    host.entity(71).unwrap().set_silent(host, true),
                    mod_api::WriteReceipt::Applied(true) | mod_api::WriteReceipt::Pending(_)
                ),
                Change::Member => host
                    .commands()
                    .run_command(&format!("team join red {}", self.member)),
                Change::Rule => host
                    .commands()
                    .run_command("team modify red collisionRule never"),
                Change::NoopRule => host
                    .commands()
                    .run_command("team modify red collisionRule pushOwnTeam"),
                Change::FailedRule => host
                    .commands()
                    .run_command("team modify missing collisionRule never"),
                Change::Tame => host.commands().run_command(&format!(
                    "data merge entity {} {{Owner:[I;0,0,0,12]}}",
                    self.member
                )),
                Change::Root => host.commands().run_command(&format!(
                    "data merge entity {} {{Owner:[I;0,0,0,13]}}",
                    self.root
                )),
                Change::Spectator => host.commands().run_command("gamemode spectator guest12"),
                Change::Teleport => host.commands().run_command("tp guest12 1000 64 5"),
                Change::FailedTeleport => host.commands().run_command("tp guest12 30000000 64 5"),
            };
            self.result.store(if ok { 1 } else { -1 }, Ordering::SeqCst);
        }
        mod_api::Flow::Continue
    }
}
pub(super) fn prepare(game: &mut Game<'_>) {
    game.build_registries();
    game.options.dedicated = Some(Vec::new());
    game.native_level_opened(game.dim);
    game.reset_entity_residency();
    game.ui.difficulty = 2;
    game.world.health_rules.difficulty = horizon_world::food::Difficulty::Normal;
    game.world
        .chunks
        .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
    for conn in [12, 13] {
        game.lan.as_mut().unwrap().hub.update_guest(conn, |g| {
            g.loaded = true;
        });
    }
}
pub(super) fn add(game: &mut Game<'_>, id: u32, kind: &str, at: Vec3, no_ai: bool) {
    let mut mob = Mob::new(id, game.res.mob_types().get(kind).unwrap().clone(), at, 0.0);
    mob.no_ai = no_ai;
    game.mobs.push(mob);
    game.set_saved_entity_uuid(id, [4, 0, 0, id as i32]);
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn actual_pre_team_commands_and_root_writes_change_next_actor_collision() {
    for change in [
        Change::None,
        Change::Silent,
        Change::Member,
        Change::Rule,
        Change::NoopRule,
        Change::FailedRule,
        Change::Tame,
        Change::Root,
    ] {
        let result = Arc::new(AtomicI32::new(0));
        mod_api::testing::with_mods(
            vec![mod_api::ModEntry::new(
                "backing",
                Box::new(BeforeFirst {
                    change,
                    result: result.clone(),
                    member: uuid_text([4, 0, 0, 72]),
                    root: uuid_text([4, 0, 0, 73]),
                }),
            )]
            .into(),
            || {
                with_guests(|game| {
                    prepare(game);
                    add(game, 71, "minecraft:pig", Vec3::new(10.0, 70.0, 0.5), true);
                    add(
                        game,
                        72,
                        "minecraft:wolf",
                        Vec3::new(10.25, 70.0, 0.5),
                        true,
                    );
                    game.score.board.add_team("red").collision_rule =
                        horizon_client::scoreboard::CollisionRule::PushOwnTeam;
                    game.score.board.add_team("blue");
                    game.score
                        .board
                        .add_to_team(&uuid_text([4, 0, 0, 71]), "red");
                    game.score.board.add_to_team("guest12", "red");
                    game.score.board.add_to_team("guest13", "blue");
                    if matches!(change, Change::Root) {
                        add(game, 73, "minecraft:wolf", Vec3::new(14.0, 70.0, 0.5), true);
                        game.mobs[1].extra.tame = true;
                        game.mobs[1].extra.owner = Some([4, 0, 0, 73]);
                        game.mobs[2].extra.tame = true;
                        game.mobs[2].extra.owner = Some([0, 0, 0, 12]);
                    } else if !matches!(change, Change::Tame) {
                        game.score
                            .board
                            .add_to_team(&uuid_text([4, 0, 0, 72]), "blue");
                    }
                    game.tick_creature_entities().unwrap();
                    let failed = matches!(change, Change::NoopRule | Change::FailedRule);
                    assert_eq!(
                        result.load(Ordering::SeqCst),
                        if failed { -1 } else { 1 },
                        "{change:?}: actual command result"
                    );
                    let blocked = matches!(change, Change::Member | Change::Rule | Change::Tame);
                    assert_eq!(
                        game.mobs[1].velocity.x == 0.0,
                        blocked,
                        "{change:?}: next actor collision"
                    );
                    let revision = game.entity_read_state.borrow().mob_backing;
                    assert_eq!(
                        revision.players, 0,
                        "{change:?}: team changes reuse player projections"
                    );
                    if matches!(change, Change::Silent) {
                        assert!(game.mobs[0].silent);
                    }
                });
            },
        );
    }
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn actual_pre_player_commands_change_next_actor_nearest_player_despawn() {
    for change in [
        Change::None,
        Change::Silent,
        Change::Spectator,
        Change::Teleport,
        Change::FailedTeleport,
    ] {
        let result = Arc::new(AtomicI32::new(0));
        mod_api::testing::with_mods(
            vec![mod_api::ModEntry::new(
                "backing",
                Box::new(BeforeFirst {
                    change,
                    result: result.clone(),
                    member: String::new(),
                    root: String::new(),
                }),
            )]
            .into(),
            || {
                with_guests(|game| {
                    prepare(game);
                    game.lan.as_mut().unwrap().hub.update_guest(13, |g| {
                        g.position = [1000.0, 64.0, 5.0];
                        g.previous_position = g.position;
                    });
                    add(game, 71, "minecraft:pig", Vec3::new(14.0, 64.0, 5.0), true);
                    add(
                        game,
                        72,
                        "minecraft:slime",
                        Vec3::new(3.25, 64.0, 5.0),
                        false,
                    );
                    horizon_client::mobs::cube::set_size(&mut game.mobs[1], 2);
                    game.tick_creature_entities().unwrap();
                    assert_eq!(
                        result.load(Ordering::SeqCst),
                        if matches!(change, Change::FailedTeleport) {
                            -1
                        } else {
                            1
                        },
                        "{change:?}"
                    );
                    let changed = matches!(change, Change::Spectator | Change::Teleport);
                    let alive = game.mobs.iter().any(|mob| mob.id == 72 && !mob.removed);
                    assert_eq!(
                        alive, !changed,
                        "{change:?}: next actor's hard-distance despawn"
                    );
                    let revision = game.entity_read_state.borrow().mob_backing;
                    assert_eq!(
                        revision.players > 0,
                        changed,
                        "{change:?}: applied relevant writes only"
                    );
                });
            },
        );
    }
}
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn canonical_player_writes_refresh_only_changed_projection_and_ignore_noops() {
    use super::native_mob_backing::{PlayerBackingIndex, PlayerBackingLoans};
    with_guests(|game| {
        prepare(game);
        game.native_lifecycle.world_open_ready().unwrap();
        game.native_lifecycle.server_tick_start(true).unwrap();
        game.prepare_ai_mob_relations(&game.mobs);
        let mobs = game.mobs.lift();
        let mut local = None;
        let mut spare = None;
        let mut guests: Vec<_> = [12, 13]
            .into_iter()
            .map(|conn| (conn, game.ai_guest_view(conn, &mobs).unwrap()))
            .collect();
        let untouched = guests[1].1.clone();
        let mut guest_ids = vec![(12, 12), (13, 13)];
        let mut nearest = crate::game::natural_spawning::NearestPlayers::new(&game.level_players());
        let mut index = PlayerBackingIndex::new(&guests, &guest_ids);
        macro_rules! refresh {
            () => {
                game.refresh_ai_player_backing(
                    &mut index,
                    PlayerBackingLoans {
                        mobs: &mobs,
                        local: &mut local,
                        spare: &mut spare,
                        guests: &mut guests,
                        guest_ids: &mut guest_ids,
                        nearest: &mut nearest,
                    },
                )
            };
        }
        game.entity_read_state.borrow_mut().mob_ai_level = Some(game.dim.key());
        for command in [
            "item replace entity guest12 weapon.mainhand with minecraft:carrot",
            "item replace entity guest12 armor.head with minecraft:creeper_head",
            "effect give guest12 minecraft:invisibility 120 0 true",
        ] {
            assert!(
                game.as_console(|game| game.run_command(command)),
                "{command}: {:?}",
                game.ui.chat.recent_lines(3)
            );
            assert_eq!(refresh!(), 1, "{command}");
            let _ = game.as_console(|game| game.run_command(command));
            assert_eq!(refresh!(), 0, "same represented facts: {command}");
        }
        assert_eq!(guests[0].1.main_hand.as_deref(), Some("minecraft:carrot"));
        assert_eq!(guests[0].1.head.as_deref(), Some("minecraft:creeper_head"));
        assert!(guests[0].1.invisible);
        assert!(guests[0].1.effects.contains(&"invisibility"));
        let entity = game.entity(12).unwrap();
        let living = game.living(entity).unwrap();
        let head = game
            .pack
            .assets
            .registry
            .resolve(&horizon_world::chunk::BlockState {
                name: "minecraft:red_bed".into(),
                properties: [("facing", "north"), ("occupied", "false"), ("part", "head")]
                    .into_iter()
                    .map(|(k, v)| (k.into(), v.into()))
                    .collect(),
            })
            .unwrap();
        game.world.set(5, 64, 5, head).unwrap();
        let mut foot = game.pack.assets.registry.state(head).unwrap().clone();
        foot.properties.insert("part".into(), "foot".into());
        game.world
            .set(5, 64, 6, game.pack.assets.registry.id(&foot).unwrap())
            .unwrap();
        assert!(game.living_start_sleeping(living, [5, 64, 5]));
        assert_eq!(refresh!(), 1);
        assert!(guests[0].1.sleeping);
        assert!(game.living_stop_sleeping(living));
        assert_eq!(refresh!(), 1);
        assert!(!guests[0].1.sleeping);
        let _ = game.living_stop_sleeping(living);
        assert_eq!(refresh!(), 0);
        assert!(game.as_console(|game| game.run_command("summon minecraft:oak_boat 3 64 5")));
        let boat_id = game.boats.last().unwrap().id;
        let command = format!("ride guest12 mount {}", uuid_text(game.uuid_for(boat_id)));
        assert!(game.as_console(|game| game.run_command(&command)));
        assert_eq!(refresh!(), 1);
        assert_eq!(guests[0].1.riding_vehicle, Some(boat_id));
        assert!(guests[0].1.vehicle.is_some());
        let _ = game.as_console(|game| game.run_command(&command));
        assert_eq!(refresh!(), 0);
        let boat = game.entity(boat_id).unwrap();
        assert!(game.entity_set_velocity(boat, [1.0, 0.0, 0.0]));
        assert_eq!(refresh!(), 1, "related vehicle velocity");
        assert_eq!(guests[0].1.vehicle.as_ref().unwrap().velocity.x, 1.0);
        assert!(game.entity_set_velocity(boat, [1.0, 0.0, 0.0]));
        assert_eq!(refresh!(), 0, "same vehicle velocity");
        assert!(!game.entity_set_velocity(boat, [f64::NAN, 0.0, 0.0]));
        assert_eq!(refresh!(), 0, "failed related-vehicle write");
        assert!(game.entity_teleport_to(boat, [8.0, 64.0, 5.0], None));
        assert_eq!(refresh!(), 1, "related vehicle pose");
        assert_eq!(
            guests[0]
                .1
                .vehicle
                .as_ref()
                .unwrap()
                .bounds
                .min(horizon_core::geometry::Axis::X),
            8.0 - 0.6875
        );
        assert!(game.entity_teleport_to(boat, [8.0, 64.0, 5.0], None));
        assert_eq!(refresh!(), 0, "same vehicle pose and stopped velocity");
        assert!(game.as_console(|game| game.run_command("ride guest12 dismount")));
        assert_eq!(refresh!(), 1);
        assert!(!guests[0].1.passenger);
        assert!(game.as_console(|g| g.run_command("summon minecraft:nautilus 10 64 5")));
        let nautilus_id = game.mobs.last().unwrap().id;
        let nautilus = game.living(game.entity(nautilus_id).unwrap()).unwrap();
        let mount = format!(
            "ride guest12 mount {}",
            uuid_text(game.uuid_for(nautilus_id))
        );
        assert!(game.as_console(|g| g.run_command(&mount)));
        assert_eq!(refresh!(), 1);
        let vehicle = guests[0].1.vehicle.as_ref().unwrap();
        assert!(
            vehicle.nautilus,
            "new current-source mounts coexist with the lent roster"
        );
        let axis = horizon_core::geometry::Axis::X;
        let width = vehicle.bounds.max(axis) - vehicle.bounds.min(axis);
        assert!(game.living_set_attribute_base(nautilus, "minecraft:movement_speed", 0.5));
        assert_eq!(
            refresh!(),
            0,
            "an unrelated mount attribute is not geometry"
        );
        let mob = game.mob(nautilus.entity()).unwrap();
        assert!(game.mob_set_baby(mob, true));
        assert_eq!(
            refresh!(),
            1,
            "related mount dimensions after canonical age apply"
        );
        let bounds = guests[0].1.vehicle.as_ref().unwrap().bounds;
        assert_eq!(bounds.max(axis) - bounds.min(axis), width * 0.5);
        assert!(game.mob_set_baby(mob, true));
        assert_eq!(refresh!(), 0, "same represented mount dimensions");
        assert!(game.as_console(|g| g.run_command("ride guest12 dismount")));
        assert_eq!(refresh!(), 1);
        assert!(game.living_set_health(living, 18.0));
        assert_eq!(refresh!(), 1);
        assert!(game.living_set_health(living, 18.0));
        assert!(!game.living_set_health(living, f32::NAN));
        assert_eq!(refresh!(), 0);
        assert!(game.entity_set_invulnerable(entity, true));
        assert_eq!(refresh!(), 1);
        assert!(guests[0].1.invulnerable);
        assert!(game.entity_set_invulnerable(entity, true));
        assert_eq!(refresh!(), 0);
        assert_eq!(
            guests[1].1, untouched,
            "another guest's projection is untouched"
        );
        assert!(game.living_set_health(living, 0.0));
        assert_eq!(refresh!(), 1);
        let p = &guests[0].1.position;
        let closest = nearest.nearest(p.x, p.y, p.z).unwrap();
        assert_eq!(closest.uuid, 12);
        assert!(!closest.alive);
        assert!(!guests.iter().find(|(conn, _)| *conn == 12).unwrap().1.alive);
        game.entity_read_state.borrow_mut().mob_ai_level = None;
        game.mobs
            .restore(mobs, horizon_client::join_seq::RestoreOrder::SurvivorsFirst);
    });
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn real_saved_uuid_path_tracks_source_level_not_temporary_game_dimension() {
    crate::game::native_entity_refs::tests::with_game(|game| {
        game.build_registries();
        game.options.dedicated = Some(Vec::new());
        game.native_level_opened(game.dim);
        game.native_lifecycle.world_open_ready().unwrap();
        game.native_lifecycle.server_tick_start(true).unwrap();
        game.enter_level(crate::game::Dim::Nether).unwrap();
        add(game, 73, "minecraft:wolf", Vec3::new(1.0, 70.0, 1.0), true);
        game.remove_saved_entity_uuid(73);
        game.enter_level(crate::game::Dim::Overworld).unwrap();
        add(game, 71, "minecraft:wolf", Vec3::new(1.0, 70.0, 1.0), true);
        add(game, 72, "minecraft:wolf", Vec3::new(2.0, 70.0, 1.0), true);
        game.remove_saved_entity_uuid(71);
        game.score.board.add_team("red");
        game.score.board.add_team("blue");
        macro_rules! team {
            ($id:expr) => {
                horizon_client::mobs::TeamLookup::team(
                    game,
                    &game.mobs[0],
                    horizon_client::mobs::Who::Mob($id),
                    None,
                )
                .map(|t| t.name.as_str())
            };
        }
        assert_eq!(team!(71), None);
        let overworld = game.dim.key();
        let nether = mod_api::DimensionKey::NETHER;
        let parked = game.uuid_for_at(nether, 73).unwrap();
        assert_eq!(game.dim.key(), overworld);
        assert!(!game.persist.uuids.contains_key(&73));
        game.enter_level(crate::game::Dim::Nether).unwrap();
        let first = game.uuid_for_at(overworld, 71).unwrap();
        game.score.board.add_to_team(&uuid_text(first), "red");
        assert_eq!(game.uuid_for_at(nether, 73), Some(parked));
        game.enter_level(crate::game::Dim::Overworld).unwrap();
        assert_eq!(team!(71), Some("red"));
        let command = format!(
            "data merge entity {} {{Owner:[I;{}]}}",
            uuid_text([4, 0, 0, 72]),
            first
                .iter()
                .map(i32::to_string)
                .collect::<Vec<_>>()
                .join(",")
        );
        assert!(game.as_console(|game| game.run_command(&command)));
        assert_eq!(team!(72), Some("red"));
        let second = [0xabcd, 0, 0, 71];
        game.score.board.add_to_team(&uuid_text(second), "blue");
        assert_eq!(game.set_saved_entity_uuid(71, second), Some(first));
        assert_eq!(team!(71), Some("blue"));
        assert_eq!(team!(72), None, "old UUID no longer names a terminal owner");
        assert!(
            game.score
                .board
                .add_to_team(&uuid_text(second).to_ascii_uppercase(), "red")
        );
        assert_eq!(
            team!(71),
            Some("blue"),
            "UUID score-holder spelling is literal"
        );
        assert_eq!(game.set_saved_entity_uuid(71, second), Some(second));
        assert_eq!(team!(71), Some("blue"));
        assert_eq!(game.remove_saved_entity_uuid(71), Some(second));
        assert_eq!(team!(71), None);
        assert_eq!(game.remove_saved_entity_uuid(71), None);
        assert_eq!(team!(71), None);
    });
}
