use super::*;
use crate::game::native_guest_turn_tests::with_guests;
use horizon_client::{
    chunk::Chunk,
    mobs::{Mob, Who},
    save::uuid_text,
};
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
fn prepare(game: &mut Game<'_>) {
    game.options.dedicated = Some(Vec::new());
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
fn add(game: &mut Game<'_>, id: u32, kind: &str, at: Vec3, no_ai: bool) {
    let mut mob = Mob::new(id, game.res.mob_types().get(kind).unwrap().clone(), at, 0.0);
    mob.no_ai = no_ai;
    game.mobs.push(mob);
    game.persist.uuids.insert(id, [4, 0, 0, id as i32]);
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
                    assert_eq!(
                        revision.teams > 0,
                        matches!(
                            change,
                            Change::Member | Change::Rule | Change::Tame | Change::Root
                        ),
                        "{change:?}: real changes only"
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
                    assert_eq!(revision.teams, 0);
                });
            },
        );
    }
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn repeated_small_team_and_owner_changes_have_population_independent_refresh_counts() {
    crate::game::native_entity_refs::tests::with_game(|game| {
        for count in [128, 8192] {
            game.score.board.end_ai_changes();
            game.mobs.clear();
            game.persist.uuids.clear();
            for id in 10_000..10_000 + count {
                add(game, id, "minecraft:wolf", Vec3::new(1.0, 70.0, 1.0), true);
            }
            game.score.board.add_team("red");
            game.score.board.add_team("blue");
            game.score
                .board
                .add_to_team(&uuid_text([0, 0, 0, 12]), "red");
            game.score
                .board
                .add_to_team(&uuid_text([0, 0, 0, 13]), "blue");
            game.mobs[0].extra.tame = true;
            game.mobs[0].extra.owner = Some([0, 0, 0, 12]);
            game.mobs[1].extra.tame = true;
            game.mobs[1].extra.owner = Some([4, 0, 0, 10_000]);
            let member = uuid_text([4, 0, 0, 10_002]);
            game.score.board.add_to_team(&member, "red");
            let teams = game.mob_team_backing_for(&game.mobs);
            game.score.board.begin_ai_changes();
            let start = std::time::Instant::now();
            let mut member_refreshes = 0;
            let mut owner_refreshes = 0;
            for round in 0..1000 {
                let (team, owner) = if round % 2 == 0 {
                    ("blue", [0, 0, 0, 13])
                } else {
                    ("red", [0, 0, 0, 12])
                };
                assert!(game.score.board.add_to_team(&member, team));
                member_refreshes += game.refresh_mob_team_deltas(&teams);
                assert_eq!(
                    teams.of(Who::Mob(10_002)).unwrap().identity,
                    game.score.board.team(team).unwrap().ai_identity()
                );
                game.mobs[0].extra.owner = Some(owner);
                game.sync_mob_owner_backing(&game.mobs[0]);
                owner_refreshes += game.refresh_mob_team_deltas(&teams);
                assert_eq!(
                    teams.of(Who::Mob(10_001)).unwrap().identity,
                    game.score.board.team(team).unwrap().ai_identity()
                );
                assert_eq!(game.refresh_mob_team_deltas(&teams), 0);
            }
            let elapsed = start.elapsed();
            assert_eq!((member_refreshes, owner_refreshes), (1000, 1000));
            println!(
                "entities={count}, iterations=1000, member_refreshes={member_refreshes}, owner_refreshes={owner_refreshes}, elapsed={elapsed:?}"
            );
            game.score.board.end_ai_changes();
        }
    });
}
