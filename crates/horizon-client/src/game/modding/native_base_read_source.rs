//! Direct borrowed-source geometry and type identity for retained base rows.
//! No target resolution, source-list scans or publication state belongs here.
use super::native_base_read_rows::{BaseReadGeometry, BaseReadSource};
use horizon_core::geometry::{Aabb, Vec3};
use std::{collections::HashMap, sync::Arc};

use horizon_client::mobs::EntityTypeGeometry;

/// Type identity is interned once, never formatted or cloned as an owned String
/// during a capture pass. This pool does not load a second resource table.
#[derive(Default)]
pub(super) struct BaseReadKindPool {
    names: HashMap<String, Arc<str>>,
    boats: [HashMap<String, Arc<str>>; 2],
}
impl BaseReadKindPool {
    pub(super) fn intern(&mut self, name: &str) -> Arc<str> {
        if let Some(key) = self.names.get(name) {
            return key.clone();
        }
        let key: Arc<str> = if name.contains(':') {
            name.into()
        } else {
            format!("minecraft:{name}").into()
        };
        self.names.insert(name.to_owned(), key.clone());
        key
    }
}

fn array(p: Vec3) -> [f64; 3] {
    [p.x, p.y, p.z]
}
fn corners(b: Aabb) -> [[f64; 3]; 2] {
    [array(b.min_position()), array(b.max_position())]
}
/// Entity.makeBoundingBox: feet-centred, with the current (possibly dynamic) dimensions.
fn feet_box(p: [f64; 3], width: f64, height: f64) -> [[f64; 3]; 2] {
    // EntityDimensions.makeBoundingBox divides its float width before widening.
    let half = f64::from((width / 2.0) as f32);
    corners(Aabb::new(
        Vec3::new(p[0] - half, p[1], p[2] - half),
        Vec3::new(p[0] + half, p[1] + height, p[2] + half),
    ))
}

/// Source kind without `target_type` or Entity::native_type (Mob's implementation
/// formats a new String). Vanilla default-namespace IDs are normalized by the pool.
pub(super) fn base_read_source_kind(
    source: BaseReadSource<'_>,
    pool: &mut BaseReadKindPool,
) -> Arc<str> {
    use BaseReadSource::*;
    if let Boat(b) = source {
        let map = &mut pool.boats[usize::from(b.chest)];
        if let Some(kind) = map.get(b.wood.as_ref()) {
            return kind.clone();
        }
        let kind: Arc<str> = b.entity_type().into();
        map.insert(b.wood.to_string(), kind.clone());
        return kind;
    }
    let name = match source {
        Mob(m) => &m.kind.id,
        Item(_) => "minecraft:item",
        Orb(_) => "minecraft:experience_orb",
        Stand(_) => "minecraft:armor_stand",
        Boat(_) => unreachable!("boat handled by material identity cache"),
        Cart(c) => c.kind.id(),
        Projectile(p) => p.entity_type(),
        Tnt(_) => "minecraft:tnt",
        Falling(_) => "minecraft:falling_block",
        Rocket(_) => "minecraft:firework_rocket",
        Trident(_) => "minecraft:trident",
        Frame(f) => f.kind(),
        Painting(_) => "minecraft:painting",
        Cushion(_) => "minecraft:cushion",
        Dragon(_) | DragonPart(_, _) => "minecraft:ender_dragon",
        Crystal(_) => "minecraft:end_crystal",
        Fangs(_) => "minecraft:evoker_fangs",
        Spawner(_) => "minecraft:ominous_item_spawner",
        Cloud(_) => "minecraft:area_effect_cloud",
        Other(o) => &o.kind,
        Hook(_) => "minecraft:fishing_bobber",
        Knot(_) => "minecraft:leash_knot",
        Player(_) | Guest(_) => "minecraft:player",
    };
    pool.intern(name)
}

