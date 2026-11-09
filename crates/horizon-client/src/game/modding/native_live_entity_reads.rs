//! Validated subjects for source-live entity reads. Handle-family and player
//! UUID checks belong at this boundary, before physical or pending refusal.
use super::{Game, commands::Target, native_read_dependencies::EntityReadDependency};
use horizon_client::join_seq::residency::{
    EntitySubtype, IdentityDetails, Source,
    table::{EntityLookup, Location},
};
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
    ActingPlayer(LivingRef),
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
            Self::Living(living) | Self::MobOrPlayer(living) | Self::ActingPlayer(living) => {
                living.entity()
            }
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

/// Identity and physical address admitted by the sole read boundary. The
/// address travels with the target so callers never infer a world from Game's
/// currently borrowed level. An OWN caller must still supply its actual loan.
#[derive(Clone, Copy)]
pub(super) struct EntityReadTarget {
    pub(super) target: Target,
    pub(super) location: Location,
}

/// Metadata admission separated from refusal selection so a multi-input read
/// validates every input and relational domain before selecting a physical error.
#[derive(Clone, Copy)]
pub(super) struct EntityReadAdmission {
    read: EntityReadTarget,
    lifted: bool,
    pending: Option<CommandId>,
}
impl EntityReadAdmission {
    pub(super) fn location(self) -> Location {
        self.read.location
    }
    pub(super) fn finish<E>(self) -> Result<EntityReadTarget, EntityReadError<E>> {
        if self.lifted {
            return Err(EntityReadError::Lifted);
        }
        if let Some(id) = self.pending {
            return Err(EntityReadError::Pending(id));
        }
        Ok(self.read)
    }
}

impl Game<'_> {
    pub(super) fn entity_read_dependency<E>(
        &self,
        subject: EntityReadSubject,
        dependencies: &[EntityReadDependency],
    ) -> Option<Result<EntityReadTarget, EntityReadError<E>>> {
        self.entity_read_admission(subject, dependencies)
            .map(EntityReadAdmission::finish)
    }

    /// Validate one subject once. No UUID creation, serialization, world RNG,
    /// heavy payload construction or speculative light projection runs here.
    pub(super) fn entity_read_admission(
        &self,
        subject: EntityReadSubject,
        dependencies: &[EntityReadDependency],
    ) -> Option<EntityReadAdmission> {
        let entity = subject.entity();
        let server = self.is_logical_server();
        if !server {
            let remote = self.remote.as_ref().filter(|remote| remote.logged_in)?;
            let id = i32::try_from(entity.id()).ok()?;
            if id != remote.entity_id {
                remote.tracked(id)?;
            }
            // Only these client families have an actual backend. Family and
            // UUID proof below still uses the same canonical admitted row.
            if !matches!(
                subject,
                EntityReadSubject::Entity(_)
                    | EntityReadSubject::Living(_)
                    | EntityReadSubject::MobOrPlayer(_)
                    | EntityReadSubject::ActingPlayer(_)
                    | EntityReadSubject::Player(_)
            ) {
                return None;
            }
        }
        if self.native_lifecycle.epoch() != Some(entity.epoch()) {
            return None;
        }
        let mut table = self.entity_residency.borrow_mut();
        let (location, state, cached, details) = table
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
                super::entity_residency::residency_target_at(location, entity.id())
            }
        };
        if !server
            && !matches!(subject, EntityReadSubject::Entity(_))
            && !matches!(target, Target::Player)
        {
            return None;
        }
        if !self.server_read_target_visible(location.owner.dimension, target)
            || !self.entity_read_subject_matches(subject, target, location, details)
        {
            return None;
        }
        Some(EntityReadAdmission {
            read: EntityReadTarget { target, location },
            lifted: (matches!(state, EntityLookup::Taken) || self.held_out.contains(&entity.id()))
                && !self.entity_read_actor_owns(entity.id(), location),
            pending: cached.and_then(|row| row.pending_dependency(dependencies)),
        })
    }

    fn entity_read_subject_matches(
        &self,
        subject: EntityReadSubject,
        target: Target,
        location: Location,
        details: IdentityDetails,
    ) -> bool {
        use EntitySubtype::*;
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
            EntityReadSubject::ActingPlayer(_) => match location.source {
                Source::LocalPlayer => true,
                Source::Guest(conn) => self.lan_acting_conn == Some(conn),
                _ => false,
            },
            EntityReadSubject::Mob(_) => matches!(target, Target::Mob(_) | Target::Dragon(_)),
            EntityReadSubject::OrdinaryMob(_) => matches!(target, Target::Mob(_)),
            EntityReadSubject::MobOrPlayer(_) => {
                matches!(target, Target::Player | Target::Guest(_) | Target::Mob(_))
            }
            EntityReadSubject::Player(player) => {
                matches!(target, Target::Player | Target::Guest(_))
                    && details.known_uuid.is_some_and(|uuid| {
                        horizon_client::lan::playerdata::ints_uuid(uuid) == player.uuid()
                    })
            }
            EntityReadSubject::Merchant(_) => {
                matches!(target, Target::Mob(_)) && details.subtype == Merchant
            }
            EntityReadSubject::ItemEntity(_) => matches!(target, Target::Item(_)),
            EntityReadSubject::Display(_) => {
                matches!(target, Target::Other(_))
                    && matches!(details.subtype, BlockDisplay | ItemDisplay | TextDisplay)
            }
            EntityReadSubject::TextDisplay(_) => {
                matches!(target, Target::Other(_)) && details.subtype == TextDisplay
            }
            EntityReadSubject::ItemDisplay(_) => {
                matches!(target, Target::Other(_)) && details.subtype == ItemDisplay
            }
            EntityReadSubject::Minecart(_) => matches!(target, Target::Cart(_)),
            EntityReadSubject::FurnaceMinecart(_) => {
                matches!(target, Target::Cart(_)) && details.subtype == FurnaceCart
            }
            EntityReadSubject::TntMinecart(_) => {
                matches!(target, Target::Cart(_)) && details.subtype == TntCart
            }
            EntityReadSubject::HopperMinecart(_) => {
                matches!(target, Target::Cart(_)) && details.subtype == HopperCart
            }
            EntityReadSubject::CommandMinecart(_) => {
                matches!(target, Target::Cart(_)) && details.subtype == CommandCart
            }
            EntityReadSubject::Dragon(_) => matches!(target, Target::Dragon(_)),
            EntityReadSubject::Projectile(_) => {
                matches!(target, Target::Projectile(_) | Target::Trident(_))
            }
            EntityReadSubject::FishingHook(_) => matches!(target, Target::Hook(_)),
        }
    }
}
