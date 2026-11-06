//! Which of the installed Horizon mods are on (`docs/modding/src/how-to/content-mods.md`), kept in
//! `horizon-mods.txt` beside `options.txt` as `mod.<id>:true` lines. Every
//! mod is off until the player turns it on in Options > Mods. A native mod is
//! built in, so on until turned off (`mod.<id>:false`); the launcher builds
//! only the native mods whose line is `true`.
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
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
    /// The keys the mod registers on Options > Controls, under its title.
    pub mappings: Vec<KeyMapping>,
    pub buttons: Vec<String>,
}

/// A key a native mod registers on Options > Controls, under the mod's title.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct KeyMapping {
    /// Its id within the mod (`zoom`); its saved binding is `key.<mod id>.<id>`.
    pub id: String,
    /// The label's translation key.
    pub label: String,
    /// The default key (`key.keyboard.z`).
    pub key: String,
    /// The default modifiers: [`SHIFT`], [`CONTROL`] and [`ALT`] bits.
    pub modifiers: u32,
    pub context: KeyConflictContext,
    /// The category's translation key; empty for none.
    pub category: String,
}

/// The screens in which two mappings can conflict.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
// exhaustive: the built-in conflict contexts
pub enum KeyConflictContext {
    #[default]
    Universal,
    InGame,
    Gui,
}
impl KeyConflictContext {
    pub fn conflicts(self, other: Self) -> bool {
        self == Self::Universal || other == Self::Universal || self == other
    }
}
/// Compare bindings, excluding unbound keys and pairs that are both at default.
pub fn bindings_conflict(
    (key, modifiers, context, at_default): (&str, u32, KeyConflictContext, bool),
    (other_key, other_modifiers, other_context, other_default): (
        &str,
        u32,
        KeyConflictContext,
        bool,
    ),
) -> bool {
    key != "key.keyboard.unknown"
        && key == other_key
        && modifiers == other_modifiers
        && context.conflicts(other_context)
        && (!at_default || !other_default)
}

#[cfg(test)]
mod conflict_tests {
    use super::{KeyConflictContext::*, bindings_conflict};

    #[test]
    fn contexts_modifiers_unbound_and_defaults_are_respected() {
        let binding = ("key.keyboard.z", 0, InGame, false);
        assert!(bindings_conflict(
            binding,
            ("key.keyboard.z", 0, Universal, true)
        ));
        assert!(!bindings_conflict(
            binding,
            ("key.keyboard.z", 0, Gui, false)
        ));
        assert!(!bindings_conflict(
            binding,
            ("key.keyboard.z", 1, InGame, false)
        ));
        assert!(!bindings_conflict(
            ("key.keyboard.z", 0, InGame, true),
            ("key.keyboard.z", 0, InGame, true)
        ));
        assert!(!bindings_conflict(
            ("key.keyboard.unknown", 0, Universal, false),
            ("key.keyboard.unknown", 0, Universal, false)
        ));
    }
}

/// The modifier bits of a key binding, in GLFW's input-event encoding.
pub const SHIFT: u32 = 1;
pub const CONTROL: u32 = 2;
pub const ALT: u32 = 4;
static MODIFIER_NAMES: [(u32, &str); 3] = [(SHIFT, "shift"), (CONTROL, "ctrl"), (ALT, "alt")];

/// A binding's value in `horizon-mods.txt`: its modifiers' names, then its key, joined by `+`.
fn encode_binding(key: &str, modifiers: u32) -> String {
    let mut value: String = MODIFIER_NAMES
        .iter()
        .filter(|(bit, _)| modifiers & *bit != 0)
        .map(|(_, name)| format!("{name}+"))
        .collect();
    value.push_str(key);
    value
}

/// Reads a binding's value back; a value without `+` is a bare key with no modifiers.
fn decode_binding(value: &str) -> (String, u32) {
    let (names, key) = value.rsplit_once('+').unwrap_or(("", value));
    let modifiers = names
        .split('+')
        .filter_map(|name| MODIFIER_NAMES.iter().find(|(_, n)| *n == name))
        .fold(0u32, |bits, (bit, _)| bits | bit);
    (key.to_owned(), modifiers)
}

impl ModList {
    pub fn load(dir: &Path) -> Self {
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
    /// Mod `id`'s key `name` as the player bound it (its key and modifiers), else `default`.
    pub fn binding(&self, id: &str, name: &str, default: (&str, u32)) -> (String, u32) {
        match self.keys.get(id).and_then(|k| k.get(name)) {
            Some(value) => decode_binding(value),
            None => (default.0.to_owned(), default.1),
        }
    }
    /// Binds mod `id`'s key `name` to `key` with `modifiers`.
    pub fn set_binding(&mut self, id: &str, name: &str, key: &str, modifiers: u32) {
        let value = encode_binding(key, modifiers);
        self.keys
            .entry(id.to_owned())
            .or_default()
            .insert(name.to_owned(), value);
    }
    /// Drops the player's binding of mod `id`'s key `name`, so it is the default again.
    pub fn reset_binding(&mut self, id: &str, name: &str) {
        if let Some(keys) = self.keys.get_mut(id) {
            keys.remove(name);
        }
    }
    pub fn save(&self, dir: &Path) {
        let path = dir.join(FILE);
        if self.enabled.is_empty()
            && self.disabled.is_empty()
            && self.keys.is_empty()
            && !path.exists()
        {
            return;
        }
        let _ = fs::write(path, self.serialize());
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
    fn a_binding_with_modifiers_round_trips_in_its_keys_line() {
        let mut list = ModList::default();
        list.set_binding("zoom", "in", "key.keyboard.z", SHIFT | ALT);
        assert_eq!(list.serialize(), "key.zoom.in:shift+alt+key.keyboard.z\n");
        let again = ModList::parse(&list.serialize());
        assert_eq!(
            again.binding("zoom", "in", ("key.keyboard.q", 0)),
            ("key.keyboard.z".to_owned(), SHIFT | ALT)
        );
    }

    #[test]
    fn a_plain_key_line_is_a_binding_without_modifiers() {
        let list = ModList::parse("key.zoom.in:key.keyboard.x\n");
        assert_eq!(
            list.binding("zoom", "in", ("key.keyboard.q", CONTROL)),
            ("key.keyboard.x".to_owned(), 0)
        );
        assert_eq!(
            list.binding("zoom", "out", ("key.keyboard.q", CONTROL)),
            ("key.keyboard.q".to_owned(), CONTROL)
        );
    }

    #[test]
    fn reset_restores_the_default_and_drops_the_line() {
        let mut list = ModList::default();
        list.set_binding("zoom", "in", "key.keyboard.x", CONTROL);
        list.reset_binding("zoom", "in");
        assert_eq!(
            list.binding("zoom", "in", ("key.keyboard.q", CONTROL)),
            ("key.keyboard.q".to_owned(), CONTROL)
        );
        assert_eq!(list.serialize(), "");
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