/// Geometry from one physically borrowed owner. Registry facts supply only the
/// fixed-size fallback and type eye height; dynamic source bounds always win.
pub(super) fn base_read_source_geometry(
    source: BaseReadSource<'_>,
    facts: EntityTypeGeometry,
) -> Option<BaseReadGeometry> {
    use BaseReadSource::*;
    macro_rules! walked {
        ($e:expr) => {
            BaseReadGeometry::from_entity($e, facts.eye_height, [facts.width, facts.height])
        };
    }
    let mut geometry = match source {
        Mob(m) => {
            let mut g = walked!(m);
            g.eye_height = m.eye_y() - m.position.y;
            g
        }
        Item(e) => walked!(e),
        Orb(e) => walked!(e),
        Stand(e) => {
            let mut g = walked!(e);
            g.eye_height = if e.marker {
                g.bounding_box = feet_box(array(e.position), 0.0, 0.0);
                0.0
            } else if e.small {
                f64::from(0.9875f32)
            } else {
                facts.eye_height
            };
            g
        }
        Boat(e) => walked!(e),
        Cart(e) => walked!(e),
        Projectile(e) => walked!(e),
        Tnt(e) => walked!(e),
        Falling(e) => walked!(e),
        Rocket(e) => walked!(e),
        Trident(e) => walked!(e),
        other => {
            let (position, bounds) = match other {
                Frame(f) => {
                    let (lo, hi) = f.bounds();
                    ([0, 1, 2].map(|i| (lo[i] + hi[i]) / 2.0), [lo, hi])
                }
                Painting(p) => {
                    let (lo, hi) = p.bounds();
                    ([0, 1, 2].map(|i| (lo[i] + hi[i]) / 2.0), [lo, hi])
                }
                Cushion(c) => {
                    let (lo, hi) = c.bounds();
                    (array(c.position), [lo, hi])
                }
                Crystal(c) => (array(c.position), corners(c.bounds())),
                Dragon(d) => {
                    let p = array(d.position);
                    (p, feet_box(p, facts.width, facts.height))
                }
                DragonPart(d, index) => {
                    d.parts.get(index)?;
                    let (p, b) = d.part_geometry(index);
                    (array(p), corners(b))
                }
                Fangs(f) => (f.pos, feet_box(f.pos, facts.width, facts.height)),
                Spawner(s) => (s.position, feet_box(s.position, facts.width, facts.height)),
                Cloud(c) => (
                    c.at,
                    feet_box(c.at, f64::from(c.radius * 2.0f32), facts.height),
                ),
                Other(o) => {
                    let p = array(o.position);
                    let bounds = if let Some(i) = &o.interaction {
                        corners(
                            super::interactions::RemoteInteraction {
                                position: o.position,
                                width: i.width,
                                height: i.height,
                                response: i.response,
                                vehicle: None,
                            }
                            .bounds(),
                        )
                    } else {
                        feet_box(p, facts.width, facts.height)
                    };
                    (p, bounds)
                }
                Hook(h) => (array(h.position), corners(h.bounds())),
                Knot(k) => {
                    // LeashFenceKnotEntity.recalculateBoundingBox computes its
                    // own fence-relative position and divides type width as a
                    // double, rather than EntityDimensions' float division.
                    let p = horizon_client::mobs::leash::knot_state(k.fence).position;
                    let half = facts.width / 2.0;
                    let bounds = Aabb::new(
                        Vec3::new(p.x - half, p.y, p.z - half),
                        Vec3::new(p.x + half, p.y + facts.height, p.z + half),
                    );
                    (array(p), corners(bounds))
                }
                Player(p) => (array(p.position), corners(p.bounds())),
                Guest(g) => (g.position, corners(g.bounds())),
                _ => unreachable!("walked sources handled above"),
            };
            BaseReadGeometry {
                position,
                rotation: [0.0; 2],
                velocity: [0.0; 3],
                on_ground: false,
                bounding_box: bounds,
                eye_height: facts.eye_height,
                dimensions: [facts.width, facts.height],
            }
        }
    };
    // Preserve the current scalar target_rotation contract, including its zero
    // rotation for items/projectiles and other kinds not explicitly listed.
    geometry.rotation = match source {
        Mob(m) => [m.yaw, m.head_pitch],
        Stand(s) => [s.yaw, 0.0],
        Boat(b) => [b.yaw, b.pitch],
        Cart(c) => [c.yaw, c.pitch],
        Fangs(f) => [f.yaw, 0.0],
        Other(o) => [o.yaw, o.pitch],
        Guest(g) => [g.yaw, g.pitch],
        _ => [0.0; 2],
    };
    geometry.dimensions = match source {
        Mob(m) => {
            let (w, h, _) = m.dims();
            [w, h]
        }
        Stand(s) => {
            if s.marker {
                [0.0; 2]
            } else if s.small {
                [0.25, f64::from(1.975f32) / 2.0]
            } else {
                [0.5, f64::from(1.975f32)]
            }
        }
        Player(p) => [
            f64::from(p.dimensions.width),
            f64::from(p.dimensions.height),
        ],
        Guest(g) => {
            let d = g.dimensions();
            [f64::from(d.width), f64::from(d.height)]
        }
        Cloud(c) => [f64::from(c.radius * 2.0f32), facts.height],
        Other(o) => o
            .interaction
            .as_ref()
            .map_or([facts.width, facts.height], |i| {
                [f64::from(i.width), f64::from(i.height)]
            }),
        DragonPart(_, index) => {
            let (w, h) = *super::dragon::PART_SIZES.get(index)?;
            [w, h]
        }
        _ => [facts.width, facts.height],
    };
    match source {
        Player(p) => {
            geometry.velocity = array(p.velocity);
            geometry.on_ground = p.ground.on_ground;
            geometry.eye_height = f64::from(p.dimensions.eye_height);
        }
        Guest(g) => {
            geometry.velocity = [0, 1, 2].map(|i| g.position[i] - g.previous_position[i]);
            geometry.on_ground = g.on_ground;
            geometry.eye_height = g.eye_height();
        }
        _ => {}
    }
    Some(geometry)
}

