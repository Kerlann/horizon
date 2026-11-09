//! Resource-backed checks of Game's production residency boundaries.
#![cfg(test)]
use super::{Dim, Game, commands::Target, native_entity_refs::tests::with_game};
use horizon_client::join_seq::residency::table::EntityLookup;

fn seed_every_owner(game: &mut Game<'_>) -> Vec<Target> {
    for (kind, tag) in [
        ("pig", "{}"),
        ("item", "{Item:{id:\"minecraft:stone\",count:1}}"),
        ("experience_orb", "{Value:1s}"),
        ("arrow", "{}"),
        ("armor_stand", "{}"),
        ("item_frame", "{}"),
        ("painting", "{variant:\"minecraft:kebab\"}"),
        ("oak_boat", "{}"),
        ("minecart", "{}"),
        ("text_display", "{}"),
        ("evoker_fangs", "{}"),
        ("end_crystal", "{}"),
        ("ender_dragon", "{}"),
        ("tnt", "{fuse:80s}"),
        (
            "falling_block",
            "{BlockState:{Name:\"minecraft:sand\"},Time:1}",
        ),
        ("firework_rocket", "{LifeTime:20}"),
        ("trident", "{}"),
        (
            "ominous_item_spawner",
            "{item:{id:\"minecraft:stone\",count:1}}",
        ),
        ("area_effect_cloud", "{Duration:200,Radius:3.0f}"),
        ("cushion", "{}"),
    ] {
        game.summon_with_nbt(&format!("minecraft:{kind}"), [1.0, 70.0, 1.0], tag)
            .unwrap_or_else(|error| panic!("{kind}: {error:?}"));
    }
    game.native_joins_skip();
    // Deliberately independent of the production dispatcher family macro.
    vec![
        Target::Mob(game.mobs[0].id),
        Target::Item(game.dropped[0].id),
        Target::Orb(game.orbs[0].id),
        Target::Projectile(game.creatures.projectiles[0].id),
        Target::Stand(game.armor_stands[0].id),
        Target::Frame(game.item_frames[0].id),
        Target::Painting(game.paintings[0].id),
        Target::Boat(game.boats[0].id),
        Target::Cart(game.carts[0].id),
        Target::Other(game.others[0].id),
        Target::Fangs(game.fangs.list[0].id),
        Target::Crystal(game.crystals[0].id),
        Target::Dragon(game.dragon.as_ref().unwrap().id),
        Target::Tnt(game.tnts[0].id),
        Target::Falling(game.falling[0].id),
        Target::Rocket(game.rockets[0].id),
        Target::Trident(game.tridents[0].id),
        Target::Spawner(game.ominous_spawners[0].id),
        Target::Cloud(game.breath_clouds[0].id),
        Target::Cushion(game.cushions[0].id),
    ]
}

