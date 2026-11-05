//! Automatic updates against fixture indexes: a content mod `ruby` whose versions are real
//! `.hmod` files on disk, listed in an index the way the registry lists them.
use super::*;
use crate::browser::horizon::tests::{sample, scratch};

const MANIFEST: &str = "id = \"ruby\"\nname = \"Ruby\"\napi = \"0.2\"\nside = \"both\"\n\
                        capabilities = [\"registries\"]\n";
const BLOCK: &[u8] = br#"{"template": "minecraft:iron_block"}"#;
const ITEM: &[u8] = br#"{"template": "minecraft:diamond"}"#;

/// `ruby` at `version` with blocks `blocks` and items `items`, written into `dir`.
fn ruby(dir: &Path, version: &str, blocks: &[&str], items: &[&str]) -> PathBuf {
    let names: Vec<(String, &[u8])> = blocks
        .iter()
        .map(|b| (format!("data/ruby/horizon/block/{b}.json"), BLOCK))
        .chain(
            items
                .iter()
                .map(|i| (format!("data/ruby/horizon/item/{i}.json"), ITEM)),
        )
        .collect();
    let files: Vec<(&str, &[u8])> = names.iter().map(|(n, b)| (n.as_str(), *b)).collect();
    let manifest = format!("{MANIFEST}version = \"{version}\"\n");
    let bytes = horizon_mods::Package::write_files(&manifest, &files).unwrap();
    let path = dir.join(format!("ruby-{version}.hmod"));
    fs::write(&path, bytes).unwrap();
    path
}

/// An index listing `ruby`'s `versions` (newest first: version, game, file on disk).
fn index_of(versions: &[(&str, &str, &Path)]) -> Index {
    let mut index = sample();
    let base = index.project("coords_hud").unwrap().clone();
    let listed = versions
        .iter()
        .map(|(version, game, path)| index::Version {
            version: (*version).into(),
            api: "0.2".into(),
            side: Some(horizon_mods::Side::Both),
            capabilities: vec![horizon_mods::Capability::Registries],
            game: (*game).into(),
            requires: Vec::new(),
            file: index::File {
                name: path.file_name().unwrap().to_string_lossy().into_owned(),
                url: format!("file://{}", path.display()),
                size: fs::metadata(path).unwrap().len(),
                sha256: index::sha256_file(path).unwrap(),
                signature: None,
            },
            ..base.versions[0].clone()
        })
        .collect();
    index.projects.push(index::Project {
        id: "ruby".into(),
        name: "Ruby".into(),
        key: None,
        versions: listed,
        ..base
    });
    index
}

const PLAY: Context = Context { in_pack: false };

/// The browser's entry for `ruby` at `version`.
fn entry(version: &str) -> installed::Entry {
    installed::Entry {
        source: SourceId::Horizon,
        project: "ruby".into(),
        slug: "ruby".into(),
        title: "Ruby".into(),
        kind: Kind::Mod,
        version: version.into(),
        number: version.into(),
        file: "horizon-mods/ruby.hmod".into(),
        checksum: String::new(),
        page: String::new(),
        dependency: false,
        verdict: None,
        skipped: Vec::new(),
        namespaces: Vec::new(),
    }
}

fn version_of(d: Decision) -> Option<String> {
    match d {
        Decision::Update(v) => Some(v.version.clone()),
        Decision::Skip => None,
    }
}

