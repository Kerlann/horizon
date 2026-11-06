//! Options > Mods > Browse mods (docs/design/mod-hotload.md): reading the
//! registry's index and installing a content mod run on a worker
//! (`horizon_client::mod_registry`); each frame takes what it finished into the
//! options screen's `mod_browser`. The window's game and the startup menu each
//! have one.
use super::Game;
use horizon_client::{
    lang,
    mod_browser::{Listing, ModBrowser, Status},
    mod_registry::{self, Done},
    settings::Settings,
};
use horizon_mods::index::Index;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    mpsc::{Receiver, TryRecvError, channel},
};

/// What the worker sends back: the index and its listings, or an install's
/// outcome with the listings after it.
enum Finished {
    Index(Result<(Arc<Index>, Vec<Listing>), String>),
    Installed(Result<Done, String>, Vec<Listing>),
}

/// One job at a time, and the index read last.
#[derive(Default)]
pub(super) struct Jobs {
    running: Option<Receiver<Finished>>,
    pending: Option<Finished>,
    index: Option<Arc<Index>>,
}

impl Jobs {
    /// Browse mods opened: the index, read again from the registry (the
    /// launcher's copy when it can't be reached).
    pub(super) fn browse(&mut self, ui: &mut ModBrowser, home: PathBuf) {
        if self.running.is_some() || self.pending.is_some() {
            return;
        }
        if ui.status != Status::Ready {
            ui.status = Status::Loading;
        }
        let (send, receive) = channel();
        self.running = Some(receive);
        spawn("mod-browser-index", move || {
            let read = mod_registry::load_index(&home, &mod_registry::location()).map(|index| {
                let listings = mod_registry::listings(&index, &home);
                (Arc::new(index), listings)
            });
            let _ = send.send(Finished::Index(read));
        });
    }

    /// Install or update `id` from the index read last.
    pub(super) fn install(&mut self, ui: &mut ModBrowser, home: PathBuf, id: String) {
        let Some(index) = self.index.clone() else {
            return;
        };
        if self.running.is_some() || self.pending.is_some() {
            return;
        }
        ui.installing = Some(id.clone());
        ui.message = None;
        let (send, receive) = channel();
        self.running = Some(receive);
        spawn("mod-browser-install", move || {
            let done = mod_registry::install(&index, &home, &id, &mod_registry::download);
            let listings = mod_registry::listings(&index, &home);
            let _ = send.send(Finished::Installed(done, listings));
        });
    }

    /// What finished since the last frame, into `ui`. Activate and save against
    /// fresh settings in the installer's transaction; callers list mods again
    /// when this returns true. A world keeps its frozen registry set, and a busy
    /// launcher leaves the completion queued rather than blocking the window.
    pub(super) fn poll(
        &mut self,
        ui: &mut ModBrowser,
        settings: &mut Settings,
        options: &Path,
        in_world: bool,
    ) -> bool {
        if in_world {
            return false;
        }
        let finished = if let Some(finished) = self.pending.take() {
            finished
        } else {
            let Some(running) = &self.running else {
                return false;
            };
            match running.try_recv() {
                Ok(finished) => finished,
                Err(TryRecvError::Empty) => return false,
                Err(TryRecvError::Disconnected) => {
                    self.running = None;
                    ui.installing = None;
                    if ui.status == Status::Loading {
                        ui.status = Status::Failed("the worker stopped".into());
                    }
                    return false;
                }
            }
        };
        self.running = None;
        match finished {
            Finished::Index(Ok((index, listings))) => {
                self.index = Some(index);
                ui.listings = listings;
                ui.status = Status::Ready;
                false
            }
            Finished::Index(Err(why)) => {
                log::warn!("Horizon: the mod registry wasn't read: {why}");
                if ui.status != Status::Ready {
                    ui.status = Status::Failed(why);
                }
                false
            }
            Finished::Installed(done, listings) => {
                let home = horizon_client::logging::shared_dir(
                    options.parent().unwrap_or_else(|| Path::new(".")),
                );
                let Some(_lock) = horizon_mods::install_lock::try_lock_home(&home) else {
                    self.pending = Some(Finished::Installed(done, listings));
                    return false;
                };
                ui.installing = None;
                ui.listings = listings;
                let done = done.and_then(|done| {
                    // The launcher may have removed or replaced this package
                    // since the worker finished. Never activate a stale result.
                    mod_registry::check_current_install(&home, &done)?;
                    settings
                        .save_with_mods(options, |mods| {
                            mods.enabled.insert(done.id.clone());
                            mods.disabled.remove(&done.id);
                        })
                        .map_err(|e| e.to_string())?;
                    Ok(done)
                });
                match done {
                    Ok(done) => {
                        log::info!(
                            "Horizon: installed the mod {} {} from the registry ({})",
                            done.id,
                            done.version,
                            if done.verified { "signed" } else { "unsigned" }
                        );
                        let name = ui
                            .listing(&done.id)
                            .map_or(done.id.clone(), |l| l.name.clone());
                        let text = lang::tr("horizon.mods.browse.done", &[&name, &done.version]);
                        ui.message = Some((text, true));
                        true
                    }
                    Err(why) => {
                        log::warn!("Horizon: a registry mod wasn't installed: {why}");
                        let text = lang::tr("horizon.mods.browse.install_failed", &[&why]);
                        ui.message = Some((text, false));
                        false
                    }
                }
            }
        }
    }
}

fn spawn(name: &str, work: impl FnOnce() + Send + 'static) {
    if let Err(e) = std::thread::Builder::new().name(name.into()).spawn(work) {
        log::warn!("Horizon: {name} did not start: {e}");
    }
}