fn assert_visible(game: &Game<'_>, targets: &[Target], dim: Dim) {
    for &target in targets {
        let id = super::native_entities::target_id(target).unwrap();
        assert!(
            matches!(game.server_resident_entity(id), EntityLookup::Found {location, value}
            if value == target && location.owner.dimension == dim.key()),
            "{target:?} in {dim:?}"
        );
    }
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn every_family_survives_active_and_parked_epoch_rebinding() {
    with_game(|game| {
        game.native_level_opened(Dim::Overworld);
        let overworld = seed_every_owner(game);
        assert_visible(game, &overworld, Dim::Overworld);
        game.enter_level(Dim::Nether).unwrap();
        let nether = seed_every_owner(game);
        assert_visible(game, &nether, Dim::Nether);
        assert_visible(game, &overworld, Dim::Overworld);
        game.reset_entity_residency();
        assert_visible(game, &overworld, Dim::Overworld);
        assert_visible(game, &nether, Dim::Nether);
        game.enter_level(Dim::Overworld).unwrap();
        game.reset_entity_residency();
        assert_visible(game, &overworld, Dim::Overworld);
        assert_visible(game, &nether, Dim::Nether);
        assert_eq!(game.entity_residency.get_mut().end_tick(), Ok(()));
    });
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn portal_and_dragon_delayed_admission_and_terminal_kill() {
    with_game(|game| {
        game.reset_entity_residency();
        for kind in ["trident", "firework_rocket", "ender_dragon", "end_crystal"] {
            game.summon_with_nbt(&format!("minecraft:{kind}"), [1.0, 70.0, 1.0], "{}")
                .unwrap();
        }
        macro_rules! pending {
            ($list:expr) => {{
                let mut entity = $list.remove(0);
                entity.id = 0;
                $list.push(entity);
            }};
        }
        pending!(game.tridents);
        pending!(game.rockets);
        pending!(game.crystals);
        let mut dragon = game.dragon.extract().unwrap();
        game.dragon.retire_extracted(dragon.id, dragon.join);
        dragon.id = 0;
        assert!(game.dragon.install(dragon).is_ok());
        game.find_entity_portals();
        assert_visible(
            game,
            &[
                Target::Trident(game.tridents[0].id),
                Target::Rocket(game.rockets[0].id),
            ],
            game.dim,
        );
        game.assign_dragon_and_crystal_ids();
        let id = game.dragon.as_ref().unwrap().id;
        assert_visible(
            game,
            &[Target::Dragon(id), Target::Crystal(game.crystals[0].id)],
            game.dim,
        );
        game.kill_dragon(id);
        assert_eq!(game.server_resident_entity(id), EntityLookup::Missing);
        assert_eq!(game.entity_residency.get_mut().end_tick(), Ok(()));
    });
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn native_summon_roots_and_pending_joins_publish_admission() {
    struct Observer;
    #[mod_api::__subscriptions]
    impl mod_api::Hooks for Observer {}
    let mods: mod_api::Mods = vec![mod_api::ModEntry::new(
        "residency-proof",
        Box::new(Observer),
    )]
    .into();
    mod_api::testing::with_mods(mods, || {
        with_game(|game| {
            game.reset_entity_residency();
            game.options.dedicated = Some(Vec::new());
            game.world.chunks.insert(
                (0, 0),
                std::sync::Arc::new(horizon_client::chunk::Chunk::empty(0, 0)),
            );
            for (kind, nbt) in [
                ("minecraft:tnt", "{fuse:80s}"),
                (
                    "minecraft:falling_block",
                    "{BlockState:{Name:\"minecraft:sand\"},Time:1}",
                ),
                ("minecraft:firework_rocket", "{LifeTime:20}"),
            ] {
                let (horizon_core::nbt::Tag::Compound(tag), _) =
                    horizon_core::nbt::parse_snbt(nbt).unwrap()
                else {
                    unreachable!()
                };
                let id = game
                    .summon(game.dim.id(), kind, [1.0, 70.0, 1.0], tag)
                    .unwrap();
                assert!(
                    matches!(game.server_resident_entity(id), EntityLookup::Found { .. }),
                    "{kind}"
                );
            }
            // Force a genuine pending-join pass rather than the LAN allocator.
            macro_rules! pending {
                ($list:expr) => {{
                    let mut entity = $list.pop().unwrap();
                    entity.id = 0;
                    $list.push(entity);
                }};
            }
            pending!(game.tnts);
            pending!(game.falling);
            pending!(game.rockets);
            game.summon_with_nbt("minecraft:arrow", [1.0, 70.0, 1.0], "{}")
                .unwrap();
            game.summon_with_nbt("minecraft:trident", [1.0, 70.0, 1.0], "{}")
                .unwrap();
            pending!(game.creatures.projectiles);
            pending!(game.tridents);
            game.native_joins_skip();
            assert_visible(
                game,
                &[
                    Target::Tnt(game.tnts[0].id),
                    Target::Falling(game.falling[0].id),
                    Target::Rocket(game.rockets[0].id),
                    Target::Projectile(game.creatures.projectiles[0].id),
                    Target::Trident(game.tridents[0].id),
                ],
                game.dim,
            );
            assert_eq!(game.entity_residency.get_mut().end_tick(), Ok(()));
        })
    });
}
