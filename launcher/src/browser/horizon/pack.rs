//! Horizon modpacks from the registry (`.hpack`, docs/modding/src/reference/registry-index.md), each played as its own
//! profile: `horizon-packs/<id>/` in the game's home holds the pack's file, `pack.json` (what
//! was installed) and `home/`, the profile's game folder: its saves, its options, and the pack's
//! mods in `home/horizon-mods/`, each downloaded and checked against the index like any
//! registry mod, turned on or off as the pack says. The pack's overrides (settings, resource
//! packs, a mod's saved settings) are copied in once, when it is installed.
use super::{fetch, index, install_version, local, newest, Index};
use crate::browser::{java, FileRef};
use horizon_mods::{index as idx, modpack::Modpack};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const RECORD: &str = "pack.json";

/// Where registry modpacks live.
pub fn packs_dir() -> PathBuf {
    crate::backend::game_home().join("horizon-packs")
}

/// What `pack.json` keeps of an installed pack.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Installed {
    pub id: String,
    pub name: String,
    pub version: String,
    pub sha256: String,
    /// The pack's mods: `id version`.
    pub mods: Vec<String>,
    #[serde(skip)]
    pub work: PathBuf,
}

/// The installed registry packs, by name.
pub fn list() -> Vec<Installed> {
    list_in(&packs_dir())
}

pub(crate) fn list_in(dir: &Path) -> Vec<Installed> {
    let mut packs: Vec<Installed> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| read(&e.path()))
        .collect();
    packs.sort_by_key(|p| p.name.to_lowercase());
    packs
}

fn read(work: &Path) -> Option<Installed> {
    let text = fs::read_to_string(work.join(RECORD)).ok()?;
    let mut pack: Installed = serde_json::from_str(&text).ok()?;
    pack.work = work.to_path_buf();
    Some(pack)
}

/// Whether `work` is a registry pack's folder.
pub fn is_pack(work: &Path) -> bool {
    // Imported Java packs also have pack.json, with a different record format.
    read(work).is_some()
}

/// The profile's game folder.
pub fn home(work: &Path) -> PathBuf {
    work.join("home")
}

/// Installs the registry modpack `id`: its newest version for this game, then each of its mods.
/// Installing it again updates it; its saves and options stay.
pub fn install(id: &str, progress: &dyn Fn(&str, f32)) -> Result<Installed, String> {
    install_in(&*index()?, local(), id, &packs_dir(), progress)
}

fn install_in(
    index: &Index,
    local: bool,
    id: &str,
    dir: &Path,
    progress: &dyn Fn(&str, f32),
) -> Result<Installed, String> {
    let p = index
        .project(id)
        .ok_or_else(|| format!("{id} isn't in Horizon's registry."))?;
    if p.kind != idx::Kind::Modpack {
        return Err(format!("{} is a {}, not a modpack.", p.name, p.kind.name()));
    }
    let v = newest(index, p)?;
    let work = dir.join(&p.id);
    // Also cover the existence check and failed-first-install cleanup. Inner
    // installers re-enter this same home lock on the current thread.
    let _lock = crate::browser::lock_home(&home(&work)).map_err(|e| e.to_string())?;
    let fresh = !work.exists();
    let result = install_version_in(index, local, p, v, &work, progress);
    if result.is_err() && fresh {
        let _ = fs::remove_dir_all(&work);
    }
    result
}

fn install_version_in(
    index: &Index,
    local: bool,
    p: &idx::Project,
    v: &idx::Version,
    work: &Path,
    progress: &dyn Fn(&str, f32),
) -> Result<Installed, String> {
    let home = home(work);
    let _lock = crate::browser::lock_home(&home).map_err(|e| e.to_string())?;
    let line = format!("{} {}", p.name, v.version);
    let file = FileRef {
        url: v.file.url.clone(),
        name: v.file.name.clone(),
        size: v.file.size,
        sha512: None,
        sha256: Some(v.file.sha256.clone()),
        sha1: None,
        primary: true,
    };
    let path = fetch(&file, &work.join("downloads"), local, &|f| {
        progress(&line, f)
    })?;
    idx::check_file(index, p, v, &path).inspect_err(|_| {
        let _ = fs::remove_file(&path);
    })?;
    let pack = Modpack::read(&path)?;
    fs::create_dir_all(&home).map_err(|e| format!("{}: {e}", home.display()))?;
    let mut mods = Vec::new();
    for m in &pack.manifest.mods {
        let found = index
            .project(&m.id)
            .and_then(|mp| Some((mp, mp.versions.iter().find(|w| w.version == m.version)?)));
        let Some((mp, mv)) = found.filter(|(_, mv)| mv.file.sha256 == m.sha256) else {
            return Err(format!(
                "{} needs {} {}, which isn't in the registry.",
                p.name, m.id, m.version
            ));
        };
        install_version(index, local, mp, mv, &home, progress)?;
        java::set_enabled(&home, &m.id, m.on)?;
        mods.push(format!("{} {}", m.id, m.version));
    }
    // Settings once: the player's own changes to the profile stay on updates.
    let first = !work.join(RECORD).exists();
    if first {
        Modpack::extract_overrides(&path, &home)?;
    }
    let kept = work.join(&v.file.name);
    fs::rename(&path, &kept).map_err(|e| format!("{}: {e}", kept.display()))?;
    let record = Installed {
        id: p.id.clone(),
        name: p.name.clone(),
        version: v.version.clone(),
        sha256: v.file.sha256.clone(),
        mods,
        work: work.to_path_buf(),
    };
    let text = serde_json::to_string_pretty(&record).map_err(|e| e.to_string())?;
    fs::write(work.join(RECORD), text).map_err(|e| e.to_string())?;
    Ok(record)
}