impl Game<'_> {
    /// Each frame: what the browser's worker finished.
    pub(super) fn drive_mod_browser(&mut self) {
        let installed = self.mod_jobs.poll(
            &mut self.ui.options.mod_browser,
            &mut self.settings,
            &Self::options_path(),
            self.player.is_some(),
        );
        if installed {
            self.start_mods();
            self.switch_mods();
            // A mod with blocks, items or worldgen waits for a restart: the
            // project page says so instead of "installed and on".
            if let (Some(note), Some((message, _))) = (
                &self.ui.options.mods_note,
                &mut self.ui.options.mod_browser.message,
            ) {
                *message = format!("{message} {note}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn completed_install(name: &str) -> (PathBuf, Settings, Jobs, ModBrowser) {
        let home = std::env::temp_dir().join(format!(
            "horizon-browser-completion-{name}-{}",
            std::process::id()
        ));
        fs::create_dir_all(home.join("horizon-mods")).unwrap();
        let package = horizon_mods::Package::write_files(
            "id = \"ruby\"\nname = \"Ruby\"\nversion = \"1.0\"\napi = \"0.2\"\nside = \"both\"\ncapabilities = [\"registries\"]\n",
            &[],
        ).unwrap();
        fs::write(home.join("horizon-mods/ruby.hmod"), package).unwrap();
        fs::write(home.join(mod_registry::MANIFEST), r#"{"entries":[{"source":"horizon","project":"ruby","version":"1.0","file":"horizon-mods/ruby.hmod"},{"source":"horizon","project":"zoom","version":"1","file":"horizon-native-mods/zoom.hmod","pin":"exact-build"}]}"#).unwrap();
        fs::write(home.join(horizon_mods::mod_list::FILE), "mod.ruby:false\n").unwrap();
        let settings = Settings::load(&home.join("options.txt"));
        let (send, receive) = channel();
        send.send(Finished::Installed(
            Ok(Done {
                id: "ruby".into(),
                version: "1.0".into(),
                verified: false,
            }),
            Vec::new(),
        ))
        .unwrap();
        (
            home,
            settings,
            Jobs {
                running: Some(receive),
                ..Jobs::default()
            },
            ModBrowser::default(),
        )
    }

    #[test]
    fn an_install_finishing_after_world_open_waits_to_enable_its_content() {
        let (home, mut settings, mut jobs, mut ui) = completed_install("world");
        let options = home.join("options.txt");
        assert!(!jobs.poll(&mut ui, &mut settings, &options, true));
        assert!(settings.mods.enabled.is_empty());
        assert!(settings.mods.disabled.contains("ruby"));
        assert!(jobs.running.is_some());
        let pins = fs::read(home.join(mod_registry::MANIFEST)).unwrap();
        // The launcher writes while completion is deferred by the open world.
        fs::write(
            home.join(horizon_mods::mod_list::FILE),
            "mod.ruby:false\nmod.launcher:true\nkey.launcher.open:key.keyboard.x\n",
        )
        .unwrap();
        assert!(jobs.poll(&mut ui, &mut settings, &options, false));
        assert!(settings.mods.enabled.contains("ruby"));
        assert!(settings.mods.enabled.contains("launcher"));
        assert_eq!(settings.mods.keys["launcher"]["open"], "key.keyboard.x");
        assert!(!settings.mods.disabled.contains("ruby"));
        assert!(jobs.running.is_none());
        assert_eq!(fs::read(home.join(mod_registry::MANIFEST)).unwrap(), pins);
        assert_eq!(
            horizon_mods::mod_list::ModList::load(&home).enabled,
            settings.mods.enabled
        );
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn a_busy_launcher_keeps_the_completion_queued() {
        let (home, mut settings, mut jobs, mut ui) = completed_install("busy");
        std::thread::scope(|scope| {
            let (ready, wait) = channel();
            let (release, released) = channel();
            let target = &home;
            let worker = scope.spawn(move || {
                let _guard = horizon_mods::install_lock::lock_home(target).unwrap();
                ready.send(()).unwrap();
                released.recv().unwrap();
                fs::write(
                    target.join(horizon_mods::mod_list::FILE),
                    "mod.launcher:true\n",
                )
                .unwrap();
            });
            wait.recv().unwrap();
            assert!(!jobs.poll(&mut ui, &mut settings, &home.join("options.txt"), false));
            assert!(jobs.pending.is_some());
            release.send(()).unwrap();
            worker.join().unwrap();
        });
        assert!(jobs.poll(&mut ui, &mut settings, &home.join("options.txt"), false));
        assert!(settings.mods.enabled.contains("launcher"));
        assert!(settings.mods.enabled.contains("ruby"));
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn a_stale_completion_does_not_enable_a_replacement_native_pin() {
        let (home, mut settings, mut jobs, mut ui) = completed_install("replaced");
        let pins = r#"{"entries":[{"source":"horizon","project":"ruby","version":"1.0","file":"horizon-native-mods/ruby.hmod","pin":"exact-build"}]}"#;
        fs::write(home.join(mod_registry::MANIFEST), pins).unwrap();
        assert!(!jobs.poll(&mut ui, &mut settings, &home.join("options.txt"), false));
        assert_eq!(
            fs::read_to_string(home.join(horizon_mods::mod_list::FILE)).unwrap(),
            "mod.ruby:false\n"
        );
        assert_eq!(
            fs::read_to_string(home.join(mod_registry::MANIFEST)).unwrap(),
            pins
        );
        assert!(matches!(ui.message, Some((_, false))));
        fs::remove_dir_all(home).unwrap();
    }
}
