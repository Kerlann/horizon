//! Decode a whole data-map file before applying any replace, values or removals.
use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};

type Tags = HashMap<String, HashSet<String>>;

#[derive(Deserialize)]
struct RawFile {
    #[serde(default)]
    replace: bool,
    #[serde(default)]
    values: BTreeMap<String, Value>,
    #[serde(default)]
    remove: Vec<Removal>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Removal {
    Id(String),
    Object { key: String },
}

enum Key {
    Entry(String),
    Tag(String),
}

/// A resource location decoded at the file/value boundary.
pub(super) fn resource_id(raw: &str) -> Result<String, String> {
    let (namespace, path) = raw.split_once(':').unwrap_or(("minecraft", raw));
    let valid = |value: &str, slash| {
        !value.is_empty()
            && value.chars().all(|c| {
                c.is_ascii_lowercase()
                    || c.is_ascii_digit()
                    || "_.-".contains(c)
                    || (slash && c == '/')
            })
    };
    if !valid(namespace, false) || !valid(path, true) {
        return Err(format!("invalid resource location {raw:?}"));
    }
    Ok(format!("{namespace}:{path}"))
}

impl Key {
    fn decode(raw: &str) -> Result<Self, String> {
        match raw.strip_prefix('#') {
            Some(tag) => resource_id(tag).map(Self::Tag),
            None => resource_id(raw).map(Self::Entry),
        }
    }
    fn visit(&self, tags: &Tags, mut visit: impl FnMut(&str)) {
        match self {
            Self::Entry(id) => visit(id),
            Self::Tag(tag) => {
                if let Some(members) = tags.get(tag) {
                    for id in members {
                        visit(id);
                    }
                }
            }
        }
    }
}

/// A validated file: callers cannot apply a half-decoded file.
pub(super) struct File<T> {
    replace: bool,
    values: Vec<(Key, T)>,
    remove: Vec<Key>,
}

impl<T: Clone> File<T> {
    pub(super) fn apply(
        self,
        map: &mut BTreeMap<String, T>,
        tags: &Tags,
        known: impl Fn(&str) -> bool,
        context: &str,
    ) {
        if self.replace {
            map.clear();
        }
        for (key, value) in self.values {
            key.visit(tags, |id| {
                if known(id) {
                    map.insert(id.to_owned(), value.clone());
                } else {
                    log::warn!("Horizon: {context}: unknown target {id}, entry skipped");
                }
            });
        }
        for key in self.remove {
            key.visit(tags, |id| {
                map.remove(id);
            });
        }
    }
}

fn entry_value(raw: &Value) -> Result<&Value, String> {
    if let Some(object) = raw.as_object()
        && let Some(value) = object.get("value")
    {
        if let Some(replace) = object.get("replace")
            && !replace.is_boolean()
        {
            return Err("entry replace must be a boolean".into());
        }
        return Ok(value);
    }
    Ok(raw)
}

fn decode<T>(
    file: Value,
    conditions: &horizon_world::conditions::LoadConditions,
    context: &str,
    value: impl Fn(&Value) -> Result<Option<T>, String>,
) -> Result<File<T>, String> {
    let raw: RawFile = serde_json::from_value(file).map_err(|error| error.to_string())?;
    let mut values = Vec::new();
    for (key, raw) in raw.values {
        let key = Key::decode(&key)?;
        if !conditions.allows_file(&raw, context) {
            continue;
        }
        if let Some(value) = value(entry_value(&raw)?)? {
            values.push((key, value));
        }
    }
    let remove = raw
        .remove
        .into_iter()
        .map(|removal| {
            let key = match removal {
                Removal::Id(key) | Removal::Object { key } => key,
            };
            Key::decode(&key)
        })
        .collect::<Result<_, _>>()?;
    Ok(File {
        replace: raw.replace,
        values,
        remove,
    })
}

/// Pack-order loading shared by raw mod maps and typed builtin codecs. Each
/// value decodes before target membership, so even an unknown target cannot
/// conceal a bad value and allow the rest of that file to apply.
pub(super) fn load<T: Clone>(
    id: &str,
    directory: &str,
    mut map: BTreeMap<String, T>,
    overlay: &horizon_world::loot::PackOverlay<'_>,
    tags: &Tags,
    value: impl Fn(&Value) -> Result<Option<T>, String>,
    known: impl Fn(&str) -> bool,
) -> (BTreeMap<String, T>, bool) {
    let (namespace, path) = id.split_once(':').expect("validated data-map type ID");
    let wanted = format!("data_maps/{namespace}:{directory}/{path}");
    let mut applied = false;
    if let Some((_, versions)) = overlay.files.iter().find(|(key, _)| *key == wanted) {
        for bytes in versions {
            let file = serde_json::from_slice::<Value>(bytes).map_err(|error| error.to_string());
            let prepared = file.and_then(|file| {
                if !overlay.conditions.allows_file(&file, &wanted) {
                    return Ok(None);
                }
                decode(file, overlay.conditions, &wanted, &value).map(Some)
            });
            match prepared {
                Ok(Some(file)) => {
                    file.apply(&mut map, tags, &known, &wanted);
                    applied = true;
                }
                Ok(None) => {}
                Err(error) => log::warn!("Horizon: {id} ({directory}): file skipped: {error}"),
            }
        }
    }
    (map, applied)
}

pub(super) fn apply_files(
    map: &mut BTreeMap<String, Value>,
    directory: &str,
    id: &str,
    overlay: &horizon_world::loot::PackOverlay<'_>,
    tags: &Tags,
) {
    let (loaded, _) = load(
        id,
        directory,
        std::mem::take(map),
        overlay,
        tags,
        |value| Ok((!value.is_null()).then(|| value.clone())),
        |_| true,
    );
    *map = loaded;
}

pub(crate) fn merge_builtin(
    id: &str,
    directory: &str,
    base: impl FnOnce() -> BTreeMap<String, Value>,
    overlay: &horizon_world::loot::PackOverlay<'_>,
    tags: &Tags,
) -> Option<BTreeMap<String, Value>> {
    let (map, applied) = load(
        id,
        directory,
        base(),
        overlay,
        tags,
        |value| Ok((!value.is_null()).then(|| value.clone())),
        |_| true,
    );
    applied.then_some(map)
}

#[cfg(test)]
pub(super) fn apply_file(map: &mut BTreeMap<String, Value>, file: &Value, tags: &Tags) -> bool {
    match decode(
        file.clone(),
        horizon_world::loot::PackOverlay::NONE.conditions,
        "test map",
        |value| Ok((!value.is_null()).then(|| value.clone())),
    ) {
        Ok(file) => {
            file.apply(map, tags, |_| true, "test map");
            true
        }
        Err(_) => false,
    }
}
