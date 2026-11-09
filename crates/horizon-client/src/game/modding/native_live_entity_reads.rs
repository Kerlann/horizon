//! Validated subjects for source-live entity reads. Handle-family and player
//! UUID checks belong at this boundary, before physical or pending refusal.
use super::{
    Game,
    commands::Target,
    native_entity_read_cache::{NativeReadDetails, NativeReadPublication},
    native_read_dependencies::EntityReadDependency,
};
use horizon_client::join_seq::residency::table::EntityLookup;
use mod_api::*;

/// A typed expected family, including properties supported by only a subtype.
/// Keeping the handle intact prevents losing PlayerRef's UUID before validation.
#[derive(Clone, Copy)]
pub(super) enum EntityReadSubject {
    Entity(EntityRef),
    Living(LivingRef),
    Mob(MobRef),
    OrdinaryMob(MobRef),
    MobOrPlayer(LivingRef),
    Player(PlayerRef),
    Merchant(MerchantRef),
    ItemEntity(ItemEntityRef),
    Display(DisplayRef),
    TextDisplay(DisplayRef),
    ItemDisplay(DisplayRef),
    Minecart(MinecartRef),
    FurnaceMinecart(MinecartRef),
    TntMinecart(MinecartRef),
    HopperMinecart(MinecartRef),
    CommandMinecart(MinecartRef),
    Dragon(DragonRef),
    Projectile(ProjectileRef),
    FishingHook(FishingHookRef),
}

impl EntityReadSubject {
    pub(super) fn entity(self) -> EntityRef {
        match self {
            Self::Entity(entity) => entity,
            Self::Living(living) | Self::MobOrPlayer(living) => living.entity(),
            Self::Mob(mob) | Self::OrdinaryMob(mob) => mob.entity(),
            Self::Player(player) => player.entity(),
            Self::Merchant(merchant) => merchant.entity(),
            Self::ItemEntity(item) => item.entity(),
            Self::Display(display) | Self::TextDisplay(display) | Self::ItemDisplay(display) => {
                display.entity()
            }
            Self::Minecart(cart)
            | Self::FurnaceMinecart(cart)
            | Self::TntMinecart(cart)
            | Self::HopperMinecart(cart)
            | Self::CommandMinecart(cart) => cart.entity(),
            Self::Dragon(dragon) => dragon.entity(),
            Self::Projectile(projectile) => projectile.entity(),
            Self::FishingHook(hook) => hook.entity(),
        }
    }
}

