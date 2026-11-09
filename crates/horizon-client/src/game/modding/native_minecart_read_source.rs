//! Minecart projections shared by resident and concrete lent-source readers.
//! Receiver/subtype admission happens before constructing this view.
use super::{
    Game,
    minecarts::{CartKind, Minecart, display_offset},
};
use horizon_world::registry::BlockStateId;
use mod_api::NativeMinecartKind;

pub(super) struct MinecartRead<'a, 'w> {
    pub(super) game: &'a Game<'w>,
    pub(super) cart: &'a Minecart,
}
impl MinecartRead<'_, '_> {
    pub(super) fn minecart_kind(&self) -> NativeMinecartKind {
        match self.cart.kind {
            CartKind::Rideable => NativeMinecartKind::Rideable,
            CartKind::Chest => NativeMinecartKind::Chest,
            CartKind::Furnace => NativeMinecartKind::Furnace,
            CartKind::Tnt => NativeMinecartKind::Tnt,
            CartKind::Hopper => NativeMinecartKind::Hopper,
            CartKind::CommandBlock => NativeMinecartKind::CommandBlock,
            CartKind::Spawner => NativeMinecartKind::Spawner,
        }
    }
    pub(super) fn minecart_max_speed(&self) -> f64 {
        self.game.cart_max_speed(self.cart)
    }
    pub(super) fn minecart_is_on_rails(&self) -> bool {
        self.cart.on_rails
    }
    pub(super) fn minecart_flipped(&self) -> bool {
        self.cart.flipped
    }
    pub(super) fn minecart_damage(&self) -> f32 {
        self.cart.damage
    }
    pub(super) fn minecart_hurt_time(&self) -> i32 {
        self.cart.hurt_time
    }
    pub(super) fn minecart_custom_display_block(&self) -> Option<BlockStateId> {
        self.cart
            .custom_display
            .as_ref()
            .and_then(|tag| self.game.state_from_nbt(tag))
    }
    pub(super) fn minecart_display_offset(&self) -> i32 {
        display_offset(self.cart) as i32
    }
    pub(super) fn minecart_fuel(&self) -> i32 {
        self.cart.fuel
    }
    pub(super) fn minecart_push(&self) -> [f64; 3] {
        [self.cart.push.x, self.cart.push.y, self.cart.push.z]
    }
    pub(super) fn minecart_fuse(&self) -> i32 {
        self.cart.fuse
    }
    pub(super) fn minecart_hopper_enabled(&self) -> bool {
        self.cart.enabled
    }
    pub(super) fn minecart_command(&self) -> String {
        self.cart.command.command.clone()
    }
}
