//! Existing non-Mob save behavior, mechanically extracted before owner-local adaptation.
use super::{Game, commands::Target};
use horizon_client::{entity_nbt, save::random_uuid};
use horizon_core::nbt::{JavaString, Tag};

pub(in crate::game) fn set_key(tag: &mut Tag, key: &str, value: Tag) {
    if let Tag::Compound(c) = tag {
        c.insert(JavaString::from(key), value);
    }
}

impl Game<'_> {
    pub(super) fn non_mob_nbt_in_level(&mut self, t: Target) -> Option<Tag> {
        let player_uuid = self.saved_player_uuid();
        Some(match t {
            Target::Item(id) => {
                let uuid = self.uuid_for(id);
                let e = self.dropped.iter().find(|e| e.id == id)?;
                entity_nbt::item_entity_to_nbt_with_items(
                    e,
                    uuid,
                    self.persist.raw.get(&id),
                    self.items(),
                )
            }
            Target::Stand(id) => {
                let uuid = self.uuid_for(id);
                let a = self.armor_stands.iter().find(|a| a.id == id)?;
                entity_nbt::armor_stand_to_nbt(a, uuid, self.persist.raw.get(&id))
            }
            Target::Frame(id) => {
                let uuid = self.uuid_for(id);
                let f = self.item_frames.iter().find(|f| f.id == id)?;
                entity_nbt::item_frame_to_nbt(f, uuid, self.persist.raw.get(&id))
            }
            Target::Painting(id) => {
                let uuid = self.uuid_for(id);
                let p = self.paintings.iter().find(|p| p.id == id)?;
                entity_nbt::painting_to_nbt(p, uuid, self.persist.raw.get(&id))
            }
            Target::Boat(id) => {
                let uuid = self.uuid_for(id);
                let i = self.boats.iter().position(|b| b.id == id)?;
                let b = &self.boats[i];
                let mut tag = b.to_nbt(uuid, self.persist.raw.get(&id));
                // Entity.saveWithoutId: the actual runtime passenger tree.
                self.put_mob_passengers(&mut tag, id);
                // AbstractBoat.addAdditionalSaveData: the live or pending leash.
                self.write_boat_leash(&mut tag, i);
                tag
            }
            Target::Cart(id) => {
                let uuid = self.uuid_for(id);
                let c = self.carts.iter().find(|c| c.id == id)?;
                let mut tag = c.to_nbt(uuid, self.persist.raw.get(&id));
                self.put_mob_passengers(&mut tag, id);
                tag
            }
            Target::Fangs(id) => {
                let uuid = self.uuid_for(id);
                let f = self.fangs.list.iter().find(|f| f.id == id)?;
                self.fangs_to_nbt(f, uuid)
            }
            Target::Other(id) => {
                let uuid = self.uuid_for(id);
                let o = self.others.iter().find(|o| o.id == id)?.clone();
                self.other_to_nbt(&o, uuid)
            }
            Target::Orb(id) => {
                let uuid = self.uuid_for(id);
                let tag = entity_nbt::orb_to_nbt(self.orbs.iter().find(|o| o.id == id)?, uuid);
                crate::game::persist::with_raw(tag, self.persist.raw.get(&id))
            }
            Target::Projectile(id) => {
                let uuid = self.uuid_for(id);
                // A homing bullet's `Target` is the entity's, not the raw
                // compound's, as `save_projectile` writes it.
                let who = self
                    .creatures
                    .projectiles
                    .iter()
                    .find(|p| p.id == id)
                    .and_then(|p| match &p.kind {
                        horizon_client::projectiles::ProjectileKind::ShulkerBullet {
                            target: Some(who),
                            ..
                        } => Some(*who),
                        _ => None,
                    });
                let target = who.and_then(|who| self.save_who_uuid(who));
                let p = self.creatures.projectiles.iter().find(|p| p.id == id)?;
                let owner = if p.owner == Some(horizon_client::mobs::Who::Player) {
                    player_uuid
                } else {
                    None
                };
                let in_block = p
                    .arrow
                    .as_ref()
                    .and_then(|a| a.last_state)
                    .map(|s| self.state_to_nbt(s));
                let raw = self.persist.raw.get(&id);
                // Projectile.addAdditionalSaveData stores the retained EntityReference,
                // including an owner that is not currently resolved to a live entity.
                let mut tag =
                    entity_nbt::arrow_to_nbt(p, uuid, p.owner_uuid.or(owner), in_block, raw)
                        .or_else(|| entity_nbt::fireball_to_nbt(p, uuid, owner, raw))
                        .or_else(|| entity_nbt::throwable_to_nbt(p, uuid, owner, raw))
                        .or_else(|| {
                            entity_nbt::spit_or_bullet_to_nbt(p, uuid, owner, target, raw)
                        })?;
                self.save_projectile_portal_cooldown(p, &mut tag);
                tag
            }
            Target::Crystal(id) => {
                let c = self.crystals.iter().find(|c| c.id == id)?;
                let raw = self.persist_anon("minecraft:end_crystal", c.position);
                let uuid = self.persist.uuids.get(&id).copied();
                let tag = c.to_nbt(uuid.unwrap_or_else(random_uuid), raw);
                crate::game::persist::with_raw(tag, self.persist.raw.get(&id))
            }
            Target::Dragon(id) => {
                let d = self.dragon.as_ref().filter(|d| d.id == id)?;
                let raw = self.persist_anon("minecraft:ender_dragon", d.position);
                let uuid = self.persist.uuids.get(&id).copied();
                let tag = d.to_nbt(uuid.unwrap_or_else(random_uuid), raw);
                crate::game::persist::with_raw(tag, self.persist.raw.get(&id))
            }
            Target::Tnt(id) => {
                let i = self.tnts.iter().position(|t| t.id == id)?;
                self.tnt_nbt(i)
            }
            Target::Cushion(id) => {
                let uuid = self.uuid_for(id);
                let c = self.cushions.iter().find(|c| c.id == id)?;
                entity_nbt::cushion_to_nbt(c, uuid, self.persist.raw.get(&id))
            }
            Target::Falling(id) => {
                let f = self.falling.iter().find(|f| f.id == id)?.clone();
                let (uuid, raw) = match &f.saved {
                    Some(saved) => (saved.0, Some(&saved.1)),
                    None => (self.uuid_for(id), None),
                };
                let tag = entity_nbt::falling_block_to_nbt(
                    self.state_to_nbt(f.state),
                    f.position,
                    f.velocity,
                    f.age,
                    f.drop_item,
                    f.damage,
                    f.fall_distance,
                    f.on_ground,
                    f.rotation,
                    uuid,
                );
                crate::game::persist::with_raw(tag, raw)
            }
            Target::Rocket(id) => {
                let uuid = self.uuid_for(id);
                let r = self.rockets.iter().find(|r| r.id == id)?;
                r.to_nbt(uuid, player_uuid, self.persist.raw.get(&id))
            }
            Target::Cloud(id) => {
                let uuid = self.uuid_for(id);
                let c = self.breath_clouds.iter().find(|c| c.id == id)?;
                let [x, y, z] = c.at;
                // Read the live cloud, including its current radius and age;
                // the raw compound alone does not contain generated clouds.
                let mut tag = self.other_to_nbt(
                    &super::commands::OtherEntity {
                        attachment_lifetime: Default::default(),
                        join: Default::default(),
                        id,
                        kind: "minecraft:area_effect_cloud".into(),
                        item_transformation: None,
                        interaction: None,
                        knot_check: None,
                        eye: None,
                        position: horizon_core::geometry::Vec3::new(x, y, z),
                        yaw: 0.0,
                        pitch: 0.0,
                    },
                    uuid,
                );
                // Entity.saveWithoutId's common fields, also for a cloud never saved.
                crate::game::persist::write_static_entity_fields(&mut tag);
                set_key(&mut tag, "Age", Tag::Int(c.age));
                set_key(&mut tag, "Duration", Tag::Int(c.duration));
                set_key(&mut tag, "Radius", Tag::Float(c.radius.to_bits()));
                let wait = c.potion.as_ref().map_or(20, |p| p.settings.wait_time);
                set_key(&mut tag, "WaitTime", Tag::Int(wait));
                if let Some(p) = &c.potion {
                    set_key(
                        &mut tag,
                        "ReapplicationDelay",
                        Tag::Int(p.settings.reapplication_delay),
                    );
                    set_key(
                        &mut tag,
                        "DurationOnUse",
                        Tag::Int(p.settings.duration_on_use),
                    );
                    set_key(
                        &mut tag,
                        "RadiusOnUse",
                        Tag::Float(p.settings.radius_on_use.to_bits()),
                    );
                    set_key(
                        &mut tag,
                        "RadiusPerTick",
                        Tag::Float(p.radius_per_tick.to_bits()),
                    );
                    if p.settings.duration_scale != 1.0 {
                        set_key(
                            &mut tag,
                            "potion_duration_scale",
                            Tag::Float(p.settings.duration_scale.to_bits()),
                        );
                    } else if let Tag::Compound(c) = &mut tag {
                        c.remove(&"potion_duration_scale".into());
                    }
                }
                tag
            }
            Target::Trident(id) => {
                let uuid = self.uuid_for(id);
                let t = self.tridents.iter().find(|t| t.id == id)?;
                t.to_nbt(uuid, self.pack.assets.registry, self.persist.raw.get(&id))
            }
            Target::Spawner(id) => {
                let s = self.ominous_spawners.iter().find(|s| s.id == id)?;
                let [x, y, z] = s.position;
                entity_nbt::ominous_item_spawner_to_nbt(
                    &s.item,
                    horizon_core::geometry::Vec3::new(x, y, z),
                    s.spawn_item_after,
                    s.uuid,
                    s.raw.as_deref(),
                )
            }
            // LeashFenceKnotEntity saves nothing of its own: the common
            // entity fields, as an explicitly summoned knot's.
            Target::Knot(id) => {
                let fence = self.natural_knot(id)?;
                let uuid = *self.persist.uuids.get(&id)?;
                let knot = super::commands::OtherEntity {
                    attachment_lifetime: Default::default(),
                    // A serialization view, not a second resident entity.
                    join: horizon_client::join_seq::JoinSeq::UNJOINED,
                    id,
                    kind: "minecraft:leash_knot".into(),
                    position: horizon_client::mobs::leash::knot_state(fence).position,
                    yaw: 0.0,
                    pitch: 0.0,
                    item_transformation: None,
                    interaction: None,
                    knot_check: None,
                    eye: None,
                };
                let mut tag = self.other_to_nbt(&knot, uuid);
                // Entity.saveWithoutId writes `remainingFireTicks` always; the
                // knot's never changes (BlockAttachedEntity.tick has no
                // baseTick).
                set_key(&mut tag, "Fire", Tag::Short(0));
                tag
            }
            _ => return None,
        })
    }
}