use super::native_base_read_rows::{BaseReadContext, BaseReadParts, CompactEntityReadRow};
use horizon_client::join_seq::residency::Kind;
use horizon_client::mobs::leash::Holder;
use mod_api::{EntityRef, LeashHolder, LevelRef, WorldEpoch};

/// Pass-borrowed engine context. Resolvers use the existing validated tables;
/// they must not scan source families or allocate runtime identities.
pub(super) struct BaseReadOwner<'a> {
    pub(super) level: LevelRef,
    pub(super) epoch: WorldEpoch,
    pub(super) logical_server: bool,
    pub(super) resolve: &'a dyn Fn(u32) -> Option<(EntityRef, Kind)>,
    pub(super) player_leash: &'a dyn Fn([i32; 4]) -> Option<EntityRef>,
    pub(super) pearl_cooldown: &'a dyn Fn(u32) -> i32,
    pub(super) type_facts: &'a dyn Fn(&str) -> Option<EntityTypeGeometry>,
    pub(super) default_player_uuid: Option<[i32; 4]>,
    /// Local/acting player state, unused for resident nonplayer families.
    pub(super) local_rotation: [f32; 2],
    pub(super) local_head_yaw: f32,
    pub(super) local_spectator: bool,
    pub(super) local_invisible: Option<bool>,
    pub(super) client_shift: Option<bool>,
    pub(super) player_portal_cooldown: Option<i32>,
}

fn leash_holder(source: BaseReadSource<'_>, owner: &BaseReadOwner<'_>) -> Option<LeashHolder> {
    let (holder, player) = match source {
        BaseReadSource::Mob(m) => {
            if m.extra.leash_pending.is_some() {
                return None;
            }
            (m.extra.leash, m.extra.leash_player)
        }
        BaseReadSource::Boat(b) => {
            if b.leash_pending.is_some() {
                return None;
            }
            (b.leash, b.leash_player.or(owner.default_player_uuid))
        }
        _ => return None,
    };
    Some(match holder {
        None => LeashHolder::Unleashed,
        Some(Holder::Knot(p)) => LeashHolder::Knot(p),
        Some(Holder::Mob(id)) => {
            let (entity, kind) = (owner.resolve)(id)?;
            if kind != Kind::Mob {
                return None;
            }
            LeashHolder::Entity(entity)
        }
        Some(Holder::Boat(id)) => {
            let (entity, kind) = (owner.resolve)(id)?;
            if kind != Kind::Boat {
                return None;
            }
            LeashHolder::Entity(entity)
        }
        Some(Holder::Player) => LeashHolder::Entity((owner.player_leash)(player?)?),
    })
}

