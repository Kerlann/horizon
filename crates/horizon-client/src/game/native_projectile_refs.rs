//! Projectile handles for native server mods: owner, deflection, and an
//! arrow's pickup, damage and critical flag. They cover the projectile list
//! and the player-thrown tridents and firework rockets, which Horizon keeps
//! in lists of their own.
use super::{Game, commands::Target};
use horizon_client::{
    arrows::Pickup,
    lan::playerdata::{ints_uuid, uuid_ints},
    mobs::Who,
    mod_api::{EntityRef, NativeArrowPickup, NativeDeflection, ProjectileRef},
    projectiles::{OwnerFields, Projectile, ProjectileKind, Turn},
};
use horizon_core::geometry::Vec3;

/// Where a projectile handle's entity lives.
#[derive(Clone, Copy)]
enum Backing {
    /// `creatures.projectiles`.
    List(usize),
    /// `tridents`: a trident a player threw.
    Trident(usize),
    /// `rockets`: a firework rocket.
    Rocket(usize),
}

impl Game<'_> {
    fn native_projectile_backing(&self, projectile: ProjectileRef) -> Option<Backing> {
        match self.native_entity_target(projectile.entity())? {
            Target::Projectile(id) => (self.creatures.projectiles.iter())
                .position(|p| p.id == id)
                .map(Backing::List),
            Target::Trident(id) => (self.tridents.iter())
                .position(|t| t.id == id)
                .map(Backing::Trident),
            Target::Rocket(id) => (self.rockets.iter())
                .position(|r| r.id == id)
                .map(Backing::Rocket),
            _ => None,
        }
    }

    /// The index in the projectile list: None for a trident a player threw,
    /// a rocket or a stale handle, which have no arrow state, kind or acceleration.
    fn native_projectile_index(&self, projectile: ProjectileRef) -> Option<usize> {
        match self.native_projectile_backing(projectile)? {
            Backing::List(i) => Some(i),
            _ => None,
        }
    }

    fn native_projectile(&self, projectile: ProjectileRef) -> Option<&Projectile> {
        self.creatures
            .projectiles
            .get(self.native_projectile_index(projectile)?)
    }

    /// Narrows an entity to a projectile: one in the projectile list, a trident
    /// a player threw or a firework rocket. None for another kind, a stale
    /// handle or a client.
    pub(super) fn native_projectile_ref(&self, entity: EntityRef) -> Option<ProjectileRef> {
        let projectile = ProjectileRef::new(entity);
        self.native_projectile_backing(projectile)
            .map(|_| projectile)
    }

    /// The owner a target names, as a resolved entity in the level in place.
    fn native_owner_entity(&self, target: Target) -> Option<EntityRef> {
        if self.target_level(target) != self.dim {
            return None;
        }
        let id = u32::try_from(self.network_entity_id(target)?).ok()?;
        self.native_entity(id)
    }

    /// `Projectile.getOwner`: the owner as a resolved entity. None for no
    /// owner, one that is gone or in another level, or a stale handle.
    pub(super) fn native_projectile_owner(&self, projectile: ProjectileRef) -> Option<EntityRef> {
        let target = match self.native_projectile_backing(projectile)? {
            Backing::List(i) => {
                let uuid = self.creatures.projectiles[i].owner_uuid?;
                self.target_by_uuid(ints_uuid(uuid))?
            }
            // A trident belongs to the guest who threw it, else the host.
            Backing::Trident(i) => self.tridents[i].guest.map_or(Target::Player, Target::Guest),
            Backing::Rocket(i) => {
                let r = &self.rockets[i];
                match r.owner? {
                    Who::Player => r.guest.map_or(Target::Player, Target::Guest),
                    Who::Mob(id) => Target::Mob(id),
                }
            }
        };
        self.native_owner_entity(target)
    }

    /// The owner fields for `Projectile.setOwner(owner)`. None for an owner
    /// that is stale or not a player or mob: Horizon keeps only those.
    fn native_projectile_owner_fields(&mut self, owner: Option<EntityRef>) -> Option<OwnerFields> {
        let Some(owner) = owner else {
            return Some((None, None, None));
        };
        let guest = match self.native_entity_target(owner)? {
            Target::Player => (Who::Player, self.lan_acting),
            Target::Guest(conn) => (Who::Player, Some(conn)),
            Target::Mob(id) => (Who::Mob(id), None),
            _ => return None,
        };
        let uuid = uuid_ints(self.native_entity_uuid(owner)?);
        Some((Some(guest.0), Some(uuid), guest.1))
    }

    /// `Projectile.setOwner` on the thing a handle names. False when the
    /// owner does not fit: a trident a player threw always has a player owner.
    fn native_projectile_apply_owner(&mut self, backing: Backing, fields: OwnerFields) -> bool {
        let (who, _, guest) = fields;
        match backing {
            Backing::List(i) => {
                let p = &mut self.creatures.projectiles[i];
                (p.owner, p.owner_uuid, p.guest) = fields;
                // `AbstractArrow.setOwner(Entity)`: a player's arrow may be picked up.
                if let Some(a) = &mut p.arrow {
                    a.set_owner(who);
                }
            }
            Backing::Trident(i) => {
                if who != Some(Who::Player) {
                    return false;
                }
                self.tridents[i].guest = guest;
            }
            Backing::Rocket(i) => {
                let r = &mut self.rockets[i];
                (r.owner, r.guest) = (who, guest);
            }
        }
        true
    }

    /// `Projectile.setOwner`. False for a stale projectile, or an owner that is
    /// stale or neither a player nor a mob (a trident a player threw takes a
    /// player only). Does not touch `leftOwner`.
    pub(super) fn native_projectile_set_owner(
        &mut self,
        projectile: ProjectileRef,
        owner: Option<EntityRef>,
    ) -> bool {
        let Some(backing) = self.native_projectile_backing(projectile) else {
            return false;
        };
        let Some(fields) = self.native_projectile_owner_fields(owner) else {
            return false;
        };
        self.native_projectile_apply_owner(backing, fields)
    }

    /// `Projectile.deflect(deflection, by, newOwner, byAttack, power)`, with
    /// `power` per axis, through `Projectile::deflect` for the projectile list
    /// (the engine path gameplay uses). `REVERSE` draws one float from the
    /// projectile's own random; `AIM_DEFLECT` uses `by`'s look and
    /// `MOMENTUM_DEFLECT` its normalized known speed, and both change nothing
    /// without `by`. The new owner is set, then `onDeflection`: a hurting
    /// projectile's acceleration power is 0.1 by an attack, else halved. A
    /// wind charge refuses for its first five ticks, with no change and no
    /// draw. A trident a player threw and a rocket turn the same way (no
    /// acceleration power). False, and nothing changed, for a stale projectile,
    /// `by` or owner, a non-finite power or a refusal.
    pub(super) fn native_projectile_deflect(
        &mut self,
        projectile: ProjectileRef,
        deflection: NativeDeflection,
        by: Option<EntityRef>,
        new_owner: Option<EntityRef>,
        by_attack: bool,
        power: [f64; 3],
    ) -> bool {
        let Some(backing) = self.native_projectile_backing(projectile) else {
            return false;
        };
        let Some(owner) = self.native_projectile_owner_fields(new_owner) else {
            return false;
        };
        if !power.iter().all(|v| v.is_finite()) {
            return false;
        }
        let turn = match (deflection, by) {
            (NativeDeflection::Reverse, _) => Turn::Reverse,
            (_, None) => Turn::Direction(None),
            (NativeDeflection::AimDeflect, Some(by)) => {
                let Some([x, y, z]) = self.native_entity_view_vector(by) else {
                    return false;
                };
                Turn::Direction(Some(Vec3::new(x, y, z)))
            }
            (NativeDeflection::MomentumDeflect, Some(by)) => {
                let Some(speed) = self.native_entity_known_speed(by) else {
                    return false;
                };
                Turn::Direction(Some(speed.normalize()))
            }
        };
        match backing {
            // `deflect` takes the `EntityReference` overload of `setOwner`: an
            // arrow's pickup does not change, unlike `set_owner`.
            Backing::List(i) => {
                self.creatures.projectiles[i].deflect(turn, power, owner, by_attack)
            }
            Backing::Trident(i) => {
                if owner.0 != Some(Who::Player) {
                    return false;
                }
                let t = &mut self.tridents[i];
                if let Some((velocity, rotation)) = turn.apply(t.velocity, power, &mut t.random) {
                    t.velocity = velocity;
                    t.yaw += rotation;
                }
                t.guest = owner.2;
                true
            }
            Backing::Rocket(i) => {
                let r = &mut self.rockets[i];
                if let Some((velocity, _)) = turn.apply(r.velocity, power, &mut r.random) {
                    r.velocity = velocity;
                }
                (r.owner, r.guest) = (owner.0, owner.2);
                true
            }
        }
    }

    /// `AbstractArrow.pickup`, for an arrow or a mob's trident.
    pub(super) fn native_projectile_pickup(
        &self,
        projectile: ProjectileRef,
    ) -> Option<NativeArrowPickup> {
        Some(
            match self.native_projectile(projectile)?.arrow.as_ref()?.pickup {
                Pickup::Disallowed => NativeArrowPickup::Disallowed,
                Pickup::Allowed => NativeArrowPickup::Allowed,
                Pickup::CreativeOnly => NativeArrowPickup::CreativeOnly,
            },
        )
    }

    /// Sets `AbstractArrow.pickup`. False for a stale handle or a projectile
    /// that is not an arrow or a mob's trident.
    pub(super) fn native_projectile_set_pickup(
        &mut self,
        projectile: ProjectileRef,
        pickup: NativeArrowPickup,
    ) -> bool {
        let Some(index) = self.native_projectile_index(projectile) else {
            return false;
        };
        let Some(arrow) = self.creatures.projectiles[index].arrow.as_mut() else {
            return false;
        };
        arrow.pickup = match pickup {
            NativeArrowPickup::Disallowed => Pickup::Disallowed,
            NativeArrowPickup::Allowed => Pickup::Allowed,
            NativeArrowPickup::CreativeOnly => Pickup::CreativeOnly,
        };
        true
    }

    /// `AbstractArrow.getBaseDamage`, for an arrow (a trident's is fixed).
    pub(super) fn native_projectile_base_damage(&self, projectile: ProjectileRef) -> Option<f64> {
        match self.native_projectile(projectile)?.kind {
            ProjectileKind::Arrow { base_damage, .. } => Some(base_damage),
            _ => None,
        }
    }

    /// `AbstractArrow.setBaseDamage`. False for a stale handle, a projectile
    /// that is not an arrow, or a value that is not finite.
    pub(super) fn native_projectile_set_base_damage(
        &mut self,
        projectile: ProjectileRef,
        damage: f64,
    ) -> bool {
        let Some(index) = self.native_projectile_index(projectile) else {
            return false;
        };
        let ProjectileKind::Arrow { base_damage, .. } = &mut self.creatures.projectiles[index].kind
        else {
            return false;
        };
        if !damage.is_finite() {
            return false;
        }
        *base_damage = damage;
        true
    }

    /// `AbstractArrow.isCritArrow`, for an arrow.
    pub(super) fn native_projectile_critical(&self, projectile: ProjectileRef) -> Option<bool> {
        let p = self.native_projectile(projectile)?;
        matches!(p.kind, ProjectileKind::Arrow { .. }).then(|| p.critical())
    }

    /// `AbstractArrow.setCritArrow`; clients see the flag at once through the
    /// entity data. False for a stale handle or a projectile that is not an arrow.
    pub(super) fn native_projectile_set_critical(
        &mut self,
        projectile: ProjectileRef,
        critical: bool,
    ) -> bool {
        let Some(index) = self.native_projectile_index(projectile) else {
            return false;
        };
        let p = &mut self.creatures.projectiles[index];
        if !matches!(p.kind, ProjectileKind::Arrow { .. }) {
            return false;
        }
        p.set_critical(critical);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        fireworks::Rocket, native_entity_refs::tests::with_game, trident::ThrownTrident,
    };
    use super::*;
    use horizon_client::{mobs::Mob, mod_api::NativeWorldEpoch, motion::PlayerMotion};
    use horizon_core::random::RandomSource;

    fn arrow(owner: Who) -> Projectile {
        let kind = ProjectileKind::Arrow {
            base_damage: 2.0,
            critical: false,
            effect: None,
        };
        let mut p = Projectile::new(kind, Vec3::new(0.0, 64.0, 0.0), Some(owner));
        p.velocity = Vec3::new(1.0, 2.0, 3.0);
        p
    }

    #[test]
    #[ignore = "needs prepared resources"]
    fn projectile_handles_read_and_write_owner_deflection_and_arrow_state() {
        with_game(|game| {
            game.options.dedicated = Some(Vec::new());
            let kind = game
                .res
                .mob_types()
                .get("minecraft:zombie")
                .unwrap()
                .clone();
            game.mobs.push(Mob::new(99, kind, Vec3::ZERO, 0.0));
            let mob = game.native_entity(99).unwrap();
            game.add_projectile(arrow(Who::Mob(99)));
            let id = game.creatures.projectiles[0].id;
            let entity = game.native_entity(id).unwrap();
            assert!(game.native_projectile_ref(mob).is_none());
            let handle = game.native_projectile_ref(entity).unwrap();
            assert_eq!(handle.entity(), entity);

            assert_eq!(handle.owner(game), Some(mob));
            assert!(handle.set_owner(game, None));
            assert_eq!(handle.owner(game), None);
            assert_eq!(game.creatures.projectiles[0].owner, None);
            assert!(handle.set_owner(game, Some(mob)));
            assert_eq!(handle.owner(game), Some(mob));
            assert_eq!(game.creatures.projectiles[0].owner, Some(Who::Mob(99)));
            assert!(!handle.set_owner(game, Some(entity)));

            assert_eq!(handle.pickup(game), Some(NativeArrowPickup::Disallowed));
            assert!(handle.set_pickup(game, NativeArrowPickup::CreativeOnly));
            assert_eq!(
                game.creatures.projectiles[0].arrow.as_ref().unwrap().pickup,
                Pickup::CreativeOnly
            );
            assert_eq!(handle.base_damage(game), Some(2.0));
            assert!(handle.set_base_damage(game, 7.5));
            assert!(!handle.set_base_damage(game, f64::NAN));
            assert_eq!(handle.base_damage(game), Some(7.5));
            assert_eq!(handle.critical(game), Some(false));
            assert!(handle.set_critical(game, true));
            assert!(game.creatures.projectiles[0].critical());

            // REVERSE: one float from the arrow's random, then the scaled velocity.
            let draw = game.creatures.projectiles[0]
                .arrow
                .as_ref()
                .unwrap()
                .random
                .clone()
                .next_float();
            let yaw = game.creatures.projectiles[0].yaw;
            assert!(handle.deflect(game, NativeDeflection::Reverse, None, None, false, [0.2; 3]));
            let p = &game.creatures.projectiles[0];
            assert_eq!(
                p.velocity,
                Vec3::new(1.0 * (-0.2 * 0.5), 2.0 * (-0.2 * 0.5), 3.0 * (-0.2 * 0.5))
            );
            assert_eq!(p.yaw, yaw + (170.0 + draw * 20.0));
            assert_eq!(p.owner, None);
            assert!(p.needs_sync);

            // AIM_DEFLECT along the deflector's look, scaled by the power.
            let look = game.native_entity_view_vector(mob).unwrap();
            assert!(handle.deflect(
                game,
                NativeDeflection::AimDeflect,
                Some(mob),
                Some(mob),
                true,
                [2.0; 3]
            ));
            let p = &game.creatures.projectiles[0];
            assert_eq!(
                p.velocity,
                Vec3::new(look[0] * 2.0, look[1] * 2.0, look[2] * 2.0)
            );
            assert_eq!(p.owner, Some(Who::Mob(99)));

            // MOMENTUM_DEFLECT of a mob that has not moved has no direction.
            assert!(handle.deflect(
                game,
                NativeDeflection::MomentumDeflect,
                Some(mob),
                None,
                false,
                [1.0; 3]
            ));
            assert_eq!(game.creatures.projectiles[0].velocity, Vec3::ZERO);

            // A hurting projectile's acceleration: 0.1 by an attack, else halved.
            let mut ball = Projectile::fireball(
                ProjectileKind::LargeFireball { power: 1 },
                Vec3::ZERO,
                None,
                [1.0, 0.0, 0.0],
            );
            ball.acceleration = 0.1;
            game.add_projectile(ball);
            let ball_id = game.creatures.projectiles[1].id;
            let ball = game
                .native_projectile_ref(game.native_entity(ball_id).unwrap())
                .unwrap();
            assert_eq!(ball.pickup(game), None);
            assert_eq!(ball.base_damage(game), None);
            assert!(!ball.set_critical(game, true));
            assert!(ball.deflect(game, NativeDeflection::Reverse, None, None, false, [1.0; 3]));
            assert_eq!(game.creatures.projectiles[1].acceleration, 0.05);
            assert!(ball.deflect(game, NativeDeflection::Reverse, None, None, true, [1.0; 3]));
            assert_eq!(game.creatures.projectiles[1].acceleration, 0.1);

            let stale = ProjectileRef::new(EntityRef::new(
                NativeWorldEpoch::new(entity.epoch().get() + 1),
                entity.id(),
            ));
            assert_eq!(stale.owner(game), None);
            assert!(!stale.set_pickup(game, NativeArrowPickup::Allowed));
            assert!(!stale.deflect(game, NativeDeflection::Reverse, None, None, false, [1.0; 3]));
            let taken = std::mem::take(&mut game.creatures.projectiles);
            assert!(handle.base_damage(game).is_none());
            game.creatures.projectiles = taken;
        });
    }

    /// A player entity for the tests: the host's player standing in for a guest.
    fn player(game: &mut Game<'_>) -> EntityRef {
        game.player = Some(PlayerMotion::new(game.motion, Vec3::ZERO).unwrap());
        game.lan_acting_uuid = Some(123);
        game.lan_acting_entity = Some(20);
        game.native_entity(20).unwrap()
    }

    #[test]
    #[ignore = "needs prepared resources"]
    fn trident_and_rocket_handles_narrow_and_turn_with_their_own_random() {
        with_game(|game| {
            game.options.dedicated = Some(Vec::new());
            let player = player(game);
            let id = game.take_entity_id();
            let mut trident = ThrownTrident::remote(id, Vec3::new(0.0, 64.0, 0.0), 10.0, 0.0);
            trident.velocity = Vec3::new(1.0, 2.0, 3.0);
            let mut expect = trident.random.clone();
            game.tridents.push(trident);
            let id = game.take_entity_id();
            let mut rocket =
                Rocket::remote(id, Vec3::new(0.0, 70.0, 0.0), Vec3::new(0.0, 1.0, 0.0));
            let mut rocket_random = rocket.random.clone();
            rocket.velocity = Vec3::new(2.0, 4.0, 6.0);
            game.rockets.push(rocket);

            let trident = game
                .native_projectile_ref(game.native_entity(game.tridents[0].id).unwrap())
                .unwrap();
            let rocket = game
                .native_projectile_ref(game.native_entity(game.rockets[0].id).unwrap())
                .unwrap();
            // A trident or rocket has no arrow state: those reads are None.
            assert_eq!(trident.pickup(game), None);
            assert_eq!(rocket.base_damage(game), None);
            assert!(!rocket.set_critical(game, true));

            // A trident a player threw keeps a player owner.
            assert!(!trident.set_owner(game, None));
            assert!(trident.set_owner(game, Some(player)));
            assert_eq!(game.tridents[0].guest, None);
            let zombie = game
                .res
                .mob_types()
                .get("minecraft:zombie")
                .unwrap()
                .clone();
            game.mobs.push(Mob::new(99, zombie, Vec3::ZERO, 0.0));
            let mob = game.native_entity(99).unwrap();
            assert!(!trident.set_owner(game, Some(mob)));

            // REVERSE: one float from the trident's own random, the level's untouched.
            let level = game.world.random.clone().next_float();
            let draw = expect.next_float();
            assert!(trident.deflect(
                game,
                NativeDeflection::Reverse,
                None,
                Some(player),
                false,
                [0.02, 0.2, 0.02]
            ));
            assert_eq!(
                game.tridents[0].velocity,
                Vec3::new(1.0 * (-0.02 * 0.5), 2.0 * (-0.2 * 0.5), 3.0 * (-0.02 * 0.5))
            );
            assert_eq!(game.tridents[0].yaw, 10.0 + (170.0 + draw * 20.0));
            assert_eq!(
                game.tridents[0].random.clone().next_float(),
                expect.next_float()
            );
            assert_eq!(game.world.random.clone().next_float(), level);
            // No owner for a trident a player threw: refused, nothing changed.
            let velocity = game.tridents[0].velocity;
            assert!(!trident.deflect(game, NativeDeflection::Reverse, None, None, false, [1.0; 3]));
            assert_eq!(game.tridents[0].velocity, velocity);

            // A rocket: owner, then REVERSE, then an owner of none.
            assert_eq!(rocket.owner(game), None);
            assert!(rocket.set_owner(game, Some(mob)));
            assert_eq!(rocket.owner(game), Some(mob));
            assert_eq!(game.rockets[0].owner, Some(Who::Mob(99)));
            let _ = rocket_random.next_float();
            assert!(rocket.deflect(game, NativeDeflection::Reverse, None, None, false, [0.5; 3]));
            assert_eq!(game.rockets[0].velocity, Vec3::new(-0.5, -1.0, -1.5));
            assert_eq!(
                game.rockets[0].random.clone().next_float(),
                rocket_random.next_float()
            );
            assert_eq!(rocket.owner(game), None);
        });
    }

    #[test]
    #[ignore = "needs prepared resources"]
    fn deflection_follows_vanilla_for_wind_charges_owners_random_and_known_speed() {
        with_game(|game| {
            game.options.dedicated = Some(Vec::new());
            let player = player(game);
            let kind = game
                .res
                .mob_types()
                .get("minecraft:zombie")
                .unwrap()
                .clone();
            game.mobs.push(Mob::new(99, kind, Vec3::ZERO, 0.0));
            let mob = game.native_entity(99).unwrap();

            // A wind charge refuses for its first five ticks, with no change and no draw.
            let mut charge =
                Projectile::new(ProjectileKind::WindCharge, Vec3::ZERO, Some(Who::Mob(99)));
            charge.velocity = Vec3::new(0.0, 1.0, 0.0);
            game.add_projectile(charge);
            let charge = game
                .native_projectile_ref(
                    game.native_entity(game.creatures.projectiles[0].id)
                        .unwrap(),
                )
                .unwrap();
            let random = game.creatures.projectiles[0].random.0.clone().next_float();
            for left in 1..=5 {
                game.creatures.projectiles[0].no_deflect_ticks = left;
                assert!(!charge.deflect(
                    game,
                    NativeDeflection::Reverse,
                    None,
                    None,
                    true,
                    [1.0; 3]
                ));
                let p = &game.creatures.projectiles[0];
                assert_eq!(p.velocity, Vec3::new(0.0, 1.0, 0.0));
                assert_eq!(p.owner, Some(Who::Mob(99)));
                assert_eq!(p.random.0.clone().next_float(), random);
            }
            game.creatures.projectiles[0].no_deflect_ticks = 0;

            // REVERSE of a non-arrow takes one float from its own random, not the level's.
            let level = game.world.random.clone().next_float();
            let mut expect = game.creatures.projectiles[0].random.0.clone();
            let _ = expect.next_float();
            assert!(charge.deflect(game, NativeDeflection::Reverse, None, None, true, [1.0; 3]));
            assert_eq!(game.creatures.projectiles[0].owner, None);
            assert_eq!(game.world.random.clone().next_float(), level);
            assert_eq!(
                game.creatures.projectiles[0].random.0.clone().next_float(),
                expect.next_float()
            );

            // Setting a disallowed arrow's owner to a player allows pickup; `deflect`
            // takes the other overload and does not.
            let mut shot = arrow(Who::Mob(99));
            shot.velocity = Vec3::new(0.0, 0.0, 1.0);
            game.add_projectile(shot);
            let arrow_ref = game
                .native_projectile_ref(
                    game.native_entity(game.creatures.projectiles[1].id)
                        .unwrap(),
                )
                .unwrap();
            assert_eq!(arrow_ref.pickup(game), Some(NativeArrowPickup::Disallowed));
            assert!(arrow_ref.deflect(
                game,
                NativeDeflection::Reverse,
                None,
                Some(player),
                false,
                [1.0; 3]
            ));
            assert_eq!(arrow_ref.pickup(game), Some(NativeArrowPickup::Disallowed));
            assert!(arrow_ref.set_owner(game, Some(player)));
            assert_eq!(arrow_ref.pickup(game), Some(NativeArrowPickup::Allowed));

            // MOMENTUM_DEFLECT normalizes the known speed (the last move), not the velocity.
            game.mobs[0].previous = Vec3::new(0.0, 0.0, -3.0);
            game.mobs[0].position = Vec3::ZERO;
            game.mobs[0].velocity = Vec3::new(5.0, 0.0, 0.0);
            assert!(arrow_ref.deflect(
                game,
                NativeDeflection::MomentumDeflect,
                Some(mob),
                None,
                true,
                [2.0; 3]
            ));
            assert_eq!(
                game.creatures.projectiles[1].velocity,
                Vec3::new(0.0, 0.0, 2.0)
            );
        });
    }

    #[test]
    #[ignore = "needs prepared resources"]
    fn momentum_follows_a_boats_displacement_and_a_refused_blow_still_attacks() {
        with_game(|game| {
            game.options.dedicated = Some(Vec::new());
            // A boat that moved along +Z while its velocity points along +X.
            let id = game.take_entity_id();
            let mut boat = super::super::boats::Boat::new(id, "oak".into(), Vec3::ZERO, 0.0);
            boat.previous = Vec3::new(0.0, 0.0, -2.0);
            boat.position = Vec3::ZERO;
            boat.velocity = Vec3::new(5.0, 0.0, 0.0);
            game.boats.push(boat);
            let boat = game.native_entity(id).unwrap();
            assert_eq!(
                game.native_entity_known_speed(boat),
                Some(Vec3::new(0.0, 0.0, 2.0))
            );
            let mut shot = arrow(Who::Mob(99));
            shot.velocity = Vec3::new(1.0, 0.0, 0.0);
            game.add_projectile(shot);
            let arrow_ref = game
                .native_projectile_ref(
                    game.native_entity(game.creatures.projectiles[0].id)
                        .unwrap(),
                )
                .unwrap();
            assert!(arrow_ref.deflect(
                game,
                NativeDeflection::MomentumDeflect,
                Some(boat),
                None,
                true,
                [3.0; 3]
            ));
            assert_eq!(
                game.creatures.projectiles[0].velocity,
                Vec3::new(0.0, 0.0, 3.0)
            );

            // A player's blow at a new wind charge: refused, yet the attack
            // charge resets and nothing about the charge changes.
            let mut charge =
                Projectile::new(ProjectileKind::WindCharge, Vec3::ZERO, Some(Who::Mob(99)));
            charge.velocity = Vec3::new(0.0, 1.0, 0.0);
            game.add_projectile(charge);
            let i = game.creatures.projectiles.len() - 1;
            game.attack_ticks = 50;
            assert!(!game.deflect(i));
            assert_eq!(game.attack_ticks, 0);
            assert_eq!(
                game.creatures.projectiles[i].velocity,
                Vec3::new(0.0, 1.0, 0.0)
            );
            assert_eq!(game.creatures.projectiles[i].owner, Some(Who::Mob(99)));
            game.creatures.projectiles[i].no_deflect_ticks = 0;
            game.attack_ticks = 50;
            assert!(game.deflect(i));
            assert_eq!(game.attack_ticks, 0);
            assert_eq!(game.creatures.projectiles[i].owner, Some(Who::Player));
        });
    }
}
