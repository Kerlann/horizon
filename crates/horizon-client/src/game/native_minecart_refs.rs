//! Minecart handles for native server mods.
use super::{
    Game,
    commands::Target,
    minecarts::{CartKind, Minecart},
};
use horizon_client::mod_api::{EntityRef, MinecartRef, NativeMinecartKind};

impl Game<'_> {
    fn native_minecart_index(&self, cart: MinecartRef) -> Option<usize> {
        let Target::Cart(id) = self.native_entity_target(cart.entity())? else {
            return None;
        };
        self.carts.iter().position(|c| c.id == id)
    }

    fn native_minecart(&self, cart: MinecartRef) -> Option<&Minecart> {
        self.carts.get(self.native_minecart_index(cart)?)
    }

    /// Edits a cart, of one kind if `kind` is given. False for another kind
    /// or a handle that no longer resolves. A cart taken out for its own tick:
    /// a write during that tick (e.g. from a Part callback) is lost when the
    /// tick stores its copy.
    fn native_minecart_edit(
        &mut self,
        cart: MinecartRef,
        kind: Option<CartKind>,
        edit: impl FnOnce(&mut Minecart),
    ) -> bool {
        let Some(index) = self.native_minecart_index(cart) else {
            return false;
        };
        if kind.is_some_and(|kind| self.carts[index].kind != kind) {
            return false;
        }
        edit(&mut self.carts[index]);
        true
    }

    /// Narrows an entity to a minecart of any kind, or None for another kind of
    /// entity, a stale handle, a client, or a cart taken out for ticking.
    pub(super) fn native_minecart_ref(&self, entity: EntityRef) -> Option<MinecartRef> {
        let cart = MinecartRef::new(entity);
        self.native_minecart_index(cart).map(|_| cart)
    }

    /// Which `AbstractMinecart` subclass the cart is.
    pub(super) fn native_minecart_kind(&self, cart: MinecartRef) -> Option<NativeMinecartKind> {
        Some(match self.native_minecart(cart)?.kind {
            CartKind::Rideable => NativeMinecartKind::Rideable,
            CartKind::Chest => NativeMinecartKind::Chest,
            CartKind::Furnace => NativeMinecartKind::Furnace,
            CartKind::Tnt => NativeMinecartKind::Tnt,
            CartKind::Hopper => NativeMinecartKind::Hopper,
            CartKind::CommandBlock => NativeMinecartKind::CommandBlock,
            CartKind::Spawner => NativeMinecartKind::Spawner,
        })
    }

    /// `AbstractMinecart.getMaxSpeed`, in blocks per tick: the old behaviour's
    /// 0.4 (0.2 in water), or with the `minecart_improvements` experiment the
    /// `max_minecart_speed` rule over 20 (halved in water). A furnace minecart
    /// takes half of that, three quarters in water (`MinecartFurnace`).
    pub(super) fn native_minecart_max_speed(&self, cart: MinecartRef) -> Option<f64> {
        Some(self.cart_max_speed(self.native_minecart(cart)?))
    }

    /// `AbstractMinecart.isOnRails`.
    pub(super) fn native_minecart_is_on_rails(&self, cart: MinecartRef) -> Option<bool> {
        Some(self.native_minecart(cart)?.on_rails)
    }

    /// `AbstractMinecart.isFlipped`.
    pub(super) fn native_minecart_flipped(&self, cart: MinecartRef) -> Option<bool> {
        Some(self.native_minecart(cart)?.flipped)
    }

    /// `AbstractMinecart.setFlipped`; saved as `FlippedRotation`, not synced.
    /// A cart taken out for its own tick: a write during that tick (e.g. from
    /// a Part callback) is lost when the tick stores its copy.
    pub(super) fn native_minecart_set_flipped(&mut self, cart: MinecartRef, flipped: bool) -> bool {
        self.native_minecart_edit(cart, None, |c| c.flipped = flipped)
    }

    /// `VehicleEntity.getDamage`, the wobble damage (`DATA_ID_DAMAGE`).
    pub(super) fn native_minecart_damage(&self, cart: MinecartRef) -> Option<f32> {
        Some(self.native_minecart(cart)?.damage)
    }

    /// `VehicleEntity.setDamage`: synced to clients. The tick counts it down
    /// only while positive.
    /// A cart taken out for its own tick: a write during that tick (e.g. from
    /// a Part callback) is lost when the tick stores its copy.
    pub(super) fn native_minecart_set_damage(&mut self, cart: MinecartRef, damage: f32) -> bool {
        self.native_minecart_edit(cart, None, |c| c.damage = damage)
    }

    /// `VehicleEntity.getHurtTime`, the wobble ticks (`DATA_ID_HURT`).
    pub(super) fn native_minecart_hurt_time(&self, cart: MinecartRef) -> Option<i32> {
        Some(self.native_minecart(cart)?.hurt_time)
    }

    /// `VehicleEntity.setHurtTime`: synced to clients.
    /// A cart taken out for its own tick: a write during that tick (e.g. from
    /// a Part callback) is lost when the tick stores its copy.
    pub(super) fn native_minecart_set_hurt_time(&mut self, cart: MinecartRef, ticks: i32) -> bool {
        self.native_minecart_edit(cart, None, |c| c.hurt_time = ticks)
    }

    /// `MinecartFurnace`'s fuel ticks; None for another kind of cart.
    pub(super) fn native_minecart_fuel(&self, cart: MinecartRef) -> Option<i32> {
        let cart = self.native_minecart(cart)?;
        (cart.kind == CartKind::Furnace).then_some(cart.fuel)
    }

    /// Sets a furnace minecart's fuel ticks, any int as vanilla's field holds
    /// it (saved as a short, the low 16 bits). False for another kind of cart.
    /// The lit flag (`DATA_ID_FUEL`) follows with fuel above zero.
    /// A cart taken out for its own tick: a write during that tick (e.g. from
    /// a Part callback) is lost when the tick stores its copy.
    pub(super) fn native_minecart_set_fuel(&mut self, cart: MinecartRef, fuel: i32) -> bool {
        self.native_minecart_edit(cart, Some(CartKind::Furnace), |c| c.fuel = fuel)
    }

    /// `MinecartTNT`'s fuse ticks, -1 when it is not primed; None for another
    /// kind of cart.
    pub(super) fn native_minecart_fuse(&self, cart: MinecartRef) -> Option<i32> {
        let cart = self.native_minecart(cart)?;
        (cart.kind == CartKind::Tnt).then_some(cart.fuse)
    }

    /// `MinecartTNT.primeFuse(null)`: an 80 tick fuse, the primed sound and
    /// entity event 70 for clients. False for another kind of cart, or with
    /// the `tnt_explodes` rule off.
    /// A cart taken out for its own tick: a write during that tick (e.g. from
    /// a Part callback) is lost when the tick stores its copy.
    pub(super) fn native_minecart_prime_fuse(&mut self, cart: MinecartRef) -> bool {
        let Some(index) = self.native_minecart_index(cart) else {
            return false;
        };
        if self.carts[index].kind != CartKind::Tnt || !self.ui.rules.bool("tnt_explodes") {
            return false;
        }
        let mut primed = self.carts[index].clone();
        self.prime_cart(&mut primed);
        self.native_minecart_edit(cart, Some(CartKind::Tnt), |c| c.fuse = primed.fuse)
    }

    /// `MinecartHopper.isEnabled`; None for another kind of cart.
    pub(super) fn native_minecart_hopper_enabled(&self, cart: MinecartRef) -> Option<bool> {
        let cart = self.native_minecart(cart)?;
        (cart.kind == CartKind::Hopper).then_some(cart.enabled)
    }

    /// `MinecartHopper.setEnabled`: saved as `Enabled`, not synced.
    /// A cart taken out for its own tick: a write during that tick (e.g. from
    /// a Part callback) is lost when the tick stores its copy.
    pub(super) fn native_minecart_set_hopper_enabled(
        &mut self,
        cart: MinecartRef,
        enabled: bool,
    ) -> bool {
        self.native_minecart_edit(cart, Some(CartKind::Hopper), |c| c.enabled = enabled)
    }
}

