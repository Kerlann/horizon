//! Which of the installed Horizon mods are on (`docs/modding/src/how-to/content-mods.md`), kept in
//! `horizon-mods.txt` beside `options.txt` as `mod.<id>:true` lines. Every
//! mod is off until the player turns it on in Options > Mods. A native mod is
//! built in, so on until turned off (`mod.<id>:false`); the launcher builds
//! only the native mods whose line is `true`.
use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    path::Path,
};

pub const FILE: &str = "horizon-mods.txt";
/// The folder holding `.hmod` files, beside `options.txt`.
pub const DIR: &str = "horizon-mods";
/// Where the launcher installs native mods (`<id>.hmod`), beside [`DIR`]. They run only in a
/// game the launcher built with them.
pub const NATIVE_DIR: &str = "horizon-native-mods";
/// Why a native mod installed in [`NATIVE_DIR`] doesn't run in this game.
pub const NOT_BUILT: &str = "Installed, not built into this game: build the game with it in the launcher (Settings > Native mods), then play the instance that build is in.";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModList {
    /// Ids of the mods the player turned on.
    pub enabled: BTreeSet<String>,
    /// Ids of the native mods the player turned off: they rest
    /// (`native_on`) until the launcher's next build leaves them out.
    pub disabled: BTreeSet<String>,
    /// The keys the player chose for mods' keys, mod id → key name → key
    /// (`key.<id>.<name>:key.keyboard.x` lines).
    pub keys: BTreeMap<String, BTreeMap<String, String>>,
    /// What the Mods page shows, filled by the game from the mods it found.
    pub installed: Vec<Installed>,
}

/// One installed mod as the Mods page shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Installed {
    pub id: String,
    /// `Name 1.0` (the file name when the manifest is unreadable).
    pub title: String,
    pub description: String,
    /// Why it is not running, when it failed.
    pub problem: Option<String>,
    /// Loaded with `--dev-mod`: always on.
    pub dev: bool,
    /// A native mod (`game/native_hooks.rs`): built into the game, on until
    /// the player turns it off, and its settings page.
    pub native: Option<NativePage>,
}

/// A native mod's settings page: its keys as Key Binds rows, then its
/// buttons. The keys the player chooses are kept in `ModList::keys`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NativePage {
    /// Each key's name, label and default key (`key.keyboard.z`).
    pub keys: Vec<[String; 3]>,
    pub buttons: Vec<String>,
}

