//! An entity's invulnerability for native server mods, through an
//! `EntityRef` (docs/native-mods.md, "Entity and level handles"): the
//! `Invulnerable` flag (`Entity.permanentlyInvulnerable`) and
//! `LivingEntity.damageCooldownTime`, the ticks after a hit in which a
//! weaker one does only the difference. Horizon has no counterpart yet of
//! 26.3's own `Entity.invulnerableTime` timer, so neither is offered. Reads
//! return None and writes false for a stale handle, on a client, and for a
//! kind Horizon keeps no such state for.
use super::{Game, commands::Target};
use horizon_client::mod_api::EntityRef;

impl Game<'_> {
    /// A player's body, where it is kept now.
    fn native_player_body(&self, t: Target) -> Option<&horizon_client::motion::PlayerMotion> {
        match t {
            Target::Player => self.player.as_ref(),
            Target::Guest(conn) => self.lan.as_ref()?.hub.guest(conn)?.motion.as_deref(),
            _ => None,
        }
    }
    fn native_player_body_mut(
        &mut self,
        t: Target,
    ) -> Option<&mut horizon_client::motion::PlayerMotion> {
        match t {
            Target::Player => self.player.as_mut(),
            Target::Guest(conn) => self
                .lan
                .as_mut()?
                .hub
                .guest_mut(conn)?
                .motion
                .as_deref_mut(),
            _ => None,
        }
    }

    /// `Entity.isPermanentlyInvulnerable`: the `Invulnerable` flag, which
    /// only damage that bypasses invulnerability (the void, `/kill`) or a
    /// creative player gets past. Mobs keep it; Horizon keeps no such flag
    /// for players or other kinds yet, so they read None.
    pub(super) fn native_entity_permanently_invulnerable(&self, entity: EntityRef) -> Option<bool> {
        match self.native_entity_target(entity)? {
            Target::Mob(id) => Some(self.mobs.iter().find(|m| m.id == id)?.invulnerable_flag),
            _ => None,
        }
    }
    /// `Entity.setInvulnerable`: sets a mob's `Invulnerable` flag, saved
    /// with it. False for the other kinds.
    pub(super) fn native_entity_set_invulnerable(
        &mut self,
        entity: EntityRef,
        invulnerable: bool,
    ) -> bool {
        let Some(Target::Mob(id)) = self.native_entity_target(entity) else {
            return false;
        };
        let Some(m) = self.mobs.iter_mut().find(|m| m.id == id) else {
            return false;
        };
        m.invulnerable_flag = invulnerable;
        true
    }
    /// `LivingEntity.damageCooldownTime`: 20 after a hit, counting down;
    /// while it is above 10 a hit does only what it deals beyond the last
    /// one. Mobs and players.
    pub(super) fn native_entity_damage_cooldown_time(&self, entity: EntityRef) -> Option<i32> {
        match self.native_entity_target(entity)? {
            Target::Mob(id) => Some(self.mobs.iter().find(|m| m.id == id)?.invulnerable),
            t => Some(self.native_player_body(t)?.health.hurt.cooldown),
        }
    }
    /// Sets `LivingEntity.damageCooldownTime`, as mods do to let rapid hits
    /// through (0). Mobs and players.
    pub(super) fn native_entity_set_damage_cooldown_time(
        &mut self,
        entity: EntityRef,
        ticks: i32,
    ) -> bool {
        let Some(t) = self.native_entity_target(entity) else {
            return false;
        };
        match t {
            Target::Mob(id) => match self.mobs.iter_mut().find(|m| m.id == id) {
                Some(m) => {
                    m.invulnerable = ticks;
                    true
                }
                None => false,
            },
            t => match self.native_player_body_mut(t) {
                Some(p) => {
                    p.health.hurt.cooldown = ticks;
                    true
                }
                None => false,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::native_entity_refs::tests::with_game;
    use horizon_client::mobs::Mob;
    use horizon_client::mod_api::{EntityRef, NativeWorldEpoch};
    use horizon_core::geometry::Vec3;

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn a_mob_keeps_its_flag_and_damage_cooldown() {
        with_game(|game| {
            let zombie = game
                .res
                .mob_types()
                .get("minecraft:zombie")
                .unwrap()
                .clone();
            game.mobs.push(Mob::new(12, zombie, Vec3::ZERO, 0.0));
            let mob = game.native_entity(12).unwrap();
            assert_eq!(
                game.native_entity_permanently_invulnerable(mob),
                Some(false)
            );
            assert!(game.native_entity_set_invulnerable(mob, true));
            assert_eq!(game.native_entity_permanently_invulnerable(mob), Some(true));
            assert!(game.mobs[0].invulnerable_flag);
            assert_eq!(game.native_entity_damage_cooldown_time(mob), Some(0));
            assert!(game.native_entity_set_damage_cooldown_time(mob, 20));
            assert_eq!(game.mobs[0].invulnerable, 20);
            assert_eq!(game.native_entity_damage_cooldown_time(mob), Some(20));
            let stale = EntityRef::new(NativeWorldEpoch::new(mob.epoch().get() + 1), 12);
            assert_eq!(game.native_entity_permanently_invulnerable(stale), None);
            assert!(!game.native_entity_set_invulnerable(stale, false));
            assert_eq!(game.native_entity_damage_cooldown_time(stale), None);
            assert!(!game.native_entity_set_damage_cooldown_time(stale, 0));
            assert!(game.mobs[0].invulnerable_flag);
        });
    }
}
