//! Compact retained base-entity payload. Capture is supplied by the publication owner;
//! this module never resolves an entity, allocates an ID, or reads a lazy UUID.
//! Dynamic type names must be interned by the owner, not formatted during capture.
use mod_api::{
    EntityReadCapture, EntityReadRow, EntityRef, LeashHolder, LevelRef, MobCategory, SoundSource,
};
use std::sync::Arc;

/// Parts are bounded by the dragon's eight parts. `None` remains distinct from
/// a known empty collection. Only explicit owned materialization allocates a Vec.
#[derive(Clone, Debug)]
pub(super) struct BaseReadParts {
    entries: [Option<EntityRef>; 8],
    len: u8,
}

impl BaseReadParts {
    pub(super) fn new(parts: &[EntityRef]) -> Option<Self> {
        if parts.len() > 8 {
            return None;
        }
        let mut entries = [None; 8];
        for (slot, part) in entries.iter_mut().zip(parts) {
            *slot = Some(*part);
        }
        Some(Self {
            entries,
            len: parts.len() as u8,
        })
    }
    pub(super) fn materialize(&self) -> Vec<EntityRef> {
        self.entries[..usize::from(self.len)]
            .iter()
            .flatten()
            .copied()
            .collect()
    }
}

/// Every stored value has the scalar API's domain/unknown semantics. No default
/// constructor: a source adapter must explicitly populate every nullable field.
#[derive(Clone, Debug)]
pub(super) struct CompactEntityReadRow {
    pub(super) custom_name_visible: Option<bool>,
    pub(super) classification: Option<MobCategory>,
    pub(super) added_to_level: Option<bool>,
    pub(super) position: [f64; 3],
    pub(super) rotation: [f32; 2],
    pub(super) kind: Option<Arc<str>>,
    pub(super) level: LevelRef,
    pub(super) permanently_invulnerable: Option<bool>,
    pub(super) invulnerable_time: Option<i32>,
    pub(super) damage_cooldown_time: Option<i32>,
    pub(super) invulnerable_to_piercing_weapon: Option<bool>,
    pub(super) velocity: [f64; 3],
    pub(super) on_ground: bool,
    pub(super) bounding_box: [[f64; 3]; 2],
    pub(super) eye_height: f64,
    pub(super) dimensions: [f64; 2],
    pub(super) is_multipart: Option<bool>,
    pub(super) parts: Option<BaseReadParts>,
    pub(super) head_yaw: Option<f32>,
    pub(super) direction: Option<&'static str>,
    pub(super) no_physics: Option<bool>,
    pub(super) leash_holder: Option<LeashHolder>,
    pub(super) sound_source: Option<SoundSource>,
    pub(super) spectator: Option<bool>,
    pub(super) is_shift_key_down: Option<bool>,
    pub(super) is_crouching: Option<bool>,
    pub(super) is_sprinting: Option<bool>,
    pub(super) is_in_water: Option<bool>,
    pub(super) is_in_lava: Option<bool>,
    pub(super) fire_immune: Option<bool>,
    pub(super) fire_ticks: Option<i32>,
    pub(super) portal_cooldown: Option<i32>,
    pub(super) air_supply: Option<i32>,
    pub(super) max_air_supply: Option<i32>,
    pub(super) is_silent: Option<bool>,
    pub(super) glowing: Option<bool>,
    pub(super) no_gravity: Option<bool>,
    pub(super) ticks_frozen: Option<i32>,
    pub(super) tick_count: Option<i32>,
    pub(super) is_invisible: Option<bool>,
    pub(super) pose: Option<&'static str>,
}

