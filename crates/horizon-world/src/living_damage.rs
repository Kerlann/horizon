//! Inner living/player damage application after the outer hurt gates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DamageStat {
    Absorbed,
    DealtAbsorbed,
    Taken,
}
/// Damage amounts observed after a successful health change.
#[derive(Clone, Copy, Debug)]
pub struct DamagePost {
    pub armor_reduction: f32,
    pub magic_reduction: f32,
    pub absorbed: f32,
    pub health_damage: f32,
}
pub trait DamageWorld {
    type Error;
    type Attacker: Copy;
    fn invulnerable(&mut self) -> Result<bool, Self::Error>;
    fn after_armor(&mut self, damage: f32) -> Result<f32, Self::Error>;
    fn after_magic(&mut self, damage: f32) -> Result<f32, Self::Error>;
    fn damage_pre(&mut self, _damage: &mut f32) -> Result<(), Self::Error> {
        Ok(())
    }
    fn absorption(&mut self) -> Result<f32, Self::Error>;
    fn set_absorption(&mut self, value: f32) -> Result<(), Self::Error>;
    fn server_player_attacker(&mut self) -> Result<Option<Self::Attacker>, Self::Error>;
    fn award_attacker(
        &mut self,
        attacker: Self::Attacker,
        stat: DamageStat,
        amount: i32,
    ) -> Result<(), Self::Error>;
    fn award_self(&mut self, stat: DamageStat, amount: i32) -> Result<(), Self::Error>;
    fn source_food_exhaustion(&mut self) -> Result<f32, Self::Error>;
    fn cause_food_exhaustion(&mut self, amount: f32) -> Result<(), Self::Error>;
    fn record_damage(&mut self, amount: f32) -> Result<(), Self::Error>;
    fn health(&mut self) -> Result<f32, Self::Error>;
    fn set_health(&mut self, value: f32) -> Result<(), Self::Error>;
    fn damage_event(&mut self) -> Result<(), Self::Error>;
    fn damage_post(&mut self, _post: DamagePost) -> Result<(), Self::Error> {
        Ok(())
    }
}
pub(crate) fn positive(value: f32) -> f32 {
    if value.is_nan() {
        value
    } else if value <= 0.0 {
        0.0
    } else {
        value
    }
}
pub(crate) fn stat_amount(value: f32) -> i32 {
    let value = value * 10.0;
    // Java Math.round(float), including negative ties and integer saturation.
    let bits = value.to_bits() as i32;
    let shift = 149 - ((bits >> 23) & 0xff);
    if (0..32).contains(&shift) {
        let mut significand = (bits & 0x7f_ffff) | 0x80_0000;
        if bits < 0 {
            significand = -significand;
        }
        ((significand >> shift) + 1) >> 1
    } else {
        value as i32
    }
}
pub(crate) const STAT_LIMIT: f32 = 3.402_823_5e37;
pub fn actually_hurt<W: DamageWorld>(
    world: &mut W,
    damage: f32,
    player: bool,
) -> Result<(), W::Error> {
    if world.invulnerable()? {
        return Ok(());
    }
    actually_hurt_after_immunity_check(world, damage, player)
}
/// Caller must have performed this hit's native invulnerability query.
pub(crate) fn actually_hurt_after_immunity_check<W: DamageWorld>(
    world: &mut W,
    damage: f32,
    player: bool,
) -> Result<(), W::Error> {
    let after_armor = world.after_armor(damage)?;
    let armor_reduction = damage - after_armor;
    let mut damage = world.after_magic(after_armor)?;
    let magic_reduction = after_armor - damage;
    world.damage_pre(&mut damage)?;
    let remaining = positive(damage - world.absorption()?);
    let absorption = world.absorption()? - (damage - remaining);
    world.set_absorption(absorption)?;
    let absorbed = damage - remaining;
    if absorbed > 0.0 && absorbed < STAT_LIMIT {
        if player {
            world.award_self(DamageStat::Absorbed, stat_amount(absorbed))?;
        } else if let Some(attacker) = world.server_player_attacker()? {
            world.award_attacker(attacker, DamageStat::DealtAbsorbed, stat_amount(absorbed))?;
        }
    }
    if remaining == 0.0 {
        return Ok(());
    }
    if player {
        let exhaustion = world.source_food_exhaustion()?;
        world.cause_food_exhaustion(exhaustion)?;
    }
    world.record_damage(remaining)?;
    let health = world.health()?;
    world.set_health(health - remaining)?;
    world.damage_post(DamagePost {
        armor_reduction,
        magic_reduction,
        absorbed,
        health_damage: remaining,
    })?;
    if player {
        if remaining < STAT_LIMIT {
            world.award_self(DamageStat::Taken, stat_amount(remaining))?;
        }
    } else {
        let absorption = world.absorption()?;
        world.set_absorption(absorption - remaining)?;
    }
    world.damage_event()
}
