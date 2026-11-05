//! The sound engine's hook points for native mods. They fire from the engine itself,
//! where vanilla's `SoundEngine` does, with the client's [`LoopContext`](mod_api::LoopContext): no game is
//! at hand there, and a sound is played from inside the engine's own loops.
use super::{Attenuation, ChannelId, SoundEngine, SoundInstance};
use mod_api::__private::built;
use mod_api::{ChannelRef, HookId, OwningLoop, SoundAttenuation, SoundPlay, SoundSource};

/// Hook point for native mods (docs/modding/src/how-to/native-mods.md, declared in `hooks/`
/// with the game's): a sound is about to play (`PlaySoundEvent`, vanilla's `SoundEngine.play`).
/// `original` is the sound as asked for; `sound` starts as a copy of it, and a mod edits it or
/// sets it `None` to play nothing. It is asked before the engine's own checks (library loaded,
/// sound resolved, volume, channel pool), for every sound the client plays, so answer fast.
#[inline(never)]
pub fn native_play_sound(
    context: &horizon_world::native_load_context::LoopContext,
    original: &SoundPlay,
    sound: &mut Option<SoundPlay>,
) {
    mod_api::dispatch_play_sound(&OwningLoop::new(context), original, sound);
}

/// Hook point for native mods (docs/modding/src/how-to/native-mods.md, declared in `hooks/`
/// with the game's): a sound that is not streamed has started on `channel`
/// (`PlaySoundSourceEvent`), after the engine set its pitch, volume and position.
#[inline(never)]
pub fn native_play_sound_source(
    context: &horizon_world::native_load_context::LoopContext,
    sound: &SoundPlay,
    channel: &ChannelRef,
) {
    mod_api::dispatch_play_sound_source(&OwningLoop::new(context), sound, channel);
}

/// Hook point for native mods (docs/modding/src/how-to/native-mods.md, declared in `hooks/`
/// with the game's): a streamed sound (music, a record) has started on `channel`
/// (`PlayStreamingSourceEvent`).
#[inline(never)]
pub fn native_play_streaming_source(
    context: &horizon_world::native_load_context::LoopContext,
    sound: &SoundPlay,
    channel: &ChannelRef,
) {
    mod_api::dispatch_play_streaming_source(&OwningLoop::new(context), sound, channel);
}

/// Hook point for native mods (docs/modding/src/how-to/native-mods.md, declared in `hooks/`
/// with the game's): the sound engine opened its output, at window start, after a resource
/// reload and when the output device changes (`SoundEngineLoadEvent`).
#[inline(never)]
pub fn native_sound_engine_load(context: &horizon_world::native_load_context::LoopContext) {
    mod_api::dispatch_sound_engine_load(&OwningLoop::new(context));
}

impl SoundInstance {
    fn play_settings(&self) -> SoundPlay {
        built::SoundPlay {
            identifier: self.identifier.clone(),
            source: SoundSource::ALL[self.source],
            volume: self.volume,
            pitch: self.pitch,
            position: self.position,
            looping: self.looping,
            delay: self.delay,
            attenuation: match self.attenuation {
                Attenuation::None => SoundAttenuation::None,
                Attenuation::Linear => SoundAttenuation::Linear,
            },
            relative: self.relative,
        }
        .into()
    }

    /// A sound a mod asks for (`ClientHost::play_local_sound`), through the same mapping
    /// as a `play_sound` edit.
    pub fn from_play(sound: &SoundPlay, random: horizon_core::random::LegacyRandom) -> Self {
        let mut instance = Self::new(
            &sound.identifier,
            0,
            1.0,
            1.0,
            random,
            false,
            0,
            Attenuation::None,
            [0.0; 3],
            true,
        );
        instance.apply_play_settings(sound.clone());
        instance
    }

