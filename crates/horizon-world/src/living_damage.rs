//! Inner living/player damage application after the outer hurt gates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DamageStat {
    Absorbed,
    DealtAbsorbed,
    Taken,
}
/// What took a share of a hit before the health, as NeoForge's `DamageContainer.Reduction`
/// names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Reduction {
    /// The damage cooldown: what the last hit already dealt (`lastHurt`), taken off a hit
    /// that lands within it.
    Invulnerability,
    /// Armor and toughness (`getDamageAfterArmorAbsorb`).
    Armor,
    /// Protection enchantments (`EnchantmentHelper.getDamageProtection`).
    Enchantments,
    /// Resistance (`getDamageAfterMagicAbsorb`'s mob effect step).
    MobEffects,
    /// The absorption hearts.
    Absorption,
    /// What the entity's own kind resists: a witch's magic resistance.
    InnateResistance,
}
const REDUCTIONS: usize = 6;
/// One hit's damage sequence from `hurtServer` to the health, as NeoForge's
/// `DamageContainer` carries it: the damage that came in, what blocking, the cooldown and
/// each reduction took, the damage `LivingDamageEvent.Pre` left and what the health lost.
/// Numeric values are 0 until the step that sets them has run; side effects are false until
/// `hurtServer` decides whether the hit is fresh.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DamageContainer {
    original: f32,
    new_damage: f32,
    blocked: f32,
    shield_damage: f32,
    post_attack_invulnerability_ticks: i32,
    should_cause_side_effects: bool,
    reductions: [f32; REDUCTIONS],
    entered_pre: f32,
    inflicted: f32,
    health_damage: f32,
}
/// A hit's damage sequence as plain values, field for field: the form it crosses into a wasm
/// mod as.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DamageValues {
    pub original: f32,
    pub new_damage: f32,
    pub blocked: f32,
    pub shield_damage: f32,
    pub post_attack_invulnerability_ticks: i32,
    pub should_cause_side_effects: bool,
    pub reductions: [f32; 6],
    pub entered_pre: f32,
    pub inflicted: f32,
    pub health_damage: f32,
}

impl From<&DamageContainer> for DamageValues {
    fn from(d: &DamageContainer) -> Self {
        Self {
            original: d.original,
            new_damage: d.new_damage,
            blocked: d.blocked,
            shield_damage: d.shield_damage,
            post_attack_invulnerability_ticks: d.post_attack_invulnerability_ticks,
            should_cause_side_effects: d.should_cause_side_effects,
            reductions: d.reductions,
            entered_pre: d.entered_pre,
            inflicted: d.inflicted,
            health_damage: d.health_damage,
        }
    }
}

/// A damage sequence from its values (the form a wasm mod gives it back in).
impl From<DamageValues> for DamageContainer {
    fn from(v: DamageValues) -> Self {
        Self {
            original: v.original,
            new_damage: v.new_damage,
            blocked: v.blocked,
            shield_damage: v.shield_damage,
            post_attack_invulnerability_ticks: v.post_attack_invulnerability_ticks,
            should_cause_side_effects: v.should_cause_side_effects,
            reductions: v.reductions,
            entered_pre: v.entered_pre,
            inflicted: v.inflicted,
            health_damage: v.health_damage,
        }
    }
}