/// Raw-owner entry for before-callback/after-tick captures while a list is
/// lifted. `id` and its join have already been validated by the owning pass;
/// this function performs no attempt to find that entity in any list.
pub(super) fn snapshot_base_read_source(
    source: BaseReadSource<'_>,
    id: u32,
    persist: &super::persist::EntityMemory,
    portals: &HashMap<u32, horizon_client::nether_portal::PortalState>,
    pool: &mut BaseReadKindPool,
    owner: &BaseReadOwner<'_>,
) -> Option<CompactEntityReadRow> {
    use BaseReadSource::*;
    let kind = base_read_source_kind(source, pool);
    // Facts belong to the current resources, not the identity pool. In
    // particular a retained kind pool must not pin the first world's sizes.
    let facts = (owner.type_facts)(&kind)?;
    let mut geometry = base_read_source_geometry(source, facts)?;
    if matches!(source, Player(_)) {
        geometry.rotation = owner.local_rotation;
    }
    let added = matches!(
        source,
        Player(_) | Guest(_) | Other(_) | Fangs(_) | Cloud(_) | Spawner(_) | Knot(_)
    ) || persist
        .live
        .binary_search_by_key(&id, |entry| entry.0)
        .is_ok();
    let portal_cooldown = match source {
        Player(_) | Guest(_) => owner.player_portal_cooldown,
        Falling(f) => Some(f.portal.cooldown),
        Projectile(p) => {
            if matches!(
                p.kind,
                horizon_client::projectiles::ProjectileKind::EnderPearl
            ) {
                Some((owner.pearl_cooldown)(id))
            } else if super::nether_portal::is_arrow(p)
                || super::nether_portal::is_portal_projectile(p)
            {
                Some(portals.get(&id).map_or(0, |p| p.cooldown))
            } else {
                None
            }
        }
        Mob(_) | Item(_) | Orb(_) | Boat(_) | Cart(_) | Tnt(_) | Rocket(_) | Trident(_) => {
            Some(portals.get(&id).map_or(0, |p| p.cooldown))
        }
        _ => None,
    };
    let parts = if let Dragon(d) = source {
        let mut parts = [EntityRef::new(owner.epoch, 0); 8];
        let mut complete = true;
        for (offset, part) in parts.iter_mut().enumerate() {
            let part_id = d.id.checked_add(offset as u32 + 1)?;
            match (owner.resolve)(part_id) {
                Some((entity, _)) => *part = entity,
                None => {
                    complete = false;
                    break;
                }
            }
        }
        if complete {
            BaseReadParts::new(&parts)
        } else {
            None
        }
    } else {
        BaseReadParts::new(&[])
    };
    Some(CompactEntityReadRow::capture(
        source,
        geometry,
        BaseReadContext {
            level: owner.level,
            logical_server: owner.logical_server,
            kind,
            saved: persist.raw.get(&id),
            added_to_level: Some(added),
            portal_cooldown,
            leash_holder: leash_holder(source, owner),
            parts,
            head_yaw: owner.local_head_yaw,
            spectator: owner.local_spectator,
            invisible: owner.local_invisible,
            client_shift: owner.client_shift,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_core::nbt::Tag;

    #[test]
    fn fixed_type_box_preserves_float_half_width_and_normalizes_negative_dimensions() {
        assert_eq!(
            feet_box([0.0; 3], f64::from(f32::from_bits(1)), 1.0),
            [[0.0, 0.0, 0.0], [0.0, 1.0, 0.0]]
        );
        assert_eq!(
            feet_box([0.0; 3], -2.0, -3.0),
            [[-1.0, -3.0, -1.0], [1.0, 0.0, 1.0]]
        );
    }

    #[test]
    fn stand_dimensions_preserve_small_and_marker_state_at_large_coordinates() {
        let mut stand =
            horizon_client::entities::ArmorStand::new(11, Vec3::new(1e16, 8.0, 1e16), 0.0);
        let facts = EntityTypeGeometry {
            width: 0.5,
            height: f64::from(1.975f32),
            eye_height: f64::from(1.7775f32),
        };
        stand.small = true;
        let small = base_read_source_geometry(BaseReadSource::Stand(&stand), facts).unwrap();
        assert_eq!(small.dimensions, [0.25, f64::from(0.9875f32)]);
        assert_eq!(small.bounding_box[0][0], small.bounding_box[1][0]);
        stand.marker = true;
        let marker = base_read_source_geometry(BaseReadSource::Stand(&stand), facts).unwrap();
        assert_eq!(marker.dimensions, [0.0; 2]);
        assert_eq!(marker.bounding_box, [[1e16, 8.0, 1e16]; 2]);
        assert_eq!(marker.eye_height, 0.0);
    }

    #[test]
    fn interaction_uses_current_source_dimensions_not_registry_defaults() {
        let mut source = super::super::commands::OtherEntity {
            attachment_lifetime: Default::default(),
            join: horizon_client::join_seq::JoinSeq::next(),
            id: 31,
            kind: "minecraft:interaction".into(),
            position: Vec3::new(-2.0, 5.0, 8.0),
            yaw: 90.0,
            pitch: -10.0,
            item_transformation: None,
            interaction: Some(super::super::interactions::InteractionState {
                width: 3.0,
                height: 4.0,
                response: false,
                attack: None,
                interaction: None,
            }),
            knot_check: None,
            eye: None,
        };
        let facts = EntityTypeGeometry {
            width: 1.0,
            height: 1.0,
            eye_height: 0.85,
        };
        let first = base_read_source_geometry(BaseReadSource::Other(&source), facts).unwrap();
        assert_eq!(first.bounding_box, [[-3.5, 5.0, 6.5], [-0.5, 9.0, 9.5]]);
        assert_eq!(first.rotation, [90.0, -10.0]);
        source.interaction.as_mut().unwrap().width = 0.5;
        source.interaction.as_mut().unwrap().height = -4.0;
        let second = base_read_source_geometry(BaseReadSource::Other(&source), facts).unwrap();
        assert_eq!(
            second.bounding_box,
            [[-2.25, 5.0, 7.75], [-1.75, 5.0, 8.25]]
        );
        assert_eq!(first.bounding_box, [[-3.5, 5.0, 6.5], [-0.5, 9.0, 9.5]]);
    }

    #[test]
    fn bound_knot_capture_uses_fence_geometry_without_persist_membership() {
        let knot = super::super::leads::Knot {
            attachment_lifetime: Default::default(),
            id: 43,
            join: horizon_client::join_seq::JoinSeq::next(),
            fence: [-2, 64, 4],
        };
        let epoch = WorldEpoch::new(9);
        let owner = BaseReadOwner {
            level: LevelRef::new(epoch, mod_api::DimensionKey::OVERWORLD),
            epoch,
            logical_server: true,
            resolve: &|_| None,
            player_leash: &|_| None,
            pearl_cooldown: &|_| 0,
            type_facts: &|_| {
                Some(EntityTypeGeometry {
                    width: 0.375,
                    height: 0.5,
                    eye_height: f64::from(0.425f32),
                })
            },
            default_player_uuid: None,
            local_rotation: [0.0; 2],
            local_head_yaw: 0.0,
            local_spectator: false,
            local_invisible: None,
            client_shift: None,
            player_portal_cooldown: None,
        };
        let mut pool = BaseReadKindPool::default();
        let row = snapshot_base_read_source(
            BaseReadSource::Knot(&knot),
            43,
            &super::super::persist::EntityMemory::default(),
            &HashMap::new(),
            &mut pool,
            &owner,
        )
        .unwrap();
        assert_eq!(row.position, [-1.5, 64.375, 4.5]);
        assert_eq!(
            row.bounding_box,
            [[-1.6875, 64.375, 4.3125], [-1.3125, 64.875, 4.6875]]
        );
        assert_eq!(row.dimensions, [0.375, 0.5]);
        assert_eq!(row.kind.as_deref(), Some("minecraft:leash_knot"));
        assert_eq!(row.added_to_level, Some(true));
        assert_eq!(row.classification, Some(mod_api::MobCategory::Misc));
        assert_eq!(row.sound_source, Some(mod_api::SoundSource::Neutral));
        assert_eq!(row.pose, Some("standing"));
        assert_eq!(row.permanently_invulnerable, None);
        assert_eq!(row.portal_cooldown, None);
        assert_eq!(row.leash_holder, None);
        assert_eq!(row.is_shift_key_down, Some(false));
    }

    #[test]
    fn lifted_source_capture_does_not_require_list_residency() {
        let epoch = WorldEpoch::new(3);
        let source = super::super::commands::OtherEntity {
            attachment_lifetime: Default::default(),
            join: horizon_client::join_seq::JoinSeq::next(),
            id: 31,
            kind: "minecraft:marker".into(),
            position: Vec3::new(2.0, 5.0, 8.0),
            yaw: 0.0,
            pitch: 0.0,
            item_transformation: None,
            interaction: None,
            knot_check: None,
            eye: None,
        };
        let mut persist = super::super::persist::EntityMemory::default();
        persist
            .raw
            .insert(31, Tag::compound([("CustomNameVisible", Tag::Int(258))]));
        let portals = HashMap::new();
        let mut pool = BaseReadKindPool::default();
        let owner = BaseReadOwner {
            level: LevelRef::new(epoch, mod_api::DimensionKey::NETHER),
            epoch,
            logical_server: true,
            resolve: &|_| None,
            player_leash: &|_| None,
            pearl_cooldown: &|_| 0,
            type_facts: &|_| {
                Some(EntityTypeGeometry {
                    width: f64::from(f32::from_bits(1)),
                    height: -2.0,
                    eye_height: 0.0,
                })
            },
            default_player_uuid: None,
            local_rotation: [0.0; 2],
            local_head_yaw: 0.0,
            local_spectator: false,
            local_invisible: None,
            client_shift: None,
            player_portal_cooldown: None,
        };
        let compact = snapshot_base_read_source(
            BaseReadSource::Other(&source),
            31,
            &persist,
            &portals,
            &mut pool,
            &owner,
        )
        .unwrap();
        let capture = mod_api::EntityReadCapture {
            tick: mod_api::TickId::new(1),
            pass: 1,
            revision: 1,
            phase: mod_api::EntityReadCapturePhase::BeforeCallback,
            pending_overlay: None,
        };
        let row = compact.materialize(capture, EntityRef::new(epoch, 31), source.join.get());
        assert_eq!(row.position, [2.0, 5.0, 8.0]);
        assert_eq!(row.bounding_box, [[2.0, 3.0, 8.0], [2.0, 5.0, 8.0]]);
        assert_eq!(row.dimensions, [f64::from(f32::from_bits(1)), -2.0]);
        assert_eq!(row.custom_name_visible, Some(true));
        assert_eq!(row.level.dimension(), mod_api::DimensionKey::NETHER);
        assert_eq!(row.portal_cooldown, None);
        assert_eq!(row.kind.as_deref(), Some("minecraft:marker"));
        // Reusing interned identity must not reuse the first resource root's
        // geometry. A previously retained row stays unchanged, however.
        let second_owner = BaseReadOwner {
            type_facts: &|_| {
                Some(EntityTypeGeometry {
                    width: 2.0,
                    height: 3.0,
                    eye_height: 1.0,
                })
            },
            ..owner
        };
        let second = snapshot_base_read_source(
            BaseReadSource::Other(&source),
            31,
            &persist,
            &portals,
            &mut pool,
            &second_owner,
        )
        .unwrap();
        let second = second.materialize(capture, EntityRef::new(epoch, 31), source.join.get());
        assert_eq!(second.dimensions, [2.0, 3.0]);
        assert_eq!(row.dimensions, [f64::from(f32::from_bits(1)), -2.0]);
    }
}
