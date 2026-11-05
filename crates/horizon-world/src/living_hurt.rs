//! Outer living damage dispatch, cooldown writes and post-hit service ordering.
use crate::living_damage::{DamageContainer, Reduction, STAT_LIMIT, stat_amount};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HurtTag {
    Fire,
    Freezing,
    DamagesHelmet,
    BypassesCooldown,
    NoImpact,
    NoKnockback,
}
#[derive(Clone, Debug)]
pub struct HurtState<S> {
    pub no_action_time: i32,
    pub cooldown: i32,
    pub last_hurt: f32,
    pub hurt_duration: i32,
    pub hurt_time: i32,
    pub last_source: Option<S>,
    pub last_stamp: i64,
}
/// One invocation retains the same incoming source and server level. Services
/// receive live state so reentrant callbacks preserve native write ordering.
pub trait HurtWorld {
    type Error;
    type Source: Copy;
    type UseItem: Copy;
    type Blocking: Copy;
    type Sound: Copy;
    fn source(&self) -> Self::Source;
    fn is_server_player(&self) -> bool;
    fn invulnerable(&mut self, s: &mut HurtState<Self::Source>) -> Result<bool, Self::Error>;
    fn dead_or_dying(&mut self, s: &mut HurtState<Self::Source>) -> Result<bool, Self::Error>;
    fn tag(&mut self, s: &mut HurtState<Self::Source>, tag: HurtTag) -> Result<bool, Self::Error>;
    fn fire_resistance(&mut self, s: &mut HurtState<Self::Source>) -> Result<bool, Self::Error>;
    /// The hit once the guards above passed (`LivingIncomingDamageEvent`): it may change the
    /// container's new damage and post-attack invulnerability ticks, and true refuses the
    /// hit. The default does nothing.
    fn hurt_start(
        &mut self,
        _s: &mut HurtState<Self::Source>,
        _container: &mut DamageContainer,
    ) -> Result<bool, Self::Error> {
        Ok(false)
    }
    fn sleeping(&mut self, s: &mut HurtState<Self::Source>) -> Result<bool, Self::Error>;
    fn stop_sleeping(&mut self, s: &mut HurtState<Self::Source>) -> Result<(), Self::Error>;
    fn use_item(&mut self, s: &mut HurtState<Self::Source>) -> Result<Self::UseItem, Self::Error>;
    fn apply_blocking(
        &mut self,
        s: &mut HurtState<Self::Source>,
        damage: f32,
    ) -> Result<f32, Self::Error>;
    fn freeze_hurts_extra(&mut self, s: &mut HurtState<Self::Source>) -> Result<bool, Self::Error>;
    fn helmet_empty(&mut self, s: &mut HurtState<Self::Source>) -> Result<bool, Self::Error>;
    fn hurt_helmet(
        &mut self,
        s: &mut HurtState<Self::Source>,
        damage: f32,
    ) -> Result<(), Self::Error>;
    /// `actuallyHurt` of `damage`, the hit's sequence in `container`.
    fn actually_hurt(
        &mut self,
        s: &mut HurtState<Self::Source>,
        container: &mut DamageContainer,
        damage: f32,
    ) -> Result<(), Self::Error>;
    fn resolve_mob(&mut self, s: &mut HurtState<Self::Source>) -> Result<(), Self::Error>;
    fn resolve_player(&mut self, s: &mut HurtState<Self::Source>) -> Result<(), Self::Error>;
    fn blocking_component(&mut self, item: Self::UseItem) -> Option<Self::Blocking>;
    fn on_blocked(
        &mut self,
        s: &mut HurtState<Self::Source>,
        block: Self::Blocking,
    ) -> Result<(), Self::Error>;
    fn broadcast_damage(&mut self, s: &mut HurtState<Self::Source>) -> Result<(), Self::Error>;
    fn mark_hurt(&mut self, s: &mut HurtState<Self::Source>) -> Result<(), Self::Error>;
    fn knockback(
        &mut self,
        s: &mut HurtState<Self::Source>,
        damage: f32,
        blocked: bool,
    ) -> Result<(), Self::Error>;
    fn death_protection(&mut self, s: &mut HurtState<Self::Source>) -> Result<bool, Self::Error>;
    fn death_sound(&mut self, s: &mut HurtState<Self::Source>) -> Result<Self::Sound, Self::Error>;
    fn make_sound(
        &mut self,
        s: &mut HurtState<Self::Source>,
        sound: Self::Sound,
    ) -> Result<(), Self::Error>;
    fn secondary_sound(&mut self, s: &mut HurtState<Self::Source>) -> Result<(), Self::Error>;
    fn die(&mut self, s: &mut HurtState<Self::Source>) -> Result<(), Self::Error>;
    fn hurt_sound(&mut self, s: &mut HurtState<Self::Source>) -> Result<(), Self::Error>;
    fn game_time(&mut self, s: &mut HurtState<Self::Source>) -> Result<i64, Self::Error>;
    fn effects_on_hurt(
        &mut self,
        s: &mut HurtState<Self::Source>,
        damage: f32,
    ) -> Result<(), Self::Error>;
    fn player_hurt_criterion(
        &mut self,
        s: &mut HurtState<Self::Source>,
        original: f32,
        damage: f32,
        blocked: bool,
    ) -> Result<(), Self::Error>;
    fn award_blocked(
        &mut self,
        s: &mut HurtState<Self::Source>,
        amount: i32,
    ) -> Result<(), Self::Error>;
    /// Looks up the attacker, then dispatches its criterion if a server player.
    fn attacker_criterion(
        &mut self,
        s: &mut HurtState<Self::Source>,
        original: f32,
        damage: f32,
        blocked: bool,
    ) -> Result<(), Self::Error>;
}
/// What `LivingDamageEvent.Pre` changed, back in the cooldown: NeoForge bubbles the damage
/// Pre left up to `lastHurt` (here vanilla's `lastHurt` plus what Pre added) and applies the
/// cooldown Pre set, which was `cooldown`. A hit Pre left alone writes nothing more, so a
/// write `actuallyHurt` made itself stands as in vanilla.
fn after_pre<S>(s: &mut HurtState<S>, container: &DamageContainer, cooldown: i32) {
    let change = container.pre_change();
    if change != 0.0 {
        s.last_hurt += change;
    }
    let ticks = container.post_attack_invulnerability_ticks();
    if ticks != cooldown {
        s.cooldown = ticks;
    }
}
pub fn hurt_server<W: HurtWorld>(
    s: &mut HurtState<W::Source>,
    w: &mut W,
    mut damage: f32,
) -> Result<bool, W::Error> {
    if w.invulnerable(s)? || w.dead_or_dying(s)? {
        return Ok(false);
    }
    if w.tag(s, HurtTag::Fire)? && w.fire_resistance(s)? {
        return Ok(false);
    }
    let mut container = DamageContainer::new(damage);
    if w.hurt_start(s, &mut container)? {
        return Ok(false);
    }
    damage = container.new_damage();
    if w.sleeping(s)? {
        w.stop_sleeping(s)?;
    }
    s.no_action_time = 0;
    if damage < 0.0 {
        damage = 0.0;
    }
    let original = damage;
    let item = w.use_item(s)?;
    let blocked_amount = w.apply_blocking(s, damage)?;
    damage -= blocked_amount;
    let blocked = blocked_amount > 0.0;
    container.record_block(blocked_amount);
    if w.tag(s, HurtTag::Freezing)? && w.freeze_hurts_extra(s)? {
        damage *= 5.0;
    }
    if w.tag(s, HurtTag::DamagesHelmet)? && !w.helmet_empty(s)? {
        w.hurt_helmet(s, damage)?;
        damage *= 0.75;
    }
    if !damage.is_finite() {
        damage = f32::MAX;
    }
    let fresh = !(s.cooldown as f32 > 10.0 && !w.tag(s, HurtTag::BypassesCooldown)?);
    container.record_should_cause_side_effects(fresh);
    if !fresh {
        if damage <= s.last_hurt {
            return Ok(false);
        }
        container.record_reduction(Reduction::Invulnerability, s.last_hurt);
        let cooldown = s.cooldown;
        container.set_post_attack_invulnerability_ticks(cooldown);
        w.actually_hurt(s, &mut container, damage - s.last_hurt)?;
        s.last_hurt = damage;
        after_pre(s, &container, cooldown);
    } else {
        // A fresh hit's cooldown: vanilla's 20, or what `LivingIncomingDamageEvent` set.
        let cooldown = container.post_attack_invulnerability_ticks();
        s.last_hurt = damage;
        s.cooldown = cooldown;
        w.actually_hurt(s, &mut container, damage)?;
        after_pre(s, &container, cooldown);
        s.hurt_duration = 10;
        s.hurt_time = s.hurt_duration;
    }
    w.resolve_mob(s)?;
    w.resolve_player(s)?;
    if fresh {
        let component = w.blocking_component(item);
        if let Some(component) = component.filter(|_| blocked) {
            w.on_blocked(s, component)?;
        } else {
            w.broadcast_damage(s)?;
        }
        if !w.tag(s, HurtTag::NoImpact)? && (!blocked || damage > 0.0) {
            w.mark_hurt(s)?;
        }
        let fully_blocked = blocked && damage <= 0.0;
        if !w.tag(s, HurtTag::NoKnockback)? && !fully_blocked {
            w.knockback(s, damage, blocked)?;
        }
    }
    if w.dead_or_dying(s)? {
        if !w.death_protection(s)? {
            if fresh {
                let sound = w.death_sound(s)?;
                w.make_sound(s, sound)?;
                w.secondary_sound(s)?;
            }
            w.die(s)?;
        }
    } else if fresh {
        w.hurt_sound(s)?;
        w.secondary_sound(s)?;
    }
    let accepted = !blocked || damage > 0.0;
    if accepted {
        s.last_source = Some(w.source());
        s.last_stamp = w.game_time(s)?;
        w.effects_on_hurt(s, damage)?;
    }
    if w.is_server_player() {
        w.player_hurt_criterion(s, original, damage, blocked)?;
        if blocked_amount > 0.0 && blocked_amount < STAT_LIMIT {
            w.award_blocked(s, stat_amount(blocked_amount))?;
        }
    }
    w.attacker_criterion(s, original, damage, blocked)?;
    Ok(accepted)
}