impl DamageContainer {
    /// A hit of `original` damage, as `hurtServer` was given it, with vanilla's 20 ticks of
    /// damage cooldown after it.
    pub fn new(original: f32) -> Self {
        Self {
            original,
            new_damage: original,
            blocked: 0.0,
            shield_damage: 0.0,
            post_attack_invulnerability_ticks: 20,
            should_cause_side_effects: false,
            reductions: [0.0; REDUCTIONS],
            entered_pre: 0.0,
            inflicted: 0.0,
            health_damage: 0.0,
        }
    }
    /// The damage the hit came in with (`getOriginalDamage`), before anything changed it.
    pub fn original(&self) -> f32 {
        self.original
    }
    /// The damage the sequence goes on with (`getNewDamage`).
    pub fn new_damage(&self) -> f32 {
        self.new_damage
    }
    /// Change the damage the sequence goes on with (`setNewDamage`). Set in
    /// `LivingDamageEvent.Pre`, it is what the absorption hearts and the health take: 0 or
    /// less takes nothing, and a value that is not a number is put back as it was.
    pub fn set_new_damage(&mut self, damage: f32) {
        self.new_damage = damage;
    }
    /// The damage a blocking item stopped (`getBlockedDamage`).
    pub fn blocked(&self) -> f32 {
        self.blocked
    }
    /// The durability the raised blocking item lost (`getShieldDamage`).
    pub fn shield_damage(&self) -> f32 {
        self.shield_damage
    }
    /// The damage cooldown the entity has once the hit is applied
    /// (`getPostAttackInvulnerabilityTicks`): 20 for a fresh hit, what is left of it for a hit
    /// within it.
    pub fn post_attack_invulnerability_ticks(&self) -> i32 {
        self.post_attack_invulnerability_ticks
    }
    /// Change the damage cooldown the entity has once the hit is applied
    /// (`setPostAttackInvulnerabilityTicks`).
    pub fn set_post_attack_invulnerability_ticks(&mut self, ticks: i32) {
        self.post_attack_invulnerability_ticks = ticks;
    }
    /// Whether `hurtServer` decided this hit was fresh (`shouldCauseSideEffects`), so it
    /// causes hurt sounds, knockback and other fresh-hit effects. False for a stronger hit
    /// inside the damage cooldown unless its source bypasses the cooldown. This is recorded
    /// before `actuallyHurt`, independently of the final damage or modded cooldown ticks.
    pub fn should_cause_side_effects(&self) -> bool {
        self.should_cause_side_effects
    }
    /// The damage sequence: `hurtServer`'s fresh-hit decision before the damage hooks run.
    #[doc(hidden)]
    pub fn record_should_cause_side_effects(&mut self, fresh: bool) {
        self.should_cause_side_effects = fresh;
    }
    /// What `reduction` took off the hit (`getReduction`).
    pub fn reduction(&self, reduction: Reduction) -> f32 {
        self.reductions[reduction as usize]
    }
    /// The damage `LivingDamageEvent.Pre` left (`getInflictedDamage`), absorption included.
    pub fn inflicted(&self) -> f32 {
        self.inflicted
    }
    /// The health the hit took (`getHealthDamage`).
    pub fn health_damage(&self) -> f32 {
        self.health_damage
    }
    /// The damage sequence: what blocking stopped.
    #[doc(hidden)]
    pub fn record_block(&mut self, blocked: f32) {
        self.blocked = blocked;
    }
    /// The damage sequence: the durability the blocking item lost.
    #[doc(hidden)]
    pub fn record_shield_damage(&mut self, durability: f32) {
        self.shield_damage = durability;
    }
    /// The damage sequence: what `reduction` took.
    #[doc(hidden)]
    pub fn record_reduction(&mut self, reduction: Reduction, amount: f32) {
        self.reductions[reduction as usize] = amount;
    }
    /// The damage sequence, before `LivingDamageEvent.Pre`: the damage it is told.
    #[doc(hidden)]
    pub fn enter_pre(&mut self, damage: f32) {
        self.entered_pre = damage;
        self.new_damage = damage;
    }
    /// The damage sequence, after `LivingDamageEvent.Pre`: the damage it left, which the
    /// sequence goes on with (`captureInflictedDamage`). Below 0 is none; not a number is
    /// what Pre was told.
    #[doc(hidden)]
    pub fn capture_inflicted(&mut self) -> f32 {
        if self.new_damage.is_nan() {
            self.new_damage = self.entered_pre;
        } else if self.new_damage < 0.0 {
            self.new_damage = 0.0;
        }
        self.inflicted = self.new_damage;
        self.inflicted
    }
    /// What `LivingDamageEvent.Pre` added to the damage (negative: took off). The damage
    /// cooldown remembers the hit with it (`lastHurt`).
    #[doc(hidden)]
    pub fn pre_change(&self) -> f32 {
        self.inflicted - self.entered_pre
    }
    /// The damage sequence: the health the hit took.
    #[doc(hidden)]
    pub fn record_health_damage(&mut self, damage: f32) {
        self.health_damage = damage;
    }
}
/// What `getDamageAfterMagicAbsorb` left of a hit, and how much of what it took was the
/// Resistance effect's (the rest is protection enchantments').
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MagicAbsorb {
    pub damage: f32,
    pub mob_effects: f32,
}
pub trait DamageWorld {
    type Error;
    type Attacker: Copy;
    fn invulnerable(&mut self) -> Result<bool, Self::Error>;
    fn after_armor(&mut self, damage: f32) -> Result<f32, Self::Error>;
    fn after_magic(&mut self, damage: f32) -> Result<MagicAbsorb, Self::Error>;
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
    /// `LivingDamageEvent.Pre`: the armor and magic reductions are in `container`, and
    /// the absorption hearts have not taken their share; it may change the new damage. The
    /// default does nothing.
    fn damage_pre(&mut self, _container: &mut DamageContainer) -> Result<(), Self::Error> {
        Ok(())
    }
    /// `LivingDamageEvent.Post`: the health has just lost `container`'s health damage (not
    /// 0). The default does nothing.
    fn damage_post(&mut self, _container: &DamageContainer) -> Result<(), Self::Error> {
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
    container: &mut DamageContainer,
    damage: f32,
    player: bool,
) -> Result<(), W::Error> {
    if world.invulnerable()? {
        return Ok(());
    }
    actually_hurt_after_immunity_check(world, container, damage, player)
}
/// Caller must have performed this hit's native invulnerability query.
pub(crate) fn actually_hurt_after_immunity_check<W: DamageWorld>(
    world: &mut W,
    container: &mut DamageContainer,
    damage: f32,
    player: bool,
) -> Result<(), W::Error> {
    let after_armor = world.after_armor(damage)?;
    container.record_reduction(Reduction::Armor, damage - after_armor);
    let magic = world.after_magic(after_armor)?;
    container.record_reduction(Reduction::MobEffects, magic.mob_effects);
    container.record_reduction(
        Reduction::Enchantments,
        after_armor - magic.mob_effects - magic.damage,
    );
    container.enter_pre(magic.damage);
    world.damage_pre(container)?;
    let damage = container.capture_inflicted();
    let remaining = positive(damage - world.absorption()?);
    let absorption = world.absorption()? - (damage - remaining);
    world.set_absorption(absorption)?;
    let absorbed = damage - remaining;
    container.record_reduction(Reduction::Absorption, absorbed);
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
    if player {
        if remaining < STAT_LIMIT {
            world.award_self(DamageStat::Taken, stat_amount(remaining))?;
        }
    } else {
        let absorption = world.absorption()?;
        world.set_absorption(absorption - remaining)?;
    }
    world.damage_event()?;
    container.record_health_damage(remaining);
    world.damage_post(container)
}