    /// Take a mod's edits of [`Self::play_settings`]. A new identifier is resolved again
    /// at the play, as every instance is, even one whose sound a caller chose.
    fn apply_play_settings(&mut self, edited: SoundPlay) {
        if edited.identifier != self.identifier {
            self.identifier = edited.identifier;
            self.chosen = false;
        }
        self.source = edited.source as usize;
        self.volume = edited.volume;
        self.pitch = edited.pitch;
        self.position = edited.position;
        self.looping = edited.looping;
        self.delay = edited.delay;
        self.attenuation = match edited.attenuation {
            SoundAttenuation::None => Attenuation::None,
            SoundAttenuation::Linear => Attenuation::Linear,
            _ => self.attenuation,
        };
        self.relative = edited.relative;
    }
}

impl SoundEngine {
    /// The loop context of this client's current resource generation.
    pub fn loop_context(&self) -> Option<&horizon_world::native_load_context::LoopContext> {
        self.loop_context.as_ref()
    }

    /// Replace the loop context when a resource generation starts or reloads.
    pub fn set_loop_context(&mut self, context: horizon_world::native_load_context::LoopContext) {
        self.loop_context = Some(context);
    }

    /// `play_sound`: false when a mod took the sound away. With no listener, or on a
    /// side with no context (a server has no sound), it costs this one check.
    pub(super) fn play_sound_allowed(&mut self, id: super::InstanceId) -> bool {
        let Some(source) = &self.loop_context else {
            return true;
        };
        if !mod_api::__private::listened(HookId::PlaySound) {
            return true;
        }
        let Some(instance) = self.instances.get_mut(&id) else {
            return true;
        };
        let original = instance.play_settings();
        let mut sound = Some(original.clone());
        native_play_sound(source, &original, &mut sound);
        match sound {
            None => false,
            Some(edited) => {
                if edited != original {
                    instance.apply_play_settings(edited);
                }
                true
            }
        }
    }

    /// `play_sound_source` or `play_streaming_source`, for a sound that just started.
    pub(super) fn notify_source_started(
        &self,
        id: super::InstanceId,
        channel: ChannelId,
        streaming: bool,
    ) {
        let Some(source) = &self.loop_context else {
            return;
        };
        let hook = if streaming {
            HookId::PlayStreamingSource
        } else {
            HookId::PlaySoundSource
        };
        if !mod_api::__private::listened(hook) {
            return;
        }
        let Some(instance) = self.instances.get(&id) else {
            return;
        };
        let sound = instance.play_settings();
        let channel = built::ChannelRef { id: channel }.into();
        if streaming {
            native_play_streaming_source(source, &sound, &channel);
        } else {
            native_play_sound_source(source, &sound, &channel);
        }
    }

