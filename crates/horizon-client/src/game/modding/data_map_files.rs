//! Shared pack-order data-map file merging.
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};

/// `minecraft:` for an id written without a namespace.
fn full_id(id: &str) -> String {
    if id.contains(':') {
        id.to_owned()
    } else {
        format!("minecraft:{id}")
    }
}

/// A file's value for an entry: the data itself, or the wrapper
/// `{"value": ..., "replace": false}` (`DataMapEntry`'s codec). `replace` only
/// matters to mergers, which types don't have yet: a newer value overrides.
fn entry_value(raw: &Value) -> Option<Value> {
    if let Value::Object(map) = raw
        && let Some(value) = map.get("value")
        && map
            .iter()
            .all(|(k, v)| k == "value" || (k == "replace" && v.is_boolean()))
    {
        return Some(value.clone());
    }
    (!raw.is_null()).then(|| raw.clone())
}

/// One file over what earlier files made of the type: `replace` clears it,
/// `values` attach (a tag to every entry in it), then `remove` takes entries
/// away, as `DataMapFile` is applied. False when the file isn't a data map.
pub(super) fn apply_file(
    map: &mut BTreeMap<String, Value>,
    file: &Value,
    tags: &HashMap<String, HashSet<String>>,
) -> bool {
    let (Some(root), Some(values)) = (file.as_object(), file.get("values")) else {
        return false;
    };
    let Some(values) = values.as_object() else {
        return false;
    };
    if root.get("replace").and_then(Value::as_bool) == Some(true) {
        map.clear();
    }
    // The members of a tag or the entry itself.
    let members = |key: &str| -> Vec<String> {
        match key.strip_prefix('#') {
            Some(tag) => tags
                .get(&full_id(tag))
                .map(|set| set.iter().cloned().collect())
                .unwrap_or_default(),
            None => vec![full_id(key)],
        }
    };
    for (key, raw) in values {
        let Some(value) = entry_value(raw) else {
            continue;
        };
        for entry in members(key) {
            map.insert(entry, value.clone());
        }
    }
    for removal in root
        .get("remove")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        // An entry or `{"key": entry}`, as `DataMapEntry.Removal` reads it.
        let key = removal.as_str().or_else(|| removal["key"].as_str());
        for entry in key.map(members).unwrap_or_default() {
            map.remove(&entry);
        }
    }
    true
}

/// The pack files of `wanted` (map `id`'s key) applied over `map`, in pack
/// order: a file whose conditions fail is left out, one that isn't a data map
/// is skipped with a warning.
pub(super) fn apply_files(
    map: &mut BTreeMap<String, Value>,
    wanted: &str,
    id: &str,
    overlay: &horizon_world::loot::PackOverlay<'_>,
    tags: &HashMap<String, HashSet<String>>,
) {
    let Some((_, versions)) = overlay.files.iter().find(|(k, _)| *k == wanted) else {
        return;
    };
    for bytes in versions {
        let file = serde_json::from_slice::<Value>(bytes).ok();
        if file
            .as_ref()
            .is_some_and(|file| !overlay.conditions.allows_file(file, wanted))
        {
            continue;
        }
        let applied = file.is_some_and(|file| apply_file(map, &file, tags));
        if !applied {
            log::warn!("Horizon: a data map file of {id} is not a data map, skipped");
        }
    }
}

/// One of NeoForge's built-in data maps (`NeoForgeDataMaps`), map `id` on the
/// registry folder `dir`: the pack files merged over `base` (vanilla's table,
/// which NeoForge ships as the map's data, so a pack can `remove` from it).
/// None where no pack has a file for the map.
pub(crate) fn merge_builtin(
    id: &str,
    dir: &str,
    base: impl FnOnce() -> BTreeMap<String, Value>,
    overlay: &horizon_world::loot::PackOverlay<'_>,
    tags: &HashMap<String, HashSet<String>>,
) -> Option<BTreeMap<String, Value>> {
    let (ns, path) = id.split_once(':')?;
    let wanted = format!("data_maps/{ns}:{dir}/{path}");
    if !overlay.files.iter().any(|(k, _)| *k == wanted) {
        return None;
    }
    let mut map = base();
    apply_files(&mut map, &wanted, id, overlay, tags);
    Some(map)
}
