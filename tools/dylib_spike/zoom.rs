// Copyright (c) 2026 Ludovic Marie and the Horizon contributors
// MIT; see tools/dylib_spike/ZOOM-LICENSE (adapted hook glue).
//! Thin trait adapter for the official MIT Zoom runtime; runtime.rs is unchanged.
use horizon_client::mod_api::*;
mod runtime;

struct Zoom;
impl Hooks for Zoom {
    fn key(&self, host: &mut dyn Host, event: &mut KeyEvent<'_>) {
        if !host.on(runtime::ID)
            || event.key != host.mod_key(runtime::ID, runtime::KEY, runtime::DEFAULT_KEY)
        {
            return;
        }
        if std::env::var_os("DYLIB_SPIKE_PANIC").is_some() {
            panic!("zoom spike panic");
        }
        // The current trait Host has no native_playing equivalent. This spike adapter
        // tests the in-world key path only; it is not a complete parity port of Zoom.
        runtime::key(true, event.pressed, |key| host.setting(runtime::ID, key));
        eprintln!("DYLIB_SPIKE zoom key: {:?}", runtime::view());
    }
    fn wheel(&self, host: &mut dyn Host, event: &mut WheelEvent) -> Flow {
        if host.on(runtime::ID) && runtime::wheel(event.steps) {
            Flow::Cancel
        } else {
            Flow::Continue
        }
    }
    fn zoom(&self, host: &dyn Host, _: &mut ZoomEvent) -> Option<NativeZoom> {
        if !host.on(runtime::ID) {
            return None;
        }
        let (factor, smooth, turn) = runtime::view()?;
        Some(NativeZoom {
            factor,
            smooth,
            turn,
        })
    }
    fn mods(&self, _: &mut dyn Host, event: &mut ModsEvent<'_>) {
        event.mods.push(NativeMod {
            id: runtime::ID.into(),
            title: format!("Zoom {}", runtime::VERSION),
            description: runtime::DESCRIPTION.into(),
            keys: vec![[
                runtime::KEY.into(),
                "Zoom key".into(),
                runtime::DEFAULT_KEY.into(),
            ]],
        });
    }
    fn settings(&self, host: &mut dyn Host, event: &mut SettingsEvent<'_>) {
        if event.mod_id == runtime::ID {
            event
                .buttons
                .extend(runtime::settings(|key| host.setting(runtime::ID, key)).labels());
        }
    }
    fn settings_click(&self, host: &mut dyn Host, event: &mut SettingsClickEvent<'_>) {
        if event.mod_id == runtime::ID {
            let mut settings = runtime::settings(|key| host.setting(runtime::ID, key));
            settings.click(event.button);
            for (key, value) in settings.stored() {
                host.set_setting(runtime::ID, key, &value);
            }
            runtime::set_settings(settings);
        }
    }
}

#[unsafe(no_mangle)]
pub fn horizon_mod_entry() -> Box<dyn Hooks> {
    Box::new(Zoom)
}