/// Turns the game's command into the pack's profile: its own game folder (the player's
/// options.txt copied in when the pack brings none), its taken-down mods turned off first.
pub fn apply(cmd: &mut Command, work: &Path) -> Result<(), String> {
    let home = home(work);
    let own_home = crate::backend::game_home();
    let _locks =
        horizon_mods::install_lock::lock_homes(&[&home, &own_home]).map_err(|e| e.to_string())?;
    if !is_pack(work) {
        return Err(format!("{} is not an installed modpack.", work.display()));
    }
    fs::create_dir_all(&home).map_err(|e| format!("{}: {e}", home.display()))?;
    let options = home.join("options.txt");
    let own = own_home.join("options.txt");
    if !options.exists() && own.is_file() {
        let _ = fs::copy(own, &options);
    }
    super::before_play(&home);
    cmd.current_dir(&home).env("HORIZON_HOME", &home);
    Ok(())
}

/// Removes an installed pack: its mods and the profile's saves.
pub fn remove(work: &Path) -> Result<(), String> {
    remove_in(work, &packs_dir())
}

fn remove_in(work: &Path, dir: &Path) -> Result<(), String> {
    if work.parent() != Some(dir) {
        return Err(format!("{} is not an installed modpack.", work.display()));
    }
    let _lock = crate::browser::lock_home(&home(work)).map_err(|e| e.to_string())?;
    if !is_pack(work) {
        return Err(format!("{} is not an installed modpack.", work.display()));
    }
    fs::remove_dir_all(work).map_err(|e| format!("{}: {e}", work.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// A local registry: the mod and a pack of it, as `file://` files.
    fn registry(dir: &Path, on: bool, extra: &str) -> Index {
        let hmod_src = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../crates/horizon-mods/tests/index/coords_hud-0.1.0.hmod");
        let hmod = dir.join("coords_hud-0.1.0.hmod");
        fs::copy(hmod_src, &hmod).unwrap();
        let hmod_sha = idx::sha256_file(&hmod).unwrap();
        let hpack = dir.join("cozy-1.0.0.hpack");
        let mut zip = zip::ZipWriter::new(fs::File::create(&hpack).unwrap());
        let o = zip::write::SimpleFileOptions::default();
        zip.start_file("horizon-modpack.toml", o).unwrap();
        write!(
            zip,
            "format = 1\nid = \"cozy\"\nname = \"Cozy\"\nversion = \"1.0.0\"\ngame = \"26.3\"\n\
             [[mods]]\nid = \"coords_hud\"\nversion = \"0.1.0\"\nsha256 = \"{hmod_sha}\"\non = {on}\n"
        )
        .unwrap();
        zip.start_file("overrides/options.txt", o).unwrap();
        zip.write_all(b"renderDistance:8\n").unwrap();
        zip.finish().unwrap();
        let size = |p: &Path| fs::metadata(p).unwrap().len();
        // A `file:///` URL: forward slashes (a Windows path's backslashes would be escapes in
        // the JSON) and a leading slash before a Windows drive.
        let url = |p: &Path| {
            let path = p.display().to_string().replace('\\', "/");
            if path.starts_with('/') {
                path
            } else {
                format!("/{path}")
            }
        };
        let text = format!(
            r#"{{"schema": 1, "projects": [
              {{"id": "coords_hud", "kind": "mod", "name": "Coordinates HUD", "summary": "", "license": "MIT",
                "source": "https://x.test", "versions": [{{"version": "0.1.0", "api": "0.2", "side": "client",
                "capabilities": ["registries"], "game": "26.3", "file": {{"name": "coords_hud-0.1.0.hmod",
                "url": "file://{}", "size": {}, "sha256": "{hmod_sha}"}}}}]}},
              {{"id": "cozy", "kind": "modpack", "name": "Cozy", "summary": "", "license": "MIT",
                "source": "https://x.test", "versions": [{{"version": "1.0.0", "api": "1", "game": "26.3",
                "requires": [{{"id": "coords_hud", "version": "0.1.0", "sha256": "{hmod_sha}"}}],
                "file": {{"name": "cozy-1.0.0.hpack", "url": "file://{}", "size": {}, "sha256": "{}"}}}}]}}
            ]{extra}}}"#,
            url(&hmod),
            size(&hmod),
            url(&hpack),
            size(&hpack),
            idx::sha256_file(&hpack).unwrap()
        );
        Index::parse_local(&text).unwrap()
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "horizon-launcher-hpack-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_pack_installs_as_a_profile_with_its_mods() {
        let dir = scratch("install");
        let index = registry(&dir, false, "");
        let packs = dir.join("horizon-packs");
        let pack = install_in(&index, true, "cozy", &packs, &|_, _| {}).unwrap();
        assert_eq!(pack.mods, ["coords_hud 0.1.0"]);
        let home = home(&packs.join("cozy"));
        assert!(home.join("horizon-mods/coords_hud.hmod").is_file());
        assert!(
            !java::is_enabled(&home, "coords_hud"),
            "the pack turns it off"
        );
        assert_eq!(
            fs::read_to_string(home.join("options.txt")).unwrap(),
            "renderDistance:8\n"
        );
        assert_eq!(
            list_in(&packs),
            [Installed {
                work: packs.join("cozy"),
                ..pack
            }]
        );
        // Settings the player changed stay on an update.
        fs::write(home.join("options.txt"), "renderDistance:12\n").unwrap();
        install_in(&index, true, "cozy", &packs, &|_, _| {}).unwrap();
        assert_eq!(
            fs::read_to_string(home.join("options.txt")).unwrap(),
            "renderDistance:12\n"
        );
        // Not a modpack, and not there.
        assert!(install_in(&index, true, "coords_hud", &packs, &|_, _| {})
            .unwrap_err()
            .contains("not a modpack"));
        assert!(install_in(&index, true, "nope", &packs, &|_, _| {}).is_err());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn pack_download_and_constituent_installs_share_one_home_transaction() {
        let dir = scratch("whole-transaction");
        let index = registry(&dir, false, "");
        let packs = dir.join("horizon-packs");
        let target = home(&packs.join("cozy"));
        let saw_pack = std::cell::Cell::new(false);
        let saw_mod = std::cell::Cell::new(false);
        install_in(&index, true, "cozy", &packs, &|line, _| {
            if line.starts_with("Cozy ") {
                saw_pack.set(true);
            } else {
                saw_mod.set(true);
            }
            std::thread::scope(|scope| {
                scope
                    .spawn(|| {
                        assert!(crate::browser::try_lock_home(&target).is_none());
                    })
                    .join()
                    .unwrap();
            });
        })
        .unwrap();
        assert!(saw_pack.get() && saw_mod.get());
        assert!(!java::is_enabled(&target, "coords_hud"));
        let record = read(&packs.join("cozy")).unwrap();
        assert_eq!(record.mods, ["coords_hud 0.1.0"]);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn pack_removal_waits_for_its_home_transaction() {
        let dir = scratch("remove-transaction");
        let index = registry(&dir, false, "");
        let packs = dir.join("horizon-packs");
        let installed = install_in(&index, true, "cozy", &packs, &|_, _| {}).unwrap();
        let target = home(&installed.work);
        std::thread::scope(|scope| {
            let guard = crate::browser::lock_home(&target).unwrap();
            let (send, receive) = std::sync::mpsc::channel();
            let work = &installed.work;
            let parent = &packs;
            let worker = scope.spawn(move || {
                send.send(()).unwrap();
                remove_in(work, parent).unwrap();
            });
            receive.recv().unwrap();
            assert!(installed.work.join(RECORD).is_file());
            // This nested helper uses the same lock, not a second mutex.
            java::set_enabled(&target, "coords_hud", true).unwrap();
            drop(guard);
            worker.join().unwrap();
        });
        assert!(!installed.work.exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_pack_with_a_taken_down_mod_installs_nothing() {
        let dir = scratch("revoked");
        let index = registry(
            &dir,
            true,
            r#", "revoked": [{"id": "coords_hud", "reason": "Test."}]"#,
        );
        let packs = dir.join("horizon-packs");
        let e = install_in(&index, true, "cozy", &packs, &|_, _| {}).unwrap_err();
        assert!(e.contains("taken down: Test."), "{e}");
        assert!(!packs.join("cozy").exists(), "nothing left behind");
        fs::remove_dir_all(&dir).unwrap();
    }
}
