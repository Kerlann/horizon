use super::*;

/// The hook points. Each does nothing in Horizon.
impl Game<'_> {
    /// A sound is about to play at `position` in `level` (NeoForge's
    /// `PlayLevelSoundEvent.AtPosition`, in `ServerLevel.playSeededSound`):
    /// every server sound not bound to an entity, mobs' own sounds included
    /// (`Entity.playSound` plays at the entity's position). A mod may change
    /// `sound`, `source`, `volume` and `pitch`, or set `cancel` and no player
    /// hears it; `original_volume` and `original_pitch` are as asked. A
    /// sound left unplayable (a volume or pitch that isn't finite and at
    /// least 0) is dropped. Sounds the server's level
    /// plays during the tick are asked as they go out, at the end of the
    /// tick, with the state then. `/playsound` sends its packet directly and
    /// isn't asked, as in vanilla. Server-side.
    pub(in crate::game) fn native_play_level_sound_at_position(
        &mut self,
        call: mod_api::__private::PlayLevelSoundAtPositionCall<'_>,
    ) {
        self.fire_play_level_sound_at_position(call);
    }
    /// A sound bound to `entity` is about to play (NeoForge's
    /// `PlayLevelSoundEvent.AtEntity`, in `ServerLevel.playSeededSound`'s
    /// entity overload, `playSound(except, sourceEntity, ...)`): vanilla's
    /// entity-bound sites (`Game::play_bound_sound`, `MobEvent::BoundSound`),
    /// the goat horn and a mod's sound bound to an entity. The fields and
    /// their rules are `native_play_level_sound_at_position`'s. Server-side.
    pub(in crate::game) fn native_play_level_sound_at_entity(
        &mut self,
        call: mod_api::__private::PlayLevelSoundAtEntityCall<'_>,
    ) {
        self.fire_play_level_sound_at_entity(call);
    }
}
