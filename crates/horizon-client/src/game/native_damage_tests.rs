//! `living_damage_pre` and `living_damage_post` heard by a trait mod, through the game's own
//! hooks: the damage container each hit fills, the hit's level host, and the mob and dragon
//! hits that fire them.
use super::dragon::{Dragon, SPECIAL};
use super::entity::damage_level_of;
use super::native_entity_refs::tests::with_game;
use horizon_client::mobs::{Damage, DamageKind, Mob};
use horizon_core::{geometry::Vec3, random::RandomSource};
use mod_api::{
    self, Hooks, LevelRandomHost, LivingDamagePostEvent, LivingDamagePreEvent,
    LivingIncomingDamageEvent, Reduction,
};
use std::sync::{Arc, Mutex};

/// What the probe heard: Pre's entity, original damage, new damage and armor's reduction, and
/// Post's health damage.
#[derive(Debug, Default, PartialEq)]
struct Heard {
    pre: Vec<(i32, f32, f32, f32)>,
    post: Vec<(i32, f32)>,
    side_effects: Vec<(i32, bool)>,
}

/// The test mod: 225001's incoming hit becomes 4, Pre sets 225003's and 225008's damage to 2,
/// and draws once from the level's random for 225010.
struct Probe(Arc<Mutex<Heard>>);

#[mod_api::__subscriptions]
impl Hooks for Probe {
    fn living_incoming_damage(
        &self,
        _: &mut dyn LevelRandomHost,
        event: &mut LivingIncomingDamageEvent<'_>,
    ) -> mod_api::Flow {
        if event.entity == 225001 {
            event.container.set_new_damage(4.0);
        }
        mod_api::Flow::Continue
    }
    fn living_damage_pre(
        &self,
        level: &mut dyn LevelRandomHost,
        event: &mut LivingDamagePreEvent<'_>,
    ) {
        let container = &mut *event.container;
        if matches!(event.entity, 225003 | 225008) {
            container.set_new_damage(2.0);
        }
        if event.entity == 225010 {
            level.with_random(|random| random.next_int()).unwrap();
        }
        let heard = (
            event.entity,
            container.original(),
            container.new_damage(),
            container.reduction(Reduction::Armor),
        );
        self.0.lock().unwrap().pre.push(heard);
    }
    fn living_damage_post(
        &self,
        _: &mut dyn LevelRandomHost,
        event: &mut LivingDamagePostEvent<'_>,
    ) {
        let heard = (event.entity, event.container.health_damage());
        let mut log = self.0.lock().unwrap();
        log.post.push(heard);
        log.side_effects
            .push((event.entity, event.container.should_cause_side_effects()));
    }
}