#[test]
fn updates_forward_on_the_same_game_only() {
    let dir = scratch("auto-decide");
    let (a, b, c) = (
        ruby(&dir, "1.0.0", &["a"], &[]),
        ruby(&dir, "1.1.0", &["a"], &[]),
        ruby(&dir, "1.2.0", &["a"], &[]),
    );
    let index = index_of(&[
        ("1.2.0", GAME, &c),
        ("1.1.0", GAME, &b),
        ("1.0.0", GAME, &a),
    ]);
    let to = |e: &installed::Entry, ctx: &Context| version_of(decide(&index, e, ctx));
    assert_eq!(to(&entry("1.0.0"), &PLAY), Some("1.2.0".into()));
    assert_eq!(to(&entry("1.2.0"), &PLAY), None, "already the newest");
    assert_eq!(
        to(&entry("0.9.0"), &PLAY),
        None,
        "a version the index no longer lists"
    );
    let in_pack = Context {
        in_pack: true,
        ..PLAY
    };
    assert_eq!(to(&entry("1.0.0"), &in_pack), None, "a modpack's profile");
    let native = installed::Entry {
        file: "horizon-native-mods/ruby.hmod".into(),
        ..entry("1.0.0")
    };
    assert_eq!(
        to(&native, &PLAY),
        None,
        "native mods require an explicit update"
    );
    let data = installed::Entry {
        kind: Kind::DataPack,
        ..entry("1.0.0")
    };
    assert_eq!(to(&data, &PLAY), None);
    let modrinth = installed::Entry {
        source: SourceId::Modrinth,
        ..entry("1.0.0")
    };
    assert_eq!(to(&modrinth, &PLAY), None);

    // Never backwards: the newest was taken down while it is installed.
    let mut down = index.clone();
    down.revoked.push(index::Revoked {
        id: "ruby".into(),
        sha256: Some(index::sha256_file(&c).unwrap()),
        reason: "Test.".into(),
        date: String::new(),
    });
    assert_eq!(version_of(decide(&down, &entry("1.2.0"), &PLAY)), None);
    // Never across Minecraft versions: what is installed was for another game.
    let other = index_of(&[("1.2.0", GAME, &c), ("1.0.0", "26.2", &a)]);
    assert_eq!(version_of(decide(&other, &entry("1.0.0"), &PLAY)), None);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_new_version_that_drops_content_is_named() {
    let dir = scratch("auto-dropped");
    let old = ruby(&dir, "1.0.0", &["lamp", "ore"], &["gem"]);
    let more = ruby(&dir, "1.1.0", &["lamp", "ore", "brick"], &["gem"]);
    let less = ruby(&dir, "1.2.0", &["lamp"], &[]);
    assert!(dropped(&old, &more).unwrap().is_empty());
    assert_eq!(
        dropped(&old, &less).unwrap(),
        ["block ruby:ore", "item ruby:gem"]
    );
    fs::remove_dir_all(&dir).unwrap();
}

/// The installed ruby: the browser's entry and the file's bytes.
fn installed_ruby(home: &Path) -> (String, Vec<u8>) {
    let list = installed::Installed::load(home);
    let e = list.find(SourceId::Horizon, "ruby").unwrap();
    (e.version.clone(), fs::read(home.join(&e.file)).unwrap())
}

#[test]
fn updates_holds_and_keeps_the_replaced_file_until_the_next_start() {
    let dir = scratch("auto-run");
    let home = dir.join("home");
    let v1 = ruby(&dir, "1.0.0", &["lamp", "ore"], &[]);
    let v2 = ruby(&dir, "1.1.0", &["lamp", "ore", "brick"], &[]);
    let v3 = ruby(&dir, "1.2.0", &["lamp"], &[]);
    let first = index_of(&[("1.0.0", GAME, &v1)]);
    let p = first.project("ruby").unwrap();
    super::super::install_version(&first, true, p, &p.versions[0], &home, &|_, _| {}).unwrap();

    // 1.1.0 adds a block: updated, the 1.0.0 file kept aside.
    let index = index_of(&[("1.1.0", GAME, &v2), ("1.0.0", GAME, &v1)]);
    let report = run_with(&index, true, &home, &PLAY, &|_, _| {});
    assert_eq!(report.updated, ["Ruby 1.1.0"]);
    assert!(
        report.held.is_empty() && report.failed.is_empty(),
        "{report:?}"
    );
    assert_eq!(
        installed_ruby(&home),
        ("1.1.0".into(), fs::read(&v2).unwrap())
    );
    let kept = kept_list(&home);
    assert_eq!(kept.len(), 1);
    assert_eq!(
        fs::read(home.join(&kept[0].copy)).unwrap(),
        fs::read(&v1).unwrap()
    );
    // A second run in the same launcher keeps it; the next start drops it, the update done.
    run_with(&index, true, &home, &PLAY, &|_, _| {});
    assert!(home.join(&kept[0].copy).is_file());
    save_kept(
        &home,
        &[Kept {
            pid: 0,
            ..kept[0].clone()
        }],
    )
    .unwrap();
    recover(&home);
    assert!(!home.join(&kept[0].copy).exists());
    assert!(kept_list(&home).is_empty());

    // 1.2.0 drops two blocks: it waits, and 1.1.0 stays as it was.
    let index = index_of(&[
        ("1.2.0", GAME, &v3),
        ("1.1.0", GAME, &v2),
        ("1.0.0", GAME, &v1),
    ]);
    let report = run_with(&index, true, &home, &PLAY, &|_, _| {});
    assert!(report.updated.is_empty(), "{report:?}");
    assert_eq!(report.held.len(), 1);
    assert!(
        report.held[0].contains("block ruby:brick, block ruby:ore"),
        "{}",
        report.held[0]
    );
    assert_eq!(
        installed_ruby(&home),
        ("1.1.0".into(), fs::read(&v2).unwrap())
    );
    assert!(kept_list(&home).is_empty());
    assert!(!home.join("mod-browser/downloads/ruby-1.2.0.hmod").exists());
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn wasm_update_waits_for_the_player() {
    let dir = scratch("auto-wasm");
    let home = dir.join("home");
    let old = ruby(&dir, "1.0.0", &["lamp"], &[]);
    let module = b"\0asm\x01\0\0\0";
    let manifest = format!(
        "{MANIFEST}version = \"1.1.0\"\n[wasm]\nsha512 = \"{}\"\ntoolchain = \"1.99.1\"\nbuilt_with = \"test\"\n",
        horizon_mods::manifest::sha512_of(module)
    );
    let new = dir.join("ruby-1.1.0.hmod");
    fs::write(
        &new,
        horizon_mods::Package::write_files(&manifest, &[("mod.wasm", module)]).unwrap(),
    )
    .unwrap();
    let first = index_of(&[("1.0.0", GAME, &old)]);
    let p = first.project("ruby").unwrap();
    super::super::install_version(&first, true, p, &p.versions[0], &home, &|_, _| {}).unwrap();
    let index = index_of(&[("1.1.0", GAME, &new), ("1.0.0", GAME, &old)]);
    let report = run_with(&index, true, &home, &PLAY, &|_, _| {});
    assert!(report.updated.is_empty(), "{report:?}");
    assert_eq!(report.held.len(), 1, "{report:?}");
    assert_eq!(
        installed_ruby(&home),
        ("1.0.0".into(), fs::read(&old).unwrap())
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn backup_paths_stay_in_the_backup_folder() {
    let dir = scratch("auto-backup-path");
    let home = dir.join("home");
    let v1 = ruby(&dir, "1.0.0", &["lamp"], &[]);
    let index = index_of(&[("1.0.0", GAME, &v1)]);
    let p = index.project("ruby").unwrap();
    super::super::install_version(&index, true, p, &p.versions[0], &home, &|_, _| {}).unwrap();
    let mut entry = installed::Installed::load(&home).entries.remove(0);
    entry.version = "../../outside".into();
    let kept = keep(&home, &entry).unwrap();
    assert_eq!(kept.copy, "mod-browser/replaced/ruby");
    assert!(home.join(&kept.copy).is_file());

    let outside = home.join("outside");
    fs::write(&outside, b"untouched").unwrap();
    save_kept(
        &home,
        &[Kept {
            copy: "mod-browser/replaced/../../outside".into(),
            pid: 0,
            ..kept
        }],
    )
    .unwrap();
    recover(&home);
    assert_eq!(fs::read(outside).unwrap(), b"untouched");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn failed_restore_keeps_its_record_for_retry() {
    let dir = scratch("auto-restore-retry");
    let home = dir.join("home");
    let v1 = ruby(&dir, "1.0.0", &["lamp"], &[]);
    let index = index_of(&[("1.0.0", GAME, &v1)]);
    let p = index.project("ruby").unwrap();
    super::super::install_version(&index, true, p, &p.versions[0], &home, &|_, _| {}).unwrap();
    let entry = installed::Installed::load(&home).entries.remove(0);
    let mut kept = keep(&home, &entry).unwrap();
    kept.pid = 0;
    save_kept(&home, &[kept.clone()]).unwrap();
    let target = entry.path(&home).unwrap();
    fs::remove_file(&target).unwrap();
    fs::create_dir(&target).unwrap();
    recover(&home);
    assert_eq!(kept_list(&home).len(), 1);
    assert!(home.join(&kept.copy).is_file());
    let v2 = ruby(&dir, "1.1.0", &["lamp", "brick"], &[]);
    let newer = index_of(&[("1.1.0", GAME, &v2), ("1.0.0", GAME, &v1)]);
    let report = run_with(&newer, true, &home, &PLAY, &|_, _| {});
    assert!(report.updated.is_empty());
    assert_eq!(kept_list(&home).len(), 1);
    assert!(home.join(&kept.copy).is_file());
    fs::remove_dir(target).unwrap();
    recover(&home);
    assert_eq!(
        installed_ruby(&home),
        ("1.0.0".into(), fs::read(v1).unwrap())
    );
    assert!(kept_list(&home).is_empty());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn an_update_cut_short_is_rolled_back_at_the_next_start() {
    let dir = scratch("auto-rollback");
    let home = dir.join("home");
    let v1 = ruby(&dir, "1.0.0", &["lamp"], &[]);
    let index = index_of(&[("1.0.0", GAME, &v1)]);
    let p = index.project("ruby").unwrap();
    super::super::install_version(&index, true, p, &p.versions[0], &home, &|_, _| {}).unwrap();
    crate::browser::java::set_enabled(&home, "ruby", false).unwrap();
    let before = installed::Installed::load(&home);
    let old = before.find(SourceId::Horizon, "ruby").unwrap().clone();
    // An earlier launcher kept 1.0.0, then stopped with a half-written file and the list still
    // naming 1.0.0.
    let mut k = keep(&home, &old).unwrap();
    k.pid = 0;
    save_kept(&home, &[k]).unwrap();
    fs::write(home.join(&old.file), b"half").unwrap();
    fs::write(home.join("horizon-mods.txt"), "mod.ruby:true\n").unwrap();
    assert!(run(&home, &PLAY, false, &|_, _| {}).is_none());
    assert_eq!(
        installed_ruby(&home),
        ("1.0.0".into(), fs::read(&v1).unwrap())
    );
    assert!(kept_list(&home).is_empty());
    assert!(!crate::browser::java::is_enabled(&home, "ruby"));
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn disabled_content_stays_disabled_but_explicit_install_enables() {
    let dir = scratch("auto-disabled");
    let home = dir.join("home");
    let v1 = ruby(&dir, "1.0.0", &["lamp"], &[]);
    let v2 = ruby(&dir, "1.1.0", &["lamp", "brick"], &[]);
    let v3 = ruby(&dir, "1.2.0", &["lamp", "brick", "ore"], &[]);
    let first = index_of(&[("1.0.0", GAME, &v1)]);
    let p = first.project("ruby").unwrap();
    super::super::install_version(&first, true, p, &p.versions[0], &home, &|_, _| {}).unwrap();
    crate::browser::java::set_enabled(&home, "ruby", false).unwrap();
    crate::browser::java::set_enabled(&home, "other", true).unwrap();
    let on_list = fs::read(home.join("horizon-mods.txt")).unwrap();

    let index = index_of(&[("1.1.0", GAME, &v2), ("1.0.0", GAME, &v1)]);
    let report = run_with(&index, true, &home, &PLAY, &|_, _| {});
    assert_eq!(report.updated, ["Ruby 1.1.0"]);
    assert_eq!(
        installed_ruby(&home),
        ("1.1.0".into(), fs::read(&v2).unwrap())
    );
    assert!(!crate::browser::java::is_enabled(&home, "ruby"));
    assert_eq!(fs::read(home.join("horizon-mods.txt")).unwrap(), on_list);
    assert_eq!(kept_list(&home)[0].enabled, Some(false));

    let explicit = index_of(&[("1.2.0", GAME, &v3), ("1.1.0", GAME, &v2)]);
    let p = explicit.project("ruby").unwrap();
    super::super::install_version(&explicit, true, p, &p.versions[0], &home, &|_, _| {}).unwrap();
    assert!(crate::browser::java::is_enabled(&home, "ruby"));
    assert!(crate::browser::java::is_enabled(&home, "other"));
    assert_eq!(
        installed_ruby(&home),
        ("1.2.0".into(), fs::read(&v3).unwrap())
    );
    assert!(kept_list(&home).is_empty());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn list_save_failure_after_the_enabled_write_restores_disabled_state_and_keeps_retry() {
    let dir = scratch("auto-disabled-save-failure");
    let home = dir.join("home");
    let v1 = ruby(&dir, "1.0.0", &["lamp"], &[]);
    let v2 = ruby(&dir, "1.1.0", &["lamp", "brick"], &[]);
    let first = index_of(&[("1.0.0", GAME, &v1)]);
    let p = first.project("ruby").unwrap();
    super::super::install_version(&first, true, p, &p.versions[0], &home, &|_, _| {}).unwrap();
    fs::write(
        home.join("horizon-mods.txt"),
        "mod.ruby:false\nmod.other:true\n",
    )
    .unwrap();
    let index = index_of(&[("1.1.0", GAME, &v2), ("1.0.0", GAME, &v1)]);
    let part = home.join(".mod-browser.json.part");
    let report = run_with(&index, true, &home, &PLAY, &|_, _| {
        // The downloaded package still passes all checks. Only the final browser-list save
        // fails, after the package rename and enabled-list write have succeeded.
        fs::create_dir_all(&part).unwrap();
    });
    assert!(report.updated.is_empty());
    assert_eq!(report.failed.len(), 1, "{report:?}");
    assert!(
        report.failed[0].contains(".mod-browser.json.part"),
        "{report:?}"
    );
    assert_eq!(
        installed_ruby(&home),
        ("1.0.0".into(), fs::read(&v1).unwrap())
    );
    assert!(!crate::browser::java::is_enabled(&home, "ruby"));
    assert_eq!(
        fs::read_to_string(home.join("horizon-mods.txt")).unwrap(),
        "mod.other:true\n"
    );
    // Rollback's list save hits the same obstruction. Its backup and disabled-state snapshot
    // must survive, and manual mutation must refuse rather than bypass the unfinished restore.
    let kept = kept_list(&home);
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].enabled, Some(false));
    assert_eq!(kept[0].pid, 0);
    assert!(home.join(&kept[0].copy).is_file());
    assert!(installed::remove(&home, "ruby").is_err());
    fs::remove_dir(&part).unwrap();
    recover(&home);
    assert_eq!(
        installed_ruby(&home),
        ("1.0.0".into(), fs::read(v1).unwrap())
    );
    assert!(!crate::browser::java::is_enabled(&home, "ruby"));
    assert!(kept_list(&home).is_empty());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn automatic_snapshot_serializes_manual_install_and_remove() {
    use std::sync::mpsc;
    use std::time::Duration;

    for remove in [false, true] {
        let dir = scratch(if remove {
            "auto-manual-remove"
        } else {
            "auto-manual-install"
        });
        let home = dir.join("home");
        let v1 = ruby(&dir, "1.0.0", &["lamp"], &[]);
        let v2 = ruby(&dir, "1.1.0", &["lamp", "brick"], &[]);
        let first = index_of(&[("1.0.0", GAME, &v1)]);
        let p = first.project("ruby").unwrap();
        super::super::install_version(&first, true, p, &p.versions[0], &home, &|_, _| {}).unwrap();
        if remove {
            let hud = first.project("coords_hud").unwrap();
            super::super::install_version(&first, true, hud, &hud.versions[0], &home, &|_, _| {})
                .unwrap();
        }
        let index = index_of(&[("1.1.0", GAME, &v2), ("1.0.0", GAME, &v1)]);
        let (paused_tx, paused_rx) = mpsc::channel();
        let (resume_tx, resume_rx) = mpsc::channel();
        let auto_home = home.clone();
        let auto_index = index.clone();
        let automatic = std::thread::spawn(move || {
            let paused = std::cell::Cell::new(false);
            run_with(&auto_index, true, &auto_home, &PLAY, &|_, _| {
                if !paused.replace(true) {
                    paused_tx.send(()).unwrap();
                    resume_rx.recv_timeout(Duration::from_secs(10)).unwrap();
                }
            })
        });
        // A has read the browser list and is waiting for its download to finish.
        paused_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        let (blocked_tx, blocked_rx) = mpsc::channel();
        let manual_home = home.clone();
        let manual = std::thread::spawn(move || {
            crate::browser::mutation::on_wait(move || blocked_tx.send(()).unwrap());
            if remove {
                installed::remove(&manual_home, "coords_hud").map(|e| vec![e])
            } else {
                let p = super::super::project_of(index.project("coords_hud").unwrap()).unwrap();
                super::super::install_from(&index, true, &p, &manual_home, &|_, _| {})
            }
        });
        // Observe B actually waiting at the shared lease, not just a spawned thread. No sleep
        // or timeout-based claim of exclusion: only then let A commit its snapshot.
        blocked_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        resume_tx.send(()).unwrap();
        let report = automatic.join().unwrap();
        assert_eq!(report.updated, ["Ruby 1.1.0"]);
        manual.join().unwrap().unwrap();
        let list = installed::Installed::load(&home);
        assert_eq!(
            list.find(SourceId::Horizon, "ruby").unwrap().version,
            "1.1.0"
        );
        assert_eq!(list.find(SourceId::Horizon, "coords_hud").is_none(), remove);
        assert_eq!(home.join("horizon-mods/coords_hud.hmod").is_file(), !remove);
        assert_eq!(installed_ruby(&home).1, fs::read(&v2).unwrap());
        // Removing an automatically updated mod must settle its completed journal, so a
        // future recovery cannot resurrect it just because the file is now intentionally gone.
        installed::remove(&home, "ruby").unwrap();
        recover(&home);
        assert!(installed::Installed::load(&home)
            .find(SourceId::Horizon, "ruby")
            .is_none());
        assert!(!home.join("horizon-mods/ruby.hmod").exists());
        fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
fn automatic_decisions_wait_for_a_manual_update_to_commit() {
    use std::sync::mpsc;
    use std::time::Duration;

    let dir = scratch("manual-auto-update");
    let home = dir.join("home");
    let v1 = ruby(&dir, "1.0.0", &["lamp"], &[]);
    let v2 = ruby(&dir, "1.1.0", &["lamp", "brick"], &[]);
    let first = index_of(&[("1.0.0", GAME, &v1)]);
    let p = first.project("ruby").unwrap();
    super::super::install_version(&first, true, p, &p.versions[0], &home, &|_, _| {}).unwrap();
    let index = index_of(&[("1.1.0", GAME, &v2), ("1.0.0", GAME, &v1)]);
    let (paused_tx, paused_rx) = mpsc::channel();
    let (resume_tx, resume_rx) = mpsc::channel();
    let manual_home = home.clone();
    let manual_index = index.clone();
    let manual = std::thread::spawn(move || {
        let p = super::super::project_of(manual_index.project("ruby").unwrap()).unwrap();
        let paused = std::cell::Cell::new(false);
        super::super::install_from(&manual_index, true, &p, &manual_home, &|_, _| {
            if !paused.replace(true) {
                paused_tx.send(()).unwrap();
                resume_rx.recv_timeout(Duration::from_secs(10)).unwrap();
            }
        })
    });
    paused_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    let (blocked_tx, blocked_rx) = mpsc::channel();
    let auto_home = home.clone();
    let automatic = std::thread::spawn(move || {
        crate::browser::mutation::on_wait(move || blocked_tx.send(()).unwrap());
        run_with(&index, true, &auto_home, &PLAY, &|_, _| {})
    });
    blocked_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    resume_tx.send(()).unwrap();
    manual.join().unwrap().unwrap();
    assert_eq!(automatic.join().unwrap(), Report::default());
    assert_eq!(
        installed_ruby(&home),
        ("1.1.0".into(), fs::read(v2).unwrap())
    );
    assert!(kept_list(&home).is_empty());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn the_report_says_what_changed_and_nothing_otherwise() {
    assert!(Report::default().lines().is_empty());
    let r = Report {
        updated: vec!["Ruby 1.1.0".into(), "Glow 2.0".into()],
        held: vec!["held".into()],
        ..Report::default()
    };
    assert_eq!(
        r.lines(),
        ["Updated automatically: Ruby 1.1.0, Glow 2.0.", "held"]
    );
}