    /// `sound_engine_load`, once the output is open and the context is current.
    pub fn notify_loaded(&self) {
        let Some(source) = &self.loop_context else {
            return;
        };
        if self.loaded && mod_api::__private::listened(HookId::SoundEngineLoad) {
            native_sound_engine_load(source);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sound_engine::test_backend::Null;
    use crate::sound_events::SoundRegistry;
    use horizon_core::random::LegacyRandom;
    use horizon_world::native_load_context::{DataLoadOrigin, LoadContext, LoadInputs, LoopSide};
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex};

    /// What the mod heard, in order.
    type Log = Arc<Mutex<Vec<String>>>;

    /// Mutes `minecraft:block.mute`, turns `minecraft:block.quiet` up and a half-tone down.
    struct Tuner(Log);

    #[mod_api::__subscriptions]
    impl mod_api::Hooks for Tuner {
        fn play_sound(
            &self,
            _context: &dyn mod_api::LoopContext,
            event: &mut mod_api::PlaySoundEvent<'_>,
        ) {
            self.0.lock().unwrap().push(format!(
                "play {} {}",
                event.original.identifier, event.original.volume
            ));
            match event.original.identifier.as_str() {
                "minecraft:block.mute" => *event.sound = None,
                "minecraft:block.quiet" => {
                    if let Some(sound) = event.sound.as_mut() {
                        sound.volume = 0.5;
                        sound.pitch = 2.0;
                    }
                }
                _ => {}
            }
        }
        fn play_sound_source(
            &self,
            _context: &dyn mod_api::LoopContext,
            event: &mut mod_api::PlaySoundSourceEvent<'_>,
        ) {
            self.0.lock().unwrap().push(format!(
                "source {} {}",
                event.sound.identifier, event.sound.volume
            ));
        }
        fn play_streaming_source(
            &self,
            _context: &dyn mod_api::LoopContext,
            event: &mut mod_api::PlayStreamingSourceEvent<'_>,
        ) {
            self.0
                .lock()
                .unwrap()
                .push(format!("stream {}", event.sound.identifier));
        }
        fn sound_engine_load(
            &self,
            context: &dyn mod_api::LoopContext,
            _event: &mut mod_api::SoundEngineLoadEvent,
        ) {
            self.0
                .lock()
                .unwrap()
                .push(format!("load {:?}", context.side()));
        }
    }

    fn engine() -> SoundEngine {
        let mut registry = SoundRegistry::default();
        registry.add_file(
            "minecraft",
            br#"{"block.mute": {"sounds": ["a"]}, "block.quiet": {"sounds": ["a"]},
                 "block.plain": {"sounds": ["a"]}, "music.menu": {"sounds": [{"name": "m", "stream": true}]}}"#,
            &|_, _| true,
        );
        let mut engine = SoundEngine::new(registry);
        let inputs = Arc::new(LoadInputs::new(BTreeMap::new(), std::env::temp_dir()));
        let context = LoadContext::new(DataLoadOrigin::ClientPreload, inputs, []);
        engine.set_loop_context(context.loop_context(LoopSide::Client, []));
        engine
    }

    fn play(
        engine: &mut SoundEngine,
        backend: &mut Null,
        name: &str,
    ) -> crate::sound_engine::PlayResult {
        let instance = SoundInstance::for_ui(name, 1.0, 1.0, LegacyRandom::new(1));
        engine.play_new(instance, false, backend).1
    }

    #[test]
    fn a_mod_mutes_retunes_and_hears_the_sources_start() {
        let log: Log = Log::default();
        let mods: Vec<mod_api::ModEntry> = vec![mod_api::ModEntry::new(
            "tuner",
            Box::new(Tuner(log.clone())),
        )];
        mod_api::testing::with_mods(mods.into(), || {
            let (mut engine, mut backend) = (engine(), Null::default());
            engine.notify_loaded();
            assert_eq!(
                play(&mut engine, &mut backend, "minecraft:block.mute"),
                crate::sound_engine::PlayResult::NotStarted
            );
            assert!(backend.volume.is_empty(), "a muted sound takes no channel");
            play(&mut engine, &mut backend, "minecraft:block.quiet");
            assert_eq!(backend.volume.values().copied().collect::<Vec<_>>(), [0.5]);
            play(&mut engine, &mut backend, "minecraft:block.plain");
            play(&mut engine, &mut backend, "minecraft:music.menu");
        });
        assert_eq!(
            *log.lock().unwrap(),
            [
                "load Client",
                "play minecraft:block.mute 1",
                "play minecraft:block.quiet 1",
                "source minecraft:block.quiet 0.5",
                "play minecraft:block.plain 1",
                "source minecraft:block.plain 1",
                "play minecraft:music.menu 1",
                "stream minecraft:music.menu",
            ]
        );
    }

    #[test]
    fn with_no_listener_a_sound_plays_as_asked() {
        let (mut engine, mut backend) = (engine(), Null::default());
        engine.notify_loaded();
        assert_eq!(
            play(&mut engine, &mut backend, "minecraft:block.mute"),
            crate::sound_engine::PlayResult::Started
        );
    }
}