/// Runs `test` in a game with the probe installed, and returns what it heard.
fn heard(test: impl FnOnce(&mut super::Game<'_>)) -> Heard {
    let log = Arc::new(Mutex::new(Heard::default()));
    let mods: mod_api::Mods = vec![mod_api::ModEntry::new(
        "probe",
        Box::new(Probe(log.clone())),
    )]
    .into();
    mod_api::testing::with_mods(mods, || with_game(test));
    std::mem::take(&mut *log.lock().unwrap())
}

fn mob(game: &super::Game<'_>, id: u32, kind: &str) -> Mob {
    let kind = game.res.mob_types().get(kind).unwrap().clone();
    Mob::new(id, kind, Vec3::ZERO, 0.0)
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn the_original_damage_is_the_hit_before_the_incoming_hook() {
    let heard = heard(|game| {
        let mut pig = mob(game, 225001, "minecraft:pig");
        assert!(pig.damage(
            damage_level_of!(game),
            10.0,
            Damage::of(DamageKind::Generic)
        ));
        assert_eq!(pig.health, 6.0);
    });
    assert_eq!(heard.pre, [(225001, 10.0, 4.0, 0.0)]);
    assert_eq!(heard.post, [(225001, 4.0)]);
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn pre_sets_the_health_lost_and_the_last_hurt_the_cooldown_compares() {
    let heard = heard(|game| {
        let mut pig = mob(game, 225003, "minecraft:pig");
        assert!(pig.damage(damage_level_of!(game), 6.0, Damage::of(DamageKind::Generic)));
        assert_eq!(pig.health, 8.0);
        // NeoForge bubbles the inflicted damage up to `lastHurt`.
        assert_eq!(pig.last_hurt, 2.0);
    });
    assert_eq!(heard.pre, [(225003, 6.0, 2.0, 0.0)]);
    assert_eq!(heard.post, [(225003, 2.0)]);
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn pre_fires_for_a_hit_its_reductions_took_to_zero() {
    let heard = heard(|game| {
        let mut witch = mob(game, 225008, "minecraft:witch");
        // Witch.getDamageAfterMagicAbsorb: none from its own doing.
        let own = Damage::mob(225008, [2.0, 0.0, 2.0]);
        assert!(witch.damage(damage_level_of!(game), 6.0, own));
        assert_eq!(witch.health, 24.0);
    });
    assert_eq!(heard.pre, [(225008, 6.0, 2.0, 0.0)]);
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn pre_draws_from_the_level_random_of_the_hit() {
    heard(|game| {
        let mut expected = game.world.random.clone();
        expected.next_int();
        let mut pig = mob(game, 225010, "minecraft:pig");
        assert!(pig.damage(damage_level_of!(game), 1.0, Damage::of(DamageKind::Generic)));
        assert_eq!(game.world.random.next_int(), expected.next_int());
    });
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_hit_on_the_dragon_fires_pre_and_post() {
    let heard = heard(|game| {
        let mut dragon = Dragon::new(Vec3::ZERO, 0.0, 0);
        dragon.id = 225009;
        game.dragon = Some(dragon);
        let _ = game.hit_special(SPECIAL, 6.0, true, DamageKind::Generic, None);
        assert_eq!(game.dragon.as_ref().unwrap().health, 194.0);
    });
    assert_eq!(heard.pre, [(225009, 6.0, 6.0, 0.0)]);
    assert_eq!(heard.post, [(225009, 6.0)]);
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn mob_and_dragon_post_records_fresh_hit_decision() {
    let heard = heard(|game| {
        let mut pig = mob(game, 225014, "minecraft:pig");
        let source = Damage::of(DamageKind::Generic);
        assert!(pig.damage(damage_level_of!(game), 2.0, source));
        assert!(pig.damage(damage_level_of!(game), 4.0, source));
        assert!(!pig.damage(damage_level_of!(game), 3.0, source));
        assert_eq!(pig.health, 6.0);
        let mut dragon = Dragon::new(Vec3::ZERO, 0.0, 0);
        dragon.id = 225015;
        game.dragon = Some(dragon);
        for amount in [2.0, 4.0, 3.0] {
            let _ = game.hit_special(SPECIAL, amount, true, DamageKind::Generic, None);
        }
        assert_eq!(game.dragon.as_ref().unwrap().health, 196.0);
    });
    assert_eq!(
        heard.post,
        [(225014, 2.0), (225014, 2.0), (225015, 2.0), (225015, 2.0)]
    );
    assert_eq!(
        heard.side_effects,
        [
            (225014, true),
            (225014, false),
            (225015, true),
            (225015, false)
        ]
    );
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn the_container_names_what_armor_took() {
    let heard = heard(|game| {
        let mut zombie = mob(game, 225011, "minecraft:zombie");
        // A mob's attack: armor takes its share, where a generic hit bypasses it.
        let attack = Damage::mob(225012, [2.0, 0.0, 2.0]);
        assert!(zombie.damage(damage_level_of!(game), 10.0, attack));
    });
    // A zombie's two armor points against 10 damage: `CombatRules.getDamageAfterAbsorb`
    // clamps 2 - 10 / 2 up to a fifth of the armor, 0.4, and keeps 1 - 0.4 / 25 of the hit.
    let after = 10.0 * (1.0 - 0.4_f32 / 25.0);
    assert_eq!(heard.pre, [(225011, 10.0, after, 10.0 - after)]);
}

/// A level built the normal way lends the game's hooks to its hits: a path that built it
/// without them would run no mod's damage hook.
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_game_built_level_has_every_damage_hook() {
    with_game(|game| {
        let hooks = &game.damage_hooks;
        assert!(hooks.incoming.is_some() && hooks.pre.is_some() && hooks.post.is_some());
        assert!(hooks.knockback.is_some() && hooks.totem.is_some() && hooks.shield.is_some());
    });
}