impl CompactEntityReadRow {
    /// Expensive owned API form is built only on explicit request. Publication
    /// identity and generation are owned by the cache, not duplicated here.
    pub(super) fn materialize(
        &self,
        capture: EntityReadCapture,
        entity: EntityRef,
        join_generation: u64,
    ) -> EntityReadRow {
        let [x, y, z] = self.position;
        let [yaw, pitch] = self.rotation;
        let view = horizon_world::view_direction::view_vector(pitch, yaw);
        let up = horizon_world::view_direction::view_vector(pitch - 90.0, yaw);
        EntityReadRow {
            capture,
            entity,
            join_generation,
            custom_name_visible: self.custom_name_visible,
            classification: self.classification,
            added_to_level: self.added_to_level,
            position: self.position,
            block_position: self.position.map(|v| v.floor() as i32),
            rotation: self.rotation,
            kind: self.kind.as_deref().map(str::to_owned),
            level: self.level,
            permanently_invulnerable: self.permanently_invulnerable,
            invulnerable_time: self.invulnerable_time,
            damage_cooldown_time: self.damage_cooldown_time,
            invulnerable_to_piercing_weapon: self.invulnerable_to_piercing_weapon,
            velocity: self.velocity,
            on_ground: self.on_ground,
            bounding_box: self.bounding_box,
            eye_height: self.eye_height,
            eye_position: [x, y + self.eye_height, z],
            view_vector: [view.x, view.y, view.z],
            up_vector: [up.x, up.y, up.z],
            is_multipart: self.is_multipart,
            parts: self.parts.as_ref().map(BaseReadParts::materialize),
            head_yaw: self.head_yaw,
            direction: self.direction.map(str::to_owned),
            dimensions: self.dimensions,
            no_physics: self.no_physics,
            leash_holder: self.leash_holder,
            sound_source: self.sound_source,
            spectator: self.spectator,
            is_shift_key_down: self.is_shift_key_down,
            is_crouching: self.is_crouching,
            is_sprinting: self.is_sprinting,
            is_in_water: self.is_in_water,
            is_in_lava: self.is_in_lava,
            fire_immune: self.fire_immune,
            fire_ticks: self.fire_ticks,
            portal_cooldown: self.portal_cooldown,
            air_supply: self.air_supply,
            max_air_supply: self.max_air_supply,
            is_silent: self.is_silent,
            glowing: self.glowing,
            no_gravity: self.no_gravity,
            ticks_frozen: self.ticks_frozen,
            tick_count: self.tick_count,
            is_invisible: self.is_invisible,
            pose: self.pose.map(str::to_owned),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn direct_saved_source_preserves_domains_and_materializes_geometry() {
        let epoch = mod_api::WorldEpoch::new(7);
        let raw = horizon_core::nbt::Tag::compound([(
            "CustomNameVisible",
            horizon_core::nbt::Tag::Byte(2),
        )]);
        let other = super::super::commands::OtherEntity {
            attachment_lifetime: Default::default(),
            join: horizon_client::join_seq::JoinSeq::next(),
            id: 17,
            kind: "minecraft:marker".into(),
            position: horizon_core::geometry::Vec3::ZERO,
            yaw: 0.0,
            pitch: 0.0,
            item_transformation: None,
            interaction: None,
            knot_check: None,
            eye: None,
        };
        let compact = CompactEntityReadRow::capture(
            BaseReadSource::Other(&other),
            BaseReadGeometry {
                position: [-0.1, 12.75, 3.0],
                rotation: [0.0, 0.0],
                velocity: [0.0; 3],
                on_ground: false,
                bounding_box: [[-0.6, 12.75, 2.5], [0.4, 14.75, 3.5]],
                eye_height: 1.5,
                dimensions: [1.0, 2.0],
            },
            BaseReadContext {
                level: LevelRef::new(epoch, mod_api::DimensionKey::OVERWORLD),
                logical_server: true,
                kind: Arc::from("minecraft:marker"),
                saved: Some(&raw),
                added_to_level: Some(true),
                portal_cooldown: None,
                leash_holder: None,
                parts: BaseReadParts::new(&[]),
                head_yaw: 0.0,
                spectator: false,
                invisible: None,
                client_shift: None,
            },
        );
        let capture = EntityReadCapture {
            tick: mod_api::TickId::new(3),
            pass: 2,
            revision: 1,
            phase: mod_api::EntityReadCapturePhase::BeforeCallback,
            pending_overlay: None,
        };
        let row = compact.materialize(capture, EntityRef::new(epoch, 17), 9);
        assert_eq!(row.block_position, [-1, 12, 3]);
        assert_eq!(row.eye_position, [-0.1, 14.25, 3.0]);
        assert_eq!(row.dimensions, [1.0, 2.0]);
        assert_eq!(row.custom_name_visible, Some(true));
        assert_eq!(row.permanently_invulnerable, None);
        assert_eq!(row.is_sprinting, Some(false));
        assert_eq!(row.is_in_water, None);
        assert_eq!(row.max_air_supply, Some(300));
        assert_eq!(row.pose.as_deref(), Some("standing"));
        assert_eq!(row.direction.as_deref(), Some("south"));
        assert_eq!(row.parts, Some(vec![]));
        assert_eq!(row.join_generation, 9);
        assert_eq!(row.capture, capture);
        assert_eq!(row.kind.as_deref(), Some("minecraft:marker"));
    }

    #[test]
    fn part_storage_preserves_unknown_empty_and_bounded_domains() {
        let empty = BaseReadParts::new(&[]).unwrap();
        assert!(empty.materialize().is_empty());
        let entity = EntityRef::new(mod_api::WorldEpoch::new(1), 17);
        let eight = [entity; 8];
        assert_eq!(BaseReadParts::new(&eight).unwrap().materialize(), eight);
        assert!(BaseReadParts::new(&[entity; 9]).is_none());
    }
}

/// Borrowed resident source; variants do not contain IDs or list positions.
#[derive(Clone, Copy)]
pub(super) enum BaseReadSource<'a> {
    Mob(&'a horizon_client::mobs::Mob),
    Item(&'a horizon_client::entities::ItemEntity),
    Orb(&'a horizon_client::entities::ExperienceOrb),
    Stand(&'a horizon_client::entities::ArmorStand),
    Boat(&'a super::boats::Boat),
    Cart(&'a super::minecarts::Minecart),
    Projectile(&'a horizon_client::projectiles::Projectile),
    Tnt(&'a super::tnt::PrimedTnt),
    Falling(&'a super::FallingBlock),
    Rocket(&'a super::fireworks::Rocket),
    Trident(&'a super::trident::ThrownTrident),
    Frame(&'a horizon_client::entities::ItemFrame),
    Painting(&'a horizon_client::entities::Painting),
    Cushion(&'a horizon_client::entities::Cushion),
    Dragon(&'a super::dragon::Dragon),
    DragonPart(&'a super::dragon::Dragon, usize),
    Crystal(&'a super::dragon::Crystal),
    Fangs(&'a super::fangs::Fangs),
    Spawner(&'a super::ominous_item_spawner::OminousItemSpawner),
    Cloud(&'a super::dragon::Breath),
    Other(&'a super::commands::OtherEntity),
    Hook(&'a super::fishing::Hook),
    Knot(&'a super::leads::Knot),
    Player(&'a horizon_client::motion::PlayerMotion),
    Guest(&'a horizon_client::lan::guest::Guest),
}

/// Shared geometry captured from a borrowed body, never by resolving a Target.
pub(super) struct BaseReadGeometry {
    pub(super) position: [f64; 3],
    pub(super) rotation: [f32; 2],
    pub(super) velocity: [f64; 3],
    pub(super) on_ground: bool,
    pub(super) bounding_box: [[f64; 3]; 2],
    pub(super) eye_height: f64,
    pub(super) dimensions: [f64; 2],
}
impl BaseReadGeometry {
    pub(super) fn from_entity<E: super::entity::Entity>(
        e: &E,
        eye_height: f64,
        dimensions: [f64; 2],
    ) -> Self {
        let p = e.position();
        let v = e.velocity();
        let b = e.bounds();
        let lo = b.min_position();
        let hi = b.max_position();
        Self {
            position: [p.x, p.y, p.z],
            rotation: e.rotation(),
            velocity: [v.x, v.y, v.z],
            on_ground: e.on_ground(),
            bounding_box: [[lo.x, lo.y, lo.z], [hi.x, hi.y, hi.z]],
            eye_height,
            dimensions,
        }
    }
}

/// Publication owner supplies already validated/indexed contextual values. These
/// are level-global state, not values reconstructed via scalar entity getters.
pub(super) struct BaseReadContext<'a> {
    pub(super) level: LevelRef,
    pub(super) logical_server: bool,
    pub(super) kind: Arc<str>,
    pub(super) saved: Option<&'a horizon_core::nbt::Tag>,
    pub(super) added_to_level: Option<bool>,
    pub(super) portal_cooldown: Option<i32>,
    pub(super) leash_holder: Option<LeashHolder>,
    pub(super) parts: Option<BaseReadParts>,
    pub(super) head_yaw: f32,
    pub(super) spectator: bool,
    pub(super) invisible: Option<bool>,
    pub(super) client_shift: Option<bool>,
}

impl CompactEntityReadRow {
    /// Captures directly from one borrowed owner. The caller validates identity
    /// once and supplies geometry and indexed level state for that same owner.
    pub(super) fn capture(
        source: BaseReadSource<'_>,
        geometry: BaseReadGeometry,
        context: BaseReadContext<'_>,
    ) -> Self {
        use BaseReadSource::*;
        use horizon_client::mobs::{Species, sync};
        use horizon_client::pose::Pose;
        use horizon_core::nbt::Tag;
        let mob = if let Mob(m) = source { Some(m) } else { None };
        let player = match source {
            Player(p) => Some(p),
            Guest(g) => g.motion.as_deref(),
            _ => None,
        };
        let saved = match context.saved {
            Some(Tag::Compound(c)) => Some(c),
            _ => None,
        };
        static KEYS: std::sync::LazyLock<[horizon_core::nbt::JavaString; 3]> =
            std::sync::LazyLock::new(|| {
                [
                    "Invulnerable".into(),
                    "invulnerable_time".into(),
                    "CustomNameVisible".into(),
                ]
            });
        let flag =
            |index: usize| saved.is_some_and(|c| matches!(c.get(&KEYS[index]), Some(Tag::Byte(1))));
        let saved_window = saved
            .and_then(|c| match c.get(&KEYS[1]) {
                Some(Tag::Int(t)) => Some(*t),
                _ => None,
            })
            .unwrap_or(0);
        let saved_invulnerability = matches!(source, Stand(_) | Dragon(_) | Item(_));
        let permanently_invulnerable = match (mob, player) {
            (Some(m), _) => Some(m.invulnerable_flag),
            (_, Some(p)) => Some(p.health.permanently_invulnerable),
            _ => saved_invulnerability.then(|| flag(0)),
        };
        let invulnerable_time = match (mob, player) {
            (Some(m), _) => Some(m.invulnerable_time),
            (_, Some(p)) => Some(p.health.invulnerable_time),
            _ => saved_invulnerability.then_some(saved_window),
        };
        let pose = if let Some(m) = mob {
            super::native_entity_state::POSES
                .get(usize::from(sync::pose_id(m)))
                .copied()
        } else if let Some(p) = player {
            Some(match p.pose {
                Pose::Standing => "standing",
                Pose::Crouching => "crouching",
                Pose::Swimming => "swimming",
                Pose::FallFlying => "fall_flying",
                Pose::SpinAttack => "spin_attack",
                Pose::Sleeping => "sleeping",
            })
        } else if matches!(source, Guest(_)) {
            None
        } else {
            Some("standing")
        };
        let classification = Some(match mob.map(|m| m.category()) {
            Some("monster") => MobCategory::Monster,
            Some("creature") => MobCategory::Creature,
            Some("ambient") => MobCategory::Ambient,
            Some("axolotls") => MobCategory::Axolotls,
            Some("underground_water_creature") => MobCategory::UndergroundWaterCreature,
            Some("water_creature") => MobCategory::WaterCreature,
            Some("water_ambient") => MobCategory::WaterAmbient,
            _ if matches!(source, Dragon(_)) => MobCategory::Monster,
            _ => MobCategory::Misc,
        });
        let sound_source = Some(if let Some(m) = mob {
            if super::native_entity_sounds::hostile_species(m.species())
                || (m.species() == Species::Rabbit
                    && m.extra.variant == horizon_client::mobs::rabbit::EVIL)
            {
                SoundSource::Hostile
            } else {
                SoundSource::Neutral
            }
        } else {
            match source {
                Player(_) | Guest(_) => SoundSource::Players,
                Item(_) | Orb(_) => SoundSource::Ambient,
                Dragon(_) => SoundSource::Hostile,
                _ if context.kind.as_ref() == "minecraft:shulker_bullet" => SoundSource::Hostile,
                _ if context.kind.as_ref() == "minecraft:lightning_bolt" => SoundSource::Weather,
                _ => SoundSource::Neutral,
            }
        });
        let custom_name_visible = Some(match source {
            Mob(m) => m.name_visible,
            Stand(s) => s.name_visible,
            Frame(_) | Item(_) | Painting(_) | Boat(_) | Cart(_) | Other(_) | Fangs(_)
            | Cloud(_) => saved
                .and_then(|c| c.get(&KEYS[2]))
                .and_then(horizon_client::nbt_path::int_value)
                .is_some_and(|v| v as i8 != 0),
            _ => false,
        });
        let no_physics = match source {
            Player(p) => Some(p.spectator),
            Guest(g) => g.motion.as_deref().map(|p| p.spectator),
            Mob(_) => Some(false),
            Dragon(_) | Cloud(_) | Spawner(_) => Some(true),
            Projectile(p) => {
                if matches!(
                    p.kind,
                    horizon_client::projectiles::ProjectileKind::ShulkerBullet { .. }
                ) {
                    Some(true)
                } else {
                    p.arrow.as_ref().map(|a| a.no_physics)
                }
            }
            Trident(t) => Some(t.no_physics),
            _ => None,
        };
        let direction = super::native_entity_moves::facing(geometry.rotation[0]);
        let mut row = Self {
            custom_name_visible,
            classification,
            added_to_level: context.added_to_level,
            position: geometry.position,
            rotation: geometry.rotation,
            kind: Some(context.kind),
            level: context.level,
            permanently_invulnerable,
            invulnerable_time,
            damage_cooldown_time: mob
                .map(|m| m.invulnerable)
                .or_else(|| player.map(|p| p.health.hurt.cooldown)),
            invulnerable_to_piercing_weapon: match (permanently_invulnerable, invulnerable_time) {
                (Some(f), Some(t)) => Some(f || t > 0),
                _ => None,
            },
            velocity: geometry.velocity,
            on_ground: geometry.on_ground,
            bounding_box: geometry.bounding_box,
            eye_height: geometry.eye_height,
            dimensions: geometry.dimensions,
            is_multipart: Some(matches!(source, Dragon(_))),
            parts: if matches!(source, Dragon(_)) {
                context.parts
            } else {
                BaseReadParts::new(&[])
            },
            head_yaw: Some(match source {
                Mob(m) => m.head_yaw,
                Guest(g) => g.head_yaw,
                Player(_) => context.head_yaw,
                _ => 0.0,
            }),
            direction: Some(direction),
            no_physics,
            leash_holder: match source {
                Mob(_) | Boat(_) => context.leash_holder,
                _ => None,
            },
            sound_source,
            spectator: Some(match source {
                Guest(g) => g.game_mode == 3,
                Player(_) => context.spectator,
                _ => false,
            }),
            is_shift_key_down: context
                .client_shift
                .or_else(|| Some(player.is_some_and(|p| p.shift_key_down))),
            is_crouching: pose.map(|p| p == "crouching"),
            is_sprinting: Some(mob.map_or_else(
                || player.is_some_and(|p| p.sprinting),
                |m| m.extra.sprinting,
            )),
            is_in_water: mob
                .map(|m| m.fluids.in_water)
                .or_else(|| player.map(|p| p.fluids.was_touching_water)),
            is_in_lava: mob
                .map(|m| m.fluids.in_lava)
                .or_else(|| player.map(|p| p.is_in_lava())),
            fire_immune: mob
                .map(|m| horizon_client::mobs::spawn_rules::fire_immune(m.species()))
                .or_else(|| player.map(|_| false)),
            fire_ticks: mob
                .map(|m| m.fire_ticks)
                .or_else(|| player.map(|p| p.fire_ticks))
                .or(match source {
                    Stand(s) => Some(s.fire_ticks),
                    Item(i) => Some(i.fire_ticks),
                    _ => None,
                }),
            portal_cooldown: match source {
                Falling(f) => Some(f.portal.cooldown),
                _ => context.portal_cooldown,
            },
            air_supply: mob.map(|m| m.air).or_else(|| player.map(|p| p.air)),
            max_air_supply: Some(super::native_entity_state::max_air(
                mob.map(|m| m.species()),
            )),
            is_silent: mob.map(|m| m.silent).or_else(|| player.map(|_| false)),
            glowing: mob.map(|m| m.glowing),
            no_gravity: mob.map(|m| m.no_gravity),
            ticks_frozen: mob
                .map(|m| m.ticks_frozen)
                .or_else(|| player.map(|p| p.ticks_frozen)),
            tick_count: match source {
                Mob(m) => Some(m.age as i32),
                Item(i) => Some(i.ticks as i32),
                Orb(o) => Some(o.ticks as i32),
                Cart(c) => Some(c.tick_count),
                _ => None,
            },
            is_invisible: match source {
                Mob(m) => Some(m.is_invisible()),
                Stand(s) => Some(s.invisible),
                Player(_) => context.invisible,
                Guest(g) => Some(g.effects.get("invisibility").is_some()),
                _ => None,
            },
            pose,
        };
        if !context.logical_server {
            // These scalar reads resolve only server-resident targets. A client
            // copy must not turn unknown server data into apparently known flags.
            row.custom_name_visible = None;
            row.classification = None;
            row.added_to_level = None;
            row.permanently_invulnerable = None;
            row.invulnerable_time = None;
            row.damage_cooldown_time = None;
            row.invulnerable_to_piercing_weapon = None;
            row.head_yaw = None;
            row.direction = None;
            row.no_physics = None;
            row.leash_holder = None;
            row.sound_source = None;
            row.spectator = None;
            row.is_crouching = None;
            row.is_sprinting = None;
            row.is_in_water = None;
            row.is_in_lava = None;
            row.fire_immune = None;
            row.fire_ticks = None;
            row.portal_cooldown = None;
            row.air_supply = None;
            row.max_air_supply = None;
            row.is_silent = None;
            row.glowing = None;
            row.no_gravity = None;
            row.ticks_frozen = None;
            row.tick_count = None;
            row.is_invisible = None;
            row.pose = None;
            row.is_shift_key_down = context.client_shift;
        }
        row
    }
}