impl Game<'_> {
    /// Validate one subject once. No UUID creation, serialization, world RNG,
    /// heavy payload construction or speculative light projection runs here.
    pub(super) fn entity_read_dependency<E>(
        &self,
        subject: EntityReadSubject,
        dependencies: &[EntityReadDependency],
    ) -> Option<Result<Target, EntityReadError<E>>> {
        let entity = subject.entity();
        if !self.is_logical_server() {
            // Client reads retain their existing tracked/local-player scope;
            // a server's physical owner state is not a client availability bit.
            let target = self.native_client_entity_target(entity)?;
            let supported = match subject {
                EntityReadSubject::Entity(_) => true,
                EntityReadSubject::Living(_) | EntityReadSubject::MobOrPlayer(_) => {
                    matches!(target, Target::Player)
                }
                EntityReadSubject::Player(player) => self.local_player() == Some(player),
                _ => false,
            };
            return supported.then_some(Ok(target));
        }
        if self.native_lifecycle.epoch() != Some(entity.epoch())
            || self.held_out.contains(&entity.id())
        {
            return None;
        }
        let mut table = self.entity_residency.borrow_mut();
        let (location, state, cached) = table
            .server_admitted_read_lookup(entity.id(), |at| self.locate_server_resident_owner(at))?;
        let target = match state {
            EntityLookup::Found {
                value: (target, true),
                ..
            } => target,
            EntityLookup::Found {
                value: (_, false), ..
            }
            | EntityLookup::Missing => return None,
            EntityLookup::Taken => {
                super::entity_residency::residency_target(location.owner.kind, entity.id())
            }
        };
        // These raw operations read the level in place. Guest session/body
        // records remain server-wide, as on their existing source paths.
        if location.owner.dimension != self.dim.key()
            && !matches!(target, Target::Player | Target::Guest(_))
        {
            return None;
        }
        if !self.server_read_target_visible(location.owner.dimension, target)
            || !self.entity_read_subject_matches(subject, target, cached)
        {
            return None;
        }
        if self.entity_read_actor_owns(entity.id(), location) {
            return Some(Ok(target));
        }
        if matches!(state, EntityLookup::Taken) {
            return Some(Err(EntityReadError::Lifted));
        }
        Some(
            match cached.and_then(|row| row.pending_dependency(dependencies)) {
                Some(id) => Err(EntityReadError::Pending(id)),
                None => Ok(target),
            },
        )
    }

    fn entity_read_subject_matches(
        &self,
        subject: EntityReadSubject,
        target: Target,
        cached: Option<&NativeReadPublication>,
    ) -> bool {
        // Type identity is immutable within an admitted generation. Its compact
        // kind string is identity metadata, not a retained heavy payload.
        let kind = || {
            cached
                .map(|row| row.base.kind.as_ref())
                .or_else(|| match target {
                    Target::Mob(id) => self
                        .mobs
                        .iter()
                        .find(|mob| mob.id == id)
                        .map(|mob| mob.kind.id.as_str()),
                    Target::Other(id) => self
                        .others
                        .iter()
                        .find(|other| other.id == id)
                        .map(|other| other.kind.as_str()),
                    Target::Cart(id) => {
                        self.carts
                            .iter()
                            .find(|cart| cart.id == id)
                            .map(|cart| match cart.kind {
                                super::minecarts::CartKind::Rideable => "minecraft:minecart",
                                super::minecarts::CartKind::Chest => "minecraft:chest_minecart",
                                super::minecarts::CartKind::Furnace => "minecraft:furnace_minecart",
                                super::minecarts::CartKind::Tnt => "minecraft:tnt_minecart",
                                super::minecarts::CartKind::Hopper => "minecraft:hopper_minecart",
                                super::minecarts::CartKind::CommandBlock => {
                                    "minecraft:command_block_minecart"
                                }
                                super::minecarts::CartKind::Spawner => "minecraft:spawner_minecart",
                            })
                    }
                    _ => None,
                })
        };
        match subject {
            EntityReadSubject::Entity(_) => true,
            EntityReadSubject::Living(_) => matches!(
                target,
                Target::Player
                    | Target::Guest(_)
                    | Target::Mob(_)
                    | Target::Stand(_)
                    | Target::Dragon(_)
            ),
            EntityReadSubject::Mob(_) => matches!(target, Target::Mob(_) | Target::Dragon(_)),
            EntityReadSubject::OrdinaryMob(_) => matches!(target, Target::Mob(_)),
            EntityReadSubject::MobOrPlayer(_) => {
                matches!(target, Target::Player | Target::Guest(_) | Target::Mob(_))
            }
            EntityReadSubject::Player(player) => {
                if !matches!(target, Target::Player | Target::Guest(_)) {
                    return false;
                }
                if let Some(row) = cached {
                    return matches!(&row.details, NativeReadDetails::Player { player: stored, .. } if stored.1 == player);
                }
                match target {
                    Target::Player => {
                        self.lan_acting_uuid == Some(player.uuid())
                            && self.native_acting_entity() == player.entity().id()
                    }
                    Target::Guest(conn) => self
                        .lan
                        .as_ref()
                        .and_then(|host| host.hub.guest(conn))
                        .is_some_and(|guest| guest.uuid == player.uuid()),
                    _ => false,
                }
            }
            EntityReadSubject::Merchant(_) => {
                matches!(target, Target::Mob(_))
                    && matches!(
                        kind(),
                        Some("minecraft:villager" | "minecraft:wandering_trader")
                    )
            }
            EntityReadSubject::ItemEntity(_) => matches!(target, Target::Item(_)),
            EntityReadSubject::Display(_) => {
                matches!(target, Target::Other(_))
                    && matches!(
                        kind(),
                        Some(
                            "minecraft:block_display"
                                | "minecraft:item_display"
                                | "minecraft:text_display"
                        )
                    )
            }
            EntityReadSubject::TextDisplay(_) => {
                matches!(target, Target::Other(_)) && kind() == Some("minecraft:text_display")
            }
            EntityReadSubject::ItemDisplay(_) => {
                matches!(target, Target::Other(_)) && kind() == Some("minecraft:item_display")
            }
            EntityReadSubject::Minecart(_) => matches!(target, Target::Cart(_)),
            EntityReadSubject::FurnaceMinecart(_) => {
                matches!(target, Target::Cart(_)) && kind() == Some("minecraft:furnace_minecart")
            }
            EntityReadSubject::TntMinecart(_) => {
                matches!(target, Target::Cart(_)) && kind() == Some("minecraft:tnt_minecart")
            }
            EntityReadSubject::HopperMinecart(_) => {
                matches!(target, Target::Cart(_)) && kind() == Some("minecraft:hopper_minecart")
            }
            EntityReadSubject::CommandMinecart(_) => {
                matches!(target, Target::Cart(_))
                    && kind() == Some("minecraft:command_block_minecart")
            }
            EntityReadSubject::Dragon(_) => matches!(target, Target::Dragon(_)),
            EntityReadSubject::Projectile(_) => {
                matches!(target, Target::Projectile(_) | Target::Trident(_))
            }
            EntityReadSubject::FishingHook(_) => matches!(target, Target::Hook(_)),
        }
    }
}