#[cfg(test)]
mod tests {
    use super::super::native_entity_refs::tests::with_game;
    use super::*;
    use horizon_client::{entities::ArmorStand, mod_api::NativeWorldEpoch};
    use horizon_core::geometry::Vec3;
    use horizon_world::nbt::Tag;

    fn spawn(game: &mut Game<'_>, id: u32, kind: CartKind) -> MinecartRef {
        game.carts.push(Minecart::new(id, kind, Vec3::ZERO));
        game.native_minecart_ref(game.native_entity(id).unwrap())
            .unwrap()
    }

    #[test]
    fn furnace_fuel_adds_with_java_int_arithmetic() {
        assert_eq!(super::super::minecarts::furnace_fuel_after(0), Some(3600));
        assert_eq!(
            super::super::minecarts::furnace_fuel_after(28_400),
            Some(32_000)
        );
        assert_eq!(super::super::minecarts::furnace_fuel_after(28_401), None);
        assert_eq!(
            super::super::minecarts::furnace_fuel_after(i32::MAX),
            Some(i32::MIN + 3599)
        );
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn minecart_handles_narrow_read_and_write_by_kind() {
        with_game(|game| {
            game.options.dedicated = Some(Vec::new());
            let kinds = [
                (CartKind::Rideable, NativeMinecartKind::Rideable),
                (CartKind::Chest, NativeMinecartKind::Chest),
                (CartKind::Furnace, NativeMinecartKind::Furnace),
                (CartKind::Tnt, NativeMinecartKind::Tnt),
                (CartKind::Hopper, NativeMinecartKind::Hopper),
                (CartKind::CommandBlock, NativeMinecartKind::CommandBlock),
                (CartKind::Spawner, NativeMinecartKind::Spawner),
            ];
            let carts: Vec<_> = kinds
                .iter()
                .zip(10..)
                .map(|((kind, _), id)| spawn(game, id, *kind))
                .collect();
            for (cart, (_, expected)) in carts.iter().zip(kinds) {
                assert_eq!(cart.kind(game), Some(expected));
            }
            let [rideable, _, furnace, tnt, hopper, ..] = carts[..] else {
                unreachable!()
            };
            game.armor_stands.push(ArmorStand::new(99, Vec3::ZERO, 0.0));
            assert!(
                game.native_minecart_ref(game.native_entity(99).unwrap())
                    .is_none()
            );

            // Max speed: old behaviour, water, furnace, then the experiment's rule.
            assert_eq!(rideable.max_speed(game), Some(0.4));
            assert_eq!(furnace.max_speed(game), Some(0.2));
            game.carts[2].in_water = true;
            assert_eq!(furnace.max_speed(game), Some(0.2 * 0.75));
            game.carts[2].in_water = false;
            game.level
                .enabled_features
                .push("minecraft:minecart_improvements".into());
            game.ui.rules =
                horizon_client::gamerules::GameRules::with_features(&game.level.enabled_features);
            assert_eq!(rideable.max_speed(game), Some(8.0 / 20.0));
            assert_eq!(furnace.max_speed(game), Some(8.0 / 20.0 * 0.5));
            game.carts[0].in_water = true;
            assert_eq!(rideable.max_speed(game), Some(8.0 / 20.0 * 0.5));

            assert_eq!(rideable.is_on_rails(game), Some(false));
            game.carts[0].on_rails = true;
            assert_eq!(rideable.is_on_rails(game), Some(true));

            assert!(rideable.set_flipped(game, true));
            assert_eq!(rideable.flipped(game), Some(true));
            assert!(rideable.set_damage(game, 12.5));
            assert!(rideable.set_hurt_time(game, 7));
            assert_eq!(
                (rideable.damage(game), rideable.hurt_time(game)),
                (Some(12.5), Some(7))
            );
            let saved = game.carts[0].to_nbt([0; 4], None);
            assert_eq!(saved.get("FlippedRotation"), Some(&Tag::Byte(1)));

            // Furnace fuel, saved and synced as the lit flag.
            assert_eq!(furnace.fuel(game), Some(0));
            assert!(furnace.set_fuel(game, 3600));
            assert_eq!(furnace.fuel(game), Some(3600));
            assert!(!tnt.set_fuel(game, 5));
            assert_eq!(tnt.fuel(game), None);
            let saved = game.carts[2].to_nbt([0; 4], None);
            assert_eq!(saved.get("Fuel"), Some(&Tag::Short(3600)));
            // The field is an int; only the save casts to a short.
            assert!(furnace.set_fuel(game, 40_000));
            assert_eq!(furnace.fuel(game), Some(40_000));
            assert_eq!(
                game.carts[2].to_nbt([0; 4], None).get("Fuel"),
                Some(&Tag::Short(40_000_i32 as i16))
            );
            assert!(furnace.set_fuel(game, i32::MIN));
            assert_eq!(furnace.fuel(game), Some(i32::MIN));
            assert!(furnace.set_fuel(game, 3600));

            // TNT fuse.
            assert_eq!(tnt.fuse(game), Some(-1));
            game.audio.capture = Some(Vec::new());
            assert!(tnt.prime_fuse(game));
            assert_eq!(tnt.fuse(game), Some(80));
            // MinecartTNT.primeFuse broadcasts entity event 70 for clients.
            assert!(
                game.lan_entity_events
                    .iter()
                    .any(|e| (e.0, e.1) == (tnt.entity().id() as i32, 70))
            );
            game.audio.capture = None;
            assert_eq!(
                game.carts[3].to_nbt([0; 4], None).get("fuse"),
                Some(&Tag::Int(80))
            );
            assert!(!furnace.prime_fuse(game));
            game.ui.rules.set(
                "tnt_explodes",
                horizon_client::gamerules::Value::Bool(false),
            );
            game.carts[3].fuse = -1;
            assert!(!tnt.prime_fuse(game));
            assert_eq!(tnt.fuse(game), Some(-1));

            // The tick counts wobble down only while positive.
            assert!(rideable.set_hurt_time(game, i32::MIN));
            assert!(rideable.set_damage(game, 0.0));
            game.tick_carts();
            assert_eq!(rideable.hurt_time(game), Some(i32::MIN));
            assert!(rideable.set_hurt_time(game, -1));
            assert!(rideable.set_damage(game, -0.5));
            game.tick_carts();
            assert_eq!(
                (rideable.hurt_time(game), rideable.damage(game)),
                (Some(-1), Some(-0.5))
            );
            assert!(rideable.set_hurt_time(game, 3));
            assert!(rideable.set_damage(game, 2.5));
            game.tick_carts();
            assert_eq!(
                (rideable.hurt_time(game), rideable.damage(game)),
                (Some(2), Some(1.5))
            );

            // Hopper.
            assert_eq!(hopper.hopper_enabled(game), Some(true));
            assert!(hopper.set_hopper_enabled(game, false));
            assert_eq!(
                game.carts[4].to_nbt([0; 4], None).get("Enabled"),
                Some(&Tag::Byte(0))
            );
            assert!(!rideable.set_hopper_enabled(game, false));
            assert_eq!(rideable.hopper_enabled(game), None);

            let stale = MinecartRef::new(EntityRef::new(
                NativeWorldEpoch::new(rideable.entity().epoch().get() + 1),
                rideable.entity().id(),
            ));
            assert_eq!(stale.kind(game), None);
            assert!(!stale.set_flipped(game, false));
            assert_eq!(stale.max_speed(game), None);
        });
    }
}