impl ModList {
    pub fn load(dir: &Path) -> Self {
        let _lock = match crate::install_lock::lock_home(dir) {
            Ok(lock) => lock,
            Err(e) => {
                log::warn!("Cannot read mod switches in {}: {e}", dir.display());
                return Self::default();
            }
        };
        fs::read_to_string(dir.join(FILE))
            .map(|t| Self::parse(&t))
            .unwrap_or_default()
    }
    pub fn parse(text: &str) -> Self {
        let ids = |value: &str| -> BTreeSet<String> {
            text.lines()
                .filter_map(|l| l.split_once(':'))
                .filter(|(_, v)| v.trim() == value)
                .filter_map(|(k, _)| k.trim().strip_prefix("mod."))
                .map(str::to_owned)
                .collect()
        };
        let (enabled, disabled) = (ids("true"), ids("false"));
        let mut keys: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
        for (k, v) in text.lines().filter_map(|l| l.split_once(':')) {
            // Mod ids have no dots; a key's name may.
            if let Some((id, name)) = k
                .trim()
                .strip_prefix("key.")
                .and_then(|r| r.split_once('.'))
            {
                let entry = keys.entry(id.to_owned()).or_default();
                entry.insert(name.to_owned(), v.trim().to_owned());
            }
        }
        Self {
            enabled,
            disabled,
            keys,
            installed: Vec::new(),
        }
    }
    pub fn serialize(&self) -> String {
        let on = self.enabled.iter().map(|id| format!("mod.{id}:true\n"));
        let off = self.disabled.iter().map(|id| format!("mod.{id}:false\n"));
        let keys = self.keys.iter().flat_map(|(id, keys)| {
            keys.iter()
                .map(move |(name, key)| format!("key.{id}.{name}:{key}\n"))
        });
        on.chain(off).chain(keys).collect()
    }
    /// Mod `id`'s chosen keys, for `Mods::set_bindings`.
    pub fn keys_of(&self, id: &str) -> Vec<(&str, &str)> {
        self.keys.get(id).map_or_else(Vec::new, |k| {
            k.iter().map(|(n, k)| (n.as_str(), k.as_str())).collect()
        })
    }
    /// Merge only this player's changes since `previous` into a fresh on-disk
    /// list, then apply an explicit action (for example, an install completion).
    /// Read, merge, edit and publish use the installer's one home lock. Unchanged
    /// switches/keys and unknown lines belong to the latest writer, not a stale
    /// settings snapshot. On success both the live list and its baseline advance.
    pub fn save(
        &mut self,
        dir: &Path,
        previous: &mut String,
        edit: impl FnOnce(&mut Self),
    ) -> io::Result<()> {
        let _lock = crate::install_lock::lock_home(dir)?;
        let path = dir.join(FILE);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(e),
        };
        let merged_text = apply_changes(&text, previous, &self.serialize());
        let mut merged = Self::parse(&merged_text);
        let before_edit = merged.serialize();
        edit(&mut merged);
        let output = apply_changes(&merged_text, &before_edit, &merged.serialize());
        if output != text {
            fs::create_dir_all(dir)?;
            let part = dir.join(format!(".{FILE}.part"));
            fs::write(&part, output)?;
            fs::rename(&part, path)?;
        }
        merged.installed = std::mem::take(&mut self.installed);
        *self = merged;
        *previous = self.serialize();
        Ok(())
    }
    pub fn is_on(&self, i: usize) -> bool {
        self.installed.get(i).is_some_and(|m| {
            m.dev
                || if m.native.is_some() {
                    !self.disabled.contains(&m.id)
                } else {
                    self.enabled.contains(&m.id)
                }
        })
    }
    /// Turn the installed mod `id` on (nothing when it isn't installed).
    pub fn turn_on(&mut self, id: &str) {
        let Some(m) = self.installed.iter().find(|m| m.id == id && !m.dev) else {
            return;
        };
        self.disabled.remove(&m.id);
        self.enabled.insert(m.id.clone());
    }
    /// Turns installed mod `i` on or off (a dev mod stays on). A native mod's
    /// line says `false` when off, so that the launcher's next build leaves
    /// it out, and `true` when on again.
    pub fn toggle(&mut self, i: usize) {
        let Some(m) = self.installed.get(i).filter(|m| !m.dev) else {
            return;
        };
        if m.native.is_some() {
            if self.disabled.remove(&m.id) {
                self.enabled.insert(m.id.clone());
            } else {
                self.enabled.remove(&m.id);
                self.disabled.insert(m.id.clone());
            }
        } else if !self.enabled.remove(&m.id) {
            // A wasm mod paused in the previous process is not loaded yet; selecting its
            // package again must clear the same code-dispatch switch used while it ran.
            self.disabled.remove(&m.id);
            self.enabled.insert(m.id.clone());
        }
    }
}

