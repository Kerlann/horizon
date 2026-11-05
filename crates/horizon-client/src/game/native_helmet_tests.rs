//! Head-only ArmorHurtEvent decisions through the real player and mob damage adapters.
use super::entity::damage_level_of;
use super::native_entity_refs::tests::with_game;
use horizon_client::{
    items::ItemStack,
    mobs::{Damage, DamageKind, Mob},
    motion::PlayerMotion,
};
use horizon_core::geometry::Vec3;
use mod_api::{
    ArmorHurtEvent, EquipmentSlot, Flow, Hooks, LevelRandomHost, LivingIncomingDamageEvent,
};
use std::sync::{Arc, Mutex};

#[derive(Debug, PartialEq)]
struct Wear {
    entity: u32,
    player: Option<u128>,
    slot: EquipmentSlot,
    damage: f32,
}
struct Probe(Arc<Mutex<Vec<Wear>>>);
#[mod_api::__subscriptions]
impl Hooks for Probe {
    fn living_incoming_damage(
        &self,
        _: &mut dyn LevelRandomHost,
        event: &mut LivingIncomingDamageEvent<'_>,
    ) -> Flow {
        if event.entity == 821005 {
            Flow::Cancel
        } else {
            Flow::Continue
        }
    }
    fn armor_hurt(&self, _: &mut dyn LevelRandomHost, event: &mut ArmorHurtEvent<'_>) -> Flow {
        for entry in event.entries.iter_mut() {
            self.0.lock().unwrap().push(Wear {
                entity: event.entity,
                player: event.player,
                slot: entry.slot,
                damage: entry.damage,
            });
            if event.entity == 821002 || event.entity == 821004 {
                entry.new_damage = 7.0;
            }
        }
        if event.entity == 821001 || event.entity == 821003 {
            Flow::Cancel
        } else {
            Flow::Continue
        }
    }
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn falling_helmet_wear_honors_cancel_edits_guards_and_cooldown_ordering() {
    let heard = Arc::new(Mutex::new(Vec::new()));
    let mods: mod_api::Mods = vec![mod_api::ModEntry::new(
        "helmet_probe",
        Box::new(Probe(heard.clone())),
    )]
    .into();
    mod_api::testing::with_mods(mods, || {
        with_game(|game| {
            let source = game
                .health_resources
                .damage
                .id("minecraft:falling_anvil")
                .unwrap();
            for id in [
                821001, 821002, 821005, 821006, 821007, 821008, 821009, 821010, 821011, 821012,
            ] {
                let mut player = PlayerMotion::new(game.motion, Vec3::ZERO).unwrap();
                player.entity_id = id;
                player.client_loaded = true;
                game.player = Some(player);
                game.creative = false;
                game.lan_acting_uuid = Some(821);
                game.armor[0] = (id != 821006).then(|| ItemStack::new("minecraft:iron_helmet", 1));
                let helmet_max = game
                    .res
                    .items()
                    .get("minecraft:iron_helmet")
                    .unwrap()
                    .max_damage;
                if id == 821008 {
                    game.armor[0].as_mut().unwrap().damage = helmet_max - 1;
                }
                if matches!(id, 821010 | 821012) {
                    game.armor[0].as_mut().unwrap().damage = helmet_max - 2;
                }
                if id == 821011 {
                    game.armor[0]
                        .as_mut()
                        .unwrap()
                        .data_mut()
                        .set_other("!minecraft:damage", Some(horizon_world::nbt::Tag::Byte(1)));
                }
                if id == 821012 {
                    game.armor[0]
                        .as_mut()
                        .unwrap()
                        .enchant("minecraft:protection", 4);
                }
                game.apply_equipment();
                if id == 821012 {
                    game.apply_effect_status();
                    let protection: f32 = game
                        .player
                        .as_ref()
                        .unwrap()
                        .status
                        .protection
                        .iter()
                        .map(|(_, value)| *value)
                        .sum();
                    assert!(protection > 0.0);
                }
                if id == 821007 {
                    game.player.as_mut().unwrap().invulnerable = true;
                }
                let rules = game.world.health_rules;
                let hurt = if id == 821001 {
                    game.falling_hurt_player(Vec3::ZERO, 8.0, "minecraft:falling_anvil");
                    game.player.as_ref().unwrap().hurt_log.last().unwrap().hurt
                } else {
                    game.player
                        .as_mut()
                        .unwrap()
                        .hurt(
                            damage_level_of!(game),
                            &game.motion.health,
                            rules,
                            source,
                            8.0,
                        )
                        .unwrap()
                };
                if id == 821009 {
                    game.armor[0] = Some(ItemStack::new("minecraft:diamond_helmet", 1));
                }
                if id == 821002 {
                    // Both decisions precede one write-back; the cooldown refuses health,
                    // not hurtHelmet. The first wear must not invalidate the second.
                    assert!(
                        !game
                            .player
                            .as_mut()
                            .unwrap()
                            .hurt(
                                damage_level_of!(game),
                                &game.motion.health,
                                rules,
                                source,
                                8.0
                            )
                            .unwrap()
                    );
                }
                game.wear_armor(&[]);
                match id {
                    821001 => {
                        assert!(hurt);
                        assert_eq!(game.armor[0].as_ref().unwrap().damage, 0);
                        assert_eq!(game.player.as_ref().unwrap().health.hurt.last_hurt, 6.0);
                    }
                    821002 => {
                        assert!(hurt);
                        assert_eq!(game.armor[0].as_ref().unwrap().damage, 14);
                        assert_eq!(
                            game.player
                                .as_ref()
                                .unwrap()
                                .helmet
                                .as_ref()
                                .unwrap()
                                .damage,
                            14
                        );
                    }
                    821008 => {
                        assert!(hurt);
                        assert!(game.armor[0].is_none());
                    }
                    821009 => {
                        assert!(hurt);
                        assert_eq!(game.armor[0].as_ref().unwrap().damage, 0);
                    }
                    821010 | 821012 => {
                        // Two points of wear break the helmet before armor and
                        // protection reduce this 8-point hit: 6 health, not 5.904.
                        assert!(hurt);
                        assert!(game.armor[0].is_none());
                        let record = game.player.as_ref().unwrap().hurt_log.last().unwrap();
                        assert_eq!(record.armor, 0.0);
                        assert_eq!(record.taken, 6.0);
                    }
                    821011 => {
                        assert!(hurt);
                        assert_eq!(game.armor[0].as_ref().unwrap().damage, 0);
                    }
                    821006 => {
                        assert!(hurt);
                        assert!(game.armor[0].is_none());
                    }
                    _ => {
                        assert!(!hurt);
                        assert_eq!(game.armor[0].as_ref().unwrap().damage, 0);
                    }
                }
            }
            for id in [821003, 821004] {
                let kind = game
                    .res
                    .mob_types()
                    .get("minecraft:zombie")
                    .unwrap()
                    .clone();
                let mut mob = Mob::new(id, kind, Vec3::ZERO, 0.0);
                mob.set_equipment_stack(
                    EquipmentSlot::Head as usize,
                    Some(ItemStack::new("minecraft:iron_helmet", 1)),
                );
                assert!(mob.damage(
                    damage_level_of!(game),
                    8.0,
                    Damage::of(DamageKind::FallingAnvil)
                ));
                if id == 821004 {
                    assert!(!mob.damage(
                        damage_level_of!(game),
                        8.0,
                        Damage::of(DamageKind::FallingAnvil)
                    ));
                }
                let events = std::mem::take(&mut mob.outbox);
                game.mobs.push(mob);
                game.handle_mob_events(events).unwrap();
                let mob = game.mobs.iter().find(|m| m.id == id).unwrap();
                assert_eq!(
                    mob.item_by_slot(EquipmentSlot::Head).unwrap().damage,
                    if id == 821003 { 0 } else { 14 }
                );
            }
        })
    });
    assert_eq!(
        *heard.lock().unwrap(),
        [
            821001, 821002, 821002, 821008, 821009, 821010, 821012, 821003, 821004, 821004
        ]
        .map(|entity| Wear {
            entity,
            player: (!matches!(entity, 821003 | 821004)).then_some(821),
            slot: EquipmentSlot::Head,
            damage: 2.0,
        })
    );
}
