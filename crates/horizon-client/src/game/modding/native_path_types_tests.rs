//! `block_path_type` and `adjacent_block_path_type` answered by block Parts (#3190): the answers
//! are asked once per state when parts resolve, and the walk evaluator reads them.
use super::Game;
use super::native_entity_refs::tests::with_game;
use horizon_client::mobs::{
    Mob,
    data::PartAnswers,
    path::{Evaluator, Finder},
};
use horizon_core::geometry::Vec3;
use mod_api::{
    AdjacentBlockPathTypeEvent, BlockPart, BlockPathTypeEvent, Hooks, Host, PathType, Registrar,
};
use std::sync::Arc;

/// Smooth stone hurts a mob that walks on it, as a cactus does.
struct Thorny;
#[mod_api::horizon_part]
impl BlockPart for Thorny {
    fn block_path_type(&self, _: &dyn Host, event: &mut BlockPathTypeEvent) {
        *event.path_type = PathType::Damaging;
    }
}

struct ThornyMod;
#[mod_api::__subscriptions]
impl Hooks for ThornyMod {
    fn register(&self, r: &mut dyn Registrar, _: &mod_api::RegistrationContext) {
        let smooth_stone = r.block_key("minecraft:smooth_stone");
        r.block_part(smooth_stone, Thorny);
    }
}

/// Gives the nodes beside its block `adjacent`.
struct Beside(Option<PathType>);
#[mod_api::horizon_part]
impl BlockPart for Beside {
    fn adjacent_block_path_type(&self, _: &dyn Host, event: &mut AdjacentBlockPathTypeEvent) {
        *event.adjacent = self.0;
    }
}

struct BesideMod;
#[mod_api::__subscriptions]
impl Hooks for BesideMod {
    fn register(&self, r: &mut dyn Registrar, _: &mod_api::RegistrationContext) {
        // Magma burns, but leaves the nodes beside it alone; glass makes them fire's neighbours.
        let magma = r.block_key("minecraft:magma_block");
        r.block_part(magma, Beside(None));
        let glass = r.block_key("minecraft:glass");
        r.block_part(glass, Beside(Some(PathType::FireInNeighbor)));
    }
}

/// Run `test` in a game with `hooks`' parts installed, over an empty chunk.
fn with_parts(id: &'static str, hooks: impl Hooks + 'static, test: impl FnOnce(&mut Game<'_>)) {
    let mods: mod_api::Mods = vec![mod_api::ModEntry::new(id, Box::new(hooks))].into();
    crate::game::api_values::install_parts(&mods).unwrap();
    mod_api::testing::with_mods(mods, || {
        with_game(|game| {
            game.world
                .chunks
                .insert((0, 0), Arc::new(horizon_client::chunk::Chunk::empty(0, 0)));
            test(game);
        })
    });
}

/// Every state answers as its block does, as if no part were attached.
fn without_parts(game: &Game<'_>) {
    game.res
        .mob_data()
        .resolve_parts(game.res.facts(), &PartAnswers::default());
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_block_part_gives_its_block_a_path_type() {
    with_parts("thorny", ThornyMod, |game| {
        let registry = game.pack.assets.registry;
        let stone = registry.default_state("minecraft:stone").unwrap();
        let smooth_stone = registry.default_state("minecraft:smooth_stone").unwrap();
        // A floor 9 blocks long and 3 wide over the void, with a smooth stone row across it.
        for x in 0..9 {
            for z in 0..3 {
                let floor = if x == 4 { smooth_stone } else { stone };
                game.world.set(x, 64, z, floor).unwrap();
            }
        }
        let kind = game
            .res
            .mob_types()
            .get("minecraft:zombie")
            .unwrap()
            .clone();
        let mut zombie = Mob::new(50, kind, Vec3::new(0.5, 65.0, 1.5), 0.0).path_mob();
        zombie.on_ground = true;
        // A path from one end to the other, and where it ends.
        let walk = |game: &Game<'_>| {
            let world = game.mob_world_base(&[]);
            Finder::new(&world, &zombie, Evaluator::Walk)
                .find(&[[8, 65, 1]], 16.0, 0, 256)
                .map(|path| (path.reached, path.nodes.iter().map(|node| node.x).max()))
        };

        without_parts(game);
        let world = game.mob_world_base(&[]);
        assert_eq!(world.path_type_static(4, 65, 1), PathType::Walkable);
        assert_eq!(walk(game), Some((true, Some(8))));

        // With the part, the row is `Damaging` (a malus of -1) and there is no way around it.
        game.resolve_block_paths();
        let world = game.mob_world_base(&[]);
        assert_eq!(world.path_type_static(4, 65, 1), PathType::Damaging);
        assert_eq!(world.path_type_static(1, 65, 1), PathType::Walkable);
        assert!(
            walk(game).is_none_or(|(reached, furthest)| !reached && furthest < Some(4)),
            "{:?}",
            walk(game)
        );
    });
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_block_part_gives_the_nodes_beside_its_block_a_path_type() {
    with_parts("beside", BesideMod, |game| {
        let registry = game.pack.assets.registry;
        let state = |name: &str| registry.default_state(name).unwrap();
        for x in 0..6 {
            game.world.set(x, 64, 0, state("minecraft:stone")).unwrap();
        }
        game.world
            .set(1, 64, 0, state("minecraft:magma_block"))
            .unwrap();

        // Vanilla: magma burns (`Fire`), so the node beside it is `FireInNeighbor`.
        without_parts(game);
        let node = |game: &Game<'_>, x| game.mob_world_base(&[]).path_type_static(x, 65, 0);
        assert_eq!(node(game, 0), PathType::FireInNeighbor);

        // Magma's part leaves the nodes beside it their own type.
        game.resolve_block_paths();
        assert_eq!(node(game, 0), PathType::Walkable);

        // Glass gives the nodes beside it `FireInNeighbor`, and only those.
        game.world.set(1, 64, 0, state("minecraft:stone")).unwrap();
        game.world.set(1, 65, 0, state("minecraft:glass")).unwrap();
        assert_eq!(node(game, 0), PathType::FireInNeighbor);
        assert_eq!(node(game, 4), PathType::Walkable);
    });
}