/// Apply a delta of supported settings without rewriting other writers' lines.
fn apply_changes(text: &str, before: &str, after: &str) -> String {
    let before: BTreeMap<_, _> = before.lines().filter_map(|l| l.split_once(':')).collect();
    let after: BTreeMap<_, _> = after.lines().filter_map(|l| l.split_once(':')).collect();
    let changed: BTreeSet<_> = before
        .keys()
        .chain(after.keys())
        .copied()
        .filter(|key| before.get(key) != after.get(key))
        .collect();
    if changed.is_empty() {
        return text.to_owned();
    }
    let mut lines: Vec<_> = text
        .lines()
        .filter(|line| {
            line.split_once(':')
                .is_none_or(|(key, _)| !changed.contains(key.trim()))
        })
        .map(str::to_owned)
        .collect();
    lines.extend(
        changed
            .iter()
            .filter_map(|key| after.get(key).map(|value| format!("{key}:{value}"))),
    );
    let mut output = lines.join("\n");
    if !output.is_empty() {
        output.push('\n');
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_ignores_the_rest() {
        let list = ModList::parse("mod.coords_hud:true\nmod.other:false\nzoom:true\n");
        assert_eq!(list.enabled.iter().collect::<Vec<_>>(), ["coords_hud"]);
        assert_eq!(ModList::parse(&list.serialize()), list);
        assert_eq!(ModList::default().serialize(), "");
    }

    #[test]
    fn keeps_the_keys_players_chose() {
        let text = "mod.zoom:true\nkey.zoom.zoom.in:key.mouse.4\nkey.zoom.out:key.keyboard.x\n";
        let list = ModList::parse(text);
        assert_eq!(
            list.keys_of("zoom"),
            [("out", "key.keyboard.x"), ("zoom.in", "key.mouse.4")]
        );
        assert!(list.keys_of("other").is_empty());
        assert_eq!(ModList::parse(&list.serialize()), list);
    }

    #[test]
    fn stale_saves_merge_only_local_switch_and_key_edits() {
        let dir = std::env::temp_dir().join(format!("horizon-mod-delta-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let mut previous = "mod.zoom:true\nmod.ruby:false\nkey.zoom.in:key.keyboard.z\n".to_owned();
        let mut list = ModList::parse(&previous);
        // A launcher write happened after the game loaded its snapshot.
        let latest = "mod.zoom:false\nmod.ruby:false\nmod.launcher:true\nkey.zoom.in:key.keyboard.x\nfuture.setting:keep\n";
        fs::write(dir.join(FILE), latest).unwrap();
        list.enabled.insert("ruby".into());
        list.disabled.remove("ruby");
        list.keys
            .entry("ruby".into())
            .or_default()
            .insert("open".into(), "key.keyboard.r".into());
        list.save(&dir, &mut previous, |_| {}).unwrap();
        assert!(!list.enabled.contains("zoom"));
        assert!(list.disabled.contains("zoom"));
        assert!(list.enabled.contains("launcher"));
        assert!(list.enabled.contains("ruby"));
        assert_eq!(list.keys["zoom"]["in"], "key.keyboard.x");
        assert_eq!(list.keys["ruby"]["open"], "key.keyboard.r");
        assert!(
            fs::read_to_string(dir.join(FILE))
                .unwrap()
                .contains("future.setting:keep\n")
        );
        // An unrelated save must not replay the old player's changes either.
        fs::write(dir.join(FILE), "mod.ruby:false\nmod.zoom:true\n").unwrap();
        list.save(&dir, &mut previous, |_| {}).unwrap();
        assert!(list.disabled.contains("ruby"));
        assert!(list.enabled.contains("zoom"));
        assert!(list.keys.is_empty());
        // An explicit completion is an action on the fresh list, even when its
        // requested value matched the player's earlier snapshot.
        fs::write(dir.join(FILE), "mod.zoom:false\nmod.launcher:true\n").unwrap();
        list.save(&dir, &mut previous, |fresh| {
            assert!(fresh.disabled.contains("zoom"));
            fresh.disabled.remove("zoom");
            fresh.enabled.insert("zoom".into());
        })
        .unwrap();
        assert!(list.enabled.contains("zoom") && list.enabled.contains("launcher"));
        assert!(!list.disabled.contains("zoom"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_failed_save_keeps_the_local_delta_for_retry() {
        let dir =
            std::env::temp_dir().join(format!("horizon-mod-delta-error-{}", std::process::id()));
        fs::create_dir_all(dir.join(FILE)).unwrap();
        let mut list = ModList::parse("mod.ruby:true\n");
        let mut previous = String::new();
        assert!(
            list.save(&dir, &mut previous, |_| panic!(
                "unreadable settings refuse the action"
            ))
            .is_err()
        );
        assert!(previous.is_empty());
        assert!(list.enabled.contains("ruby"));
        fs::remove_dir(dir.join(FILE)).unwrap();
        list.save(&dir, &mut previous, |_| {}).unwrap();
        assert_eq!(
            fs::read_to_string(dir.join(FILE)).unwrap(),
            "mod.ruby:true\n"
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn selecting_a_paused_package_clears_its_previous_code_switch() {
        let mut list = ModList::parse("mod.wasm_mod:false\n");
        list.installed.push(Installed {
            id: "wasm_mod".into(),
            title: "wasm_mod".into(),
            description: String::new(),
            problem: None,
            dev: false,
            native: None,
        });
        list.toggle(0);
        assert!(list.enabled.contains("wasm_mod"));
        assert!(!list.disabled.contains("wasm_mod"));
        list.enabled.clear();
        list.disabled.insert("wasm_mod".into());
        list.turn_on("wasm_mod");
        assert!(list.enabled.contains("wasm_mod"));
        assert!(!list.disabled.contains("wasm_mod"));
    }

    #[test]
    fn toggles_installed_and_native_mods_but_not_dev_ones() {
        let installed = |id: &str, dev| Installed {
            id: id.into(),
            title: id.into(),
            description: String::new(),
            problem: None,
            dev,
            native: None,
        };
        let mut list = ModList {
            installed: vec![installed("a", false), installed("b", true)],
            ..Default::default()
        };
        assert!(!list.is_on(0) && list.is_on(1));
        list.toggle(0);
        list.toggle(1);
        assert!(list.is_on(0) && list.is_on(1));
        assert_eq!(list.serialize(), "mod.a:true\n");
        list.toggle(0);
        assert!(!list.is_on(0));
        list.installed.push(Installed {
            native: Some(NativePage::default()),
            ..installed("c", false)
        });
        // On until turned off; off, the launcher leaves it out.
        assert!(list.is_on(2));
        list.toggle(2);
        assert!(!list.is_on(2));
        assert_eq!(list.serialize(), "mod.c:false\n");
        assert_eq!(ModList::parse(&list.serialize()).disabled, list.disabled);
        list.toggle(2);
        assert!(list.is_on(2));
        assert_eq!(list.serialize(), "mod.c:true\n");
    }
}
