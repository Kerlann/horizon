//! Validated subjects for source-live entity reads. Handle-family and player
//! UUID checks belong at this boundary, before physical or pending refusal.
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
