//! Save visibility while authoritative rows are lent out.
use super::*;
use crate::game::commands;

impl Game<'_> {
    /// Accepted damage is distinct from survival, destruction, or TNT priming.
    pub(in crate::game) fn hurt_cart_result(
        &mut self,
        i: usize,
        mut damage: f32,
        creative: bool,
        fire: bool,
    ) -> bool {
        let join = self.carts[i].join;
        let id = self.carts[i].id;
        let kind = self.carts[i].kind.id();
        let mut cancel = false;
        self.native_vehicle_damage(kind, &mut damage, &mut cancel);
        if cancel {
            return false;
        }
        let Some(i) = self
            .carts
            .locate(join)
            .filter(|_| !self.persist.removals.contains(&commands::Target::Cart(id)))
        else {
            return false;
        };
        let destroyed = self.carts.update(i, |c| {
            c.hurt_dir = -c.hurt_dir;
            c.hurt_time = 10;
            c.damage += damage * 10.0;
            // MinecartTNT.shouldSourceDestroy: fire and explosions destroy
            // (so ignite) a TNT cart whatever damage it has taken.
            cart_is_destroyed(creative, c.damage, fire && c.kind == CartKind::Tnt)
        });
        if destroyed {
            let speed = self.carts[i].velocity.x * self.carts[i].velocity.x
                + self.carts[i].velocity.z * self.carts[i].velocity.z;
            let tnt_prime =
                self.carts[i].kind == CartKind::Tnt && (fire || speed >= f64::from(0.01f32));
            if !tnt_prime {
                self.eject_cart_mobs(i);
            }
            let id = self.carts[i].id;
            if !tnt_prime {
                self.release_vehicle_container(id);
            }
            if !tnt_prime {
                let at = [
                    self.carts[i].position.x,
                    self.carts[i].position.y,
                    self.carts[i].position.z,
                ];
                self.native_vehicle_destroy(self.carts[i].kind.id(), at);
            }
            let Some(i) = self
                .carts
                .locate(join)
                .filter(|_| !self.persist.removals.contains(&commands::Target::Cart(id)))
            else {
                return true;
            };
            if tnt_prime {
                // MinecartTNT.destroy: event and sound precede both fuse RNG draws.
                if self.carts[i].fuse < 0 {
                    self.prime_cart_join(join);
                    if let Some(at) = self.carts.locate(join) {
                        self.carts.update(at, |c| {
                            c.fuse = c.random.next_int_bound(20).unwrap_or(0)
                                + c.random.next_int_bound(20).unwrap_or(0)
                        });
                    }
                }
            } else {
                let c = self.carts.remove(i);
                let at = [c.position.x, c.position.y, c.position.z];
                // AbstractMinecartContainer.remove: `Containers.dropContents`
                // for every destroying removal (the kill below, and a creative
                // hit's discard), with no `entity_drops` gate of its own.
                self.drop_vehicle_contents(at, &c.items);
                // VehicleEntity.destroy: `kill` (the cart's own ENTITY_DIE
                // after that removal), then the drops; a creative hit only
                // discards it.
                if !creative {
                    self.entity_killed_event(at, false);
                }
                if !creative
                    && self
                        .ui
                        .rules
                        .bool_at(horizon_client::gamerules::vanilla::ENTITY_DROPS)
                {
                    // `spawnAtLocation`: an ItemEntity's own motion.
                    self.spawn_item_own(ItemStack::new(c.kind.item(), 1), at, 10);
                    // `chestVehicleDestroyed` drops the emptied container
                    // again, which only draws each slot's spot.
                    let empty = vec![None; c.items.len()];
                    self.drop_vehicle_contents(at, &empty);
                }
            }
        }
        true
    }
}
