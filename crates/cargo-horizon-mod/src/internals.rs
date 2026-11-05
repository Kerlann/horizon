//! Tool-written internal declarations and the old/new pin's signature diff.
use horizon_mods::{
    Manifest,
    index::native,
    internals::{self, ListedInternal},
    manifest::MANIFEST,
};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    path::Path,
};
use toml_edit::{DocumentMut, Item, Value};

const USAGE: &str = "usage: cargo horizon-mod internals add <path>…";
const CATALOG: &str = "internals.json";
const CATALOG_LIMIT: u64 = 64 << 20;
const ARCHIVE_LIMIT: u64 = 4 << 30;

pub fn run(args: &[String]) -> Result<(), String> {
    let Some((command, paths)) = args.split_first() else {
        return Err(USAGE.into());
    };
    if command != "add" || paths.is_empty() || paths.iter().any(|p| p.starts_with('-')) {
        return Err(USAGE.into());
    }
    add(Path::new("."), paths)
}

fn add(dir: &Path, paths: &[String]) -> Result<(), String> {
    let path = dir.join(MANIFEST);
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let manifest = Manifest::parse(&text)?;
    let native = manifest
        .native
        .as_ref()
        .ok_or("internals add needs a [native] mod")?;
    let catalog = load(dir, &native.horizon)?;
    // Resolve every request before editing: one missing or gated path leaves the file alone.
    let entries = paths
        .iter()
        .map(|path| internals::resolve(&catalog, &manifest.id, path).map_err(|e| e.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    let mut doc: DocumentMut = text
        .parse()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let mut listed: BTreeMap<_, _> = native
        .internals
        .iter()
        .enumerate()
        .map(|(index, entry)| (entry.path.as_str(), index))
        .collect();
    for entry in entries {
        if let Some(&index) = listed.get(entry.path.as_str()) {
            set_hash(&mut doc, index, &entry.hash)?;
        } else {
            let item = &mut doc["native"]["internals"];
            if item.is_none() {
                *item = toml_edit::value(toml_edit::Array::new());
            }
            // The manifest accepts both inline arrays and [[native.internals]] tables.
            if let Some(array) = item.as_array_mut() {
                let mut internal = toml_edit::InlineTable::new();
                internal.insert("path", entry.path.as_str().into());
                internal.insert("hash", entry.hash.as_str().into());
                internal.fmt();
                array.push(internal);
            } else if let Some(array) = item.as_array_of_tables_mut() {
                let mut internal = toml_edit::Table::new();
                internal["path"] = toml_edit::value(&entry.path);
                internal["hash"] = toml_edit::value(&entry.hash);
                array.push(internal);
            } else {
                return Err(format!("{MANIFEST}: native.internals must be an array"));
            }
            listed.insert(&entry.path, listed.len());
        }
        println!("Added internal {}", entry.path);
    }
    let candidate = doc.to_string();
    fs::write(&path, candidate).map_err(|e| format!("{}: {e}", path.display()))
}

/// Report all declarations, leaving unavailable ones declared so a build still refuses them.
/// Only resolvable entries receive the new game's fingerprint.
pub fn refresh(
    manifest: &Manifest,
    doc: &mut DocumentMut,
    old: &[ListedInternal],
    new: &[ListedInternal],
) -> Result<String, String> {
    let native = manifest
        .native
        .as_ref()
        .ok_or("update needs a [native] mod")?;
    let mut report = String::new();
    for (index, declared) in native.internals.iter().enumerate() {
        let path = &declared.path;
        let entry = match internals::resolve(new, &manifest.id, path) {
            Ok(entry) => entry,
            Err(error) => {
                report.push_str(&format!("missing: {path} ({error})\n"));
                continue;
            }
        };
        match internals::resolve(old, &manifest.id, path) {
            Ok(previous) if previous.hash == entry.hash => {
                report.push_str(&format!("unchanged: {path}\n"));
            }
            previous => {
                let signature =
                    previous.map_or("<unavailable in old game>", |e| e.signature.as_str());
                report.push_str(&format!(
                    "changed: {path}\n  {signature} → {}\n",
                    entry.signature
                ));
            }
        }
        set_hash(doc, index, &entry.hash)?;
    }
    Ok(report)
}

fn set_hash(doc: &mut DocumentMut, index: usize, hash: &str) -> Result<(), String> {
    let item = &mut doc["native"]["internals"];
    let value = if let Some(array) = item.as_array_mut() {
        array
            .get_mut(index)
            .and_then(Value::as_inline_table_mut)
            .and_then(|i| i.get_mut("hash"))
    } else {
        item.as_array_of_tables_mut()
            .and_then(|array| array.get_mut(index))
            .and_then(|i| i.get_mut("hash"))
            .and_then(Item::as_value_mut)
    }
    .ok_or_else(|| format!("{MANIFEST}: internal {index} has no hash"))?;
    set_string(value, hash);
    Ok(())
}

/// Replacing only the value keeps whitespace and comments around tool-written strings.
pub fn set_string(value: &mut Value, text: &str) {
    if value.as_str() == Some(text) {
        return;
    }
    let decor = value.decor().clone();
    *value = Value::from(text);
    *value.decor_mut() = decor;
}

/// The same release tags and archive layout the launcher reads. Keep each pin's catalog in
/// the mod's build folder, so updating can still show the old signature after a release expires.
pub fn load(dir: &Path, revision: &str) -> Result<Vec<ListedInternal>, String> {
    let cache = dir.join("target/horizon-mod/prebuilt").join(revision);
    let path = cache.join(CATALOG);
    if path.is_file() {
        return internals::read(&path);
    }
    let host = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        ("windows", "x86_64") => "x86_64-pc-windows-gnu",
        ("macos", "aarch64") => "aarch64-apple-darwin",
        _ => return Err("no prebuilt game for this computer".into()),
    };
    let name = native::prebuilt_archive(host, false);
    let release = format!(
        "https://github.com/{}/releases/download/{}{revision}",
        native::PREBUILT_REPO,
        native::PREBUILT_TAG
    );
    let get = |file: &str| {
        let url = format!("{release}/{file}");
        ureq::get(&url).call().map_err(|e| format!("{url}: {e}"))
    };
    let sums = get("SHA256SUMS")?
        .into_string()
        .map_err(|e| e.to_string())?;
    let expected = sums
        .lines()
        .find_map(|line| {
            let (hash, file) = line.split_once(char::is_whitespace)?;
            (file.trim().trim_start_matches('*') == name).then_some(hash)
        })
        .ok_or_else(|| format!("{release}/SHA256SUMS: no checksum for {name}"))?;
    println!("Reading internals from the prebuilt game at {revision}");
    cache_catalog(&cache, &name, get(&name)?.into_reader(), expected)
}

/// Verify the whole archive before trusting any catalog bytes; never unpack its other files.
fn cache_catalog(
    cache: &Path,
    name: &str,
    body: impl Read,
    expected: &str,
) -> Result<Vec<ListedInternal>, String> {
    fs::create_dir_all(cache).map_err(|e| format!("{}: {e}", cache.display()))?;
    let path = cache.join(CATALOG);
    let archive = cache.join(format!("{name}.{}.partial", std::process::id()));
    let result = (|| {
        let mut body = body.take(ARCHIVE_LIMIT + 1);
        let mut file = fs::File::create(&archive).map_err(|e| e.to_string())?;
        let mut hash = Sha256::new();
        let mut size = 0;
        let mut buffer = [0; 65536];
        loop {
            let read = body.read(&mut buffer).map_err(|e| e.to_string())?;
            if read == 0 {
                break;
            }
            size += read as u64;
            if size > ARCHIVE_LIMIT {
                return Err(format!("{name}: prebuilt archive exceeds 4 GiB"));
            }
            hash.update(&buffer[..read]);
            file.write_all(&buffer[..read]).map_err(|e| e.to_string())?;
        }
        drop(file);
        if format!("{:x}", hash.finalize()) != expected {
            return Err(format!(
                "The prebuilt game {name} doesn't match its release's checksum. Try again."
            ));
        }
        let file = fs::File::open(&archive).map_err(|e| e.to_string())?;
        let text = catalog_from_archive(file)?;
        let entries = serde_json::from_str(&text).map_err(|e| format!("{CATALOG}: {e}"))?;
        let partial = path.with_extension(format!("json.{}.partial", std::process::id()));
        fs::write(&partial, text).map_err(|e| format!("{}: {e}", partial.display()))?;
        fs::rename(&partial, &path).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(entries)
    })();
    let _ = fs::remove_file(&archive);
    result
}

fn catalog_from_archive(reader: impl Read) -> Result<String, String> {
    let reader = flate2::read::GzDecoder::new(reader).take(8 << 30);
    let mut archive = tar::Archive::new(reader);
    for entry in archive.entries().map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path().map_err(|e| e.to_string())?;
        if path != Path::new(CATALOG) && path != Path::new("./internals.json") {
            continue;
        }
        if !entry.header().entry_type().is_file() {
            return Err(format!("{CATALOG}: not a file in the prebuilt archive"));
        }
        let mut text = String::new();
        entry
            .take(CATALOG_LIMIT + 1)
            .read_to_string(&mut text)
            .map_err(|e| e.to_string())?;
        if text.len() as u64 > CATALOG_LIMIT {
            return Err(format!("{CATALOG}: catalog exceeds 64 MiB"));
        }
        return Ok(text);
    }
    Err(format!("prebuilt archive has no {CATALOG}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn archive(path: &str, kind: tar::EntryType, contents: &str) -> Vec<u8> {
        let gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        let mut archive = tar::Builder::new(gzip);
        let mut header = tar::Header::new_gnu();
        header.set_size(contents.len() as u64);
        header.set_mode(0o644);
        header.set_entry_type(kind);
        header.set_cksum();
        archive
            .append_data(&mut header, path, contents.as_bytes())
            .unwrap();
        archive.into_inner().unwrap().finish().unwrap()
    }

    #[test]
    fn reads_only_the_root_catalog_from_the_prebuilt_archive() {
        let catalog = include_str!("../tests/fixtures/internals-old.json");
        let bytes = archive("./internals.json", tar::EntryType::Regular, catalog);
        assert_eq!(catalog_from_archive(bytes.as_slice()).unwrap(), catalog);
        let bytes = archive("deps/internals.json", tar::EntryType::Regular, catalog);
        assert_eq!(
            catalog_from_archive(bytes.as_slice()).unwrap_err(),
            "prebuilt archive has no internals.json"
        );
        let bytes = archive("internals.json", tar::EntryType::Symlink, "");
        assert!(
            catalog_from_archive(bytes.as_slice())
                .unwrap_err()
                .contains("not a file")
        );
    }

    #[test]
    fn downloaded_catalog_is_cached_only_after_checksum_and_json_checks() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/i3-d/tests")
            .join(format!("archive-cache-{}", std::process::id()));
        let catalog = include_str!("../tests/fixtures/internals-old.json");
        let bytes = archive("./internals.json", tar::EntryType::Regular, catalog);
        let error = cache_catalog(&dir, "game.tar.gz", bytes.as_slice(), "wrong").unwrap_err();
        assert!(error.contains("doesn't match its release's checksum"));
        assert!(!dir.join(CATALOG).exists());
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);

        let invalid = archive("./internals.json", tar::EntryType::Regular, "not JSON");
        let hash = format!("{:x}", Sha256::digest(&invalid));
        assert!(cache_catalog(&dir, "game.tar.gz", invalid.as_slice(), &hash).is_err());
        assert!(!dir.join(CATALOG).exists());

        let hash = format!("{:x}", Sha256::digest(&bytes));
        let entries = cache_catalog(&dir, "game.tar.gz", bytes.as_slice(), &hash).unwrap();
        assert_eq!(entries[0].path, "horizon_client::game::Game");
        assert_eq!(fs::read_to_string(dir.join(CATALOG)).unwrap(), catalog);
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn tools_and_launcher_share_the_freshness_check() {
        let entries: Vec<ListedInternal> =
            serde_json::from_str(include_str!("../tests/fixtures/internals-new.json")).unwrap();
        let mut declared = horizon_mods::manifest::Internal {
            path: "horizon_client::game::Game<'_>::reach".into(),
            hash: "2222".into(),
        };
        assert_eq!(
            internals::check(&entries, "example", &declared)
                .unwrap_err()
                .to_string(),
            "E_INTERNAL_CHANGED: example uses horizon_client::game::Game<'_>::reach, whose signature is now impl Game<'_> { fn reach(&self, hand: Hand) -> f64 }; run cargo horizon-mod update to see what changed."
        );
        declared.hash = "abcd".into();
        internals::check(&entries, "example", &declared).unwrap();
    }
}
