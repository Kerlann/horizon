//! Real CLI regressions use separate launcher/game/config roots and never touch player data.
use std::{fs, path::PathBuf, process::Command};

struct Home(tempfile::TempDir);
impl Home {
    fn new() -> Self {
        Self(tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap())
    }
    fn file(&self, relative: &str, contents: &[u8]) -> PathBuf {
        let path = self.0.path().join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, contents).unwrap();
        path
    }
    fn run(&self, row: Option<&str>) -> std::process::Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_horizon-launcher"));
        command.arg("storage");
        if let Some(row) = row {
            command.args(["--clear", row]);
        }
        command
            .current_dir(self.0.path())
            .env("HORIZON_HOME", self.0.path().join("game"))
            .env("HORIZON_LAUNCHER_HOME", self.0.path().join("launcher"))
            .env("XDG_DATA_HOME", self.0.path().join("data"))
            .env("XDG_CONFIG_HOME", self.0.path().join("config"))
            .env("HORIZON_LAUNCHER_TRASH", self.0.path().join("trash"))
            .env(
                "HORIZON_LAUNCHER_CLIPBOARD",
                self.0.path().join("clipboard"),
            )
            .env("HORIZON_LAUNCHER_DESKTOP", self.0.path().join("desktop"))
            .env("HORIZON_LAUNCHER_NO_SELF_UPDATE", "1")
            .env("HORIZON_NO_AUDIO", "1");
        #[cfg(target_os = "linux")]
        unsafe {
            use std::os::unix::process::CommandExt;
            command.pre_exec(|| {
                let limit = libc::rlimit {
                    rlim_cur: 256,
                    rlim_max: 256,
                };
                if libc::setrlimit(libc::RLIMIT_NOFILE, &limit) == 0 {
                    Ok(())
                } else {
                    Err(std::io::Error::last_os_error())
                }
            });
        }
        command.output().unwrap()
    }
    fn clear(&self, row: &str) -> serde_json::Value {
        let output = self.run(Some(row));
        assert!(output.status.success(), "{output:?}");
        serde_json::from_slice(&output.stdout).unwrap()
    }
}

#[test]
fn build_files_clear_selects_target_not_worlds_or_prebuilt_games() {
    let home = Home::new();
    let world = home.file("launcher/worlds/precious/level.dat", b"world");
    let game_world = home.file("game/worlds/precious/level.dat", b"world");
    let output = home.file("launcher/native/target/fast/output", &[7; 8192]);
    let source = home.file("launcher/native/source/rev/lib.rs", b"source");
    let prebuilt = home.file("launcher/native/prebuilt/rev/game", b"prebuilt");
    let report = home.clear("build-files");
    assert_eq!(fs::read(prebuilt).unwrap(), b"prebuilt");
    assert!(!output.exists());
    assert!(!source.exists());
    let row = report["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["category"] == "build-files")
        .unwrap();
    assert!(row["freed_bytes"].as_u64().unwrap() > 0);
    home.clear("all");
    assert_eq!(fs::read(world).unwrap(), b"world");
    assert_eq!(fs::read(game_world).unwrap(), b"world");
}

#[cfg(unix)]
#[test]
fn running_unplayed_version_is_excluded_until_its_process_exits() {
    use std::process::Stdio;
    let home = Home::new();
    home.file("launcher/versions/v0.0.1/.installed", b"");
    home.file("launcher/settings.json", br#"{"selected":"v0.0.1"}"#);
    home.file("launcher/versions/native-played/.installed", b"");
    let played = home.file(
        "launcher/versions/native-played/game",
        b"instance-pinned native game",
    );
    home.file(
        "launcher/instances.json",
        br#"{"instances":{"horizon":{"version":"native-played"}}}"#,
    );
    home.file("launcher/versions/native-live/.installed", b"");
    let game = home.file("launcher/versions/native-live/game", b"running");
    // ps sees the version path while read waits on the retained pipe. No game data is needed.
    let mut process = Command::new("sh")
        .args(["-c", "read -r ignored", "storage-fixture"])
        .arg(&game)
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    home.clear("spare-game-builds");
    assert!(game.exists());
    assert!(
        played.exists(),
        "instance-pinned native version was selected"
    );
    process.kill().unwrap();
    process.wait().unwrap();
    home.clear("spare-game-builds");
    assert!(!game.exists());
    home.clear("all");
    assert!(
        played.exists(),
        "Clear all selected an instance-pinned native version"
    );
}

impl Home {
    fn installed_version(&self) {
        self.file("launcher/versions/v0.0.1/.installed", b"");
        self.file("launcher/settings.json", br#"{"selected":"v0.0.1"}"#);
    }
}

#[cfg(unix)]
#[test]
fn instance_caches_are_preserved_while_running_and_cleared_when_idle() {
    use std::process::Stdio;
    let home = Home::new();
    home.installed_version();
    let game = home.file("launcher/versions/v0.0.1/game", b"running");
    let cache = home.file("game/.horizon-cache/held-output", b"active instance cache");
    let mut process = Command::new("sh")
        .args(["-c", "read -r ignored", "storage-instance"])
        .arg(game)
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    home.clear("other-caches");
    assert_eq!(
        fs::read(&cache).unwrap(),
        b"active instance cache",
        "running instance cache was selected"
    );
    process.kill().unwrap();
    process.wait().unwrap();
    home.clear("other-caches");
    assert!(!cache.exists(), "idle instance cache was not selected");
}

#[test]
fn unresolved_instance_caches_are_never_selected() {
    let home = Home::new();
    home.file(
        "launcher/settings.json",
        br#"{"selected":"missing-version"}"#,
    );
    let cache = home.file("game/wasm-cache/precious", b"unresolved instance cache");
    home.clear("other-caches");
    assert_eq!(
        fs::read(cache).unwrap(),
        b"unresolved instance cache",
        "an unresolved instance cannot prove it is idle"
    );
}

#[test]
fn retention_removes_only_reports_older_than_fourteen_days_and_keeps_the_newest() {
    use std::time::{Duration, SystemTime};
    let home = Home::new();
    home.installed_version();
    for (name, half_days) in [
        ("logs/old.log", 29),
        ("logs/recent.log", 27),
        ("logs/latest.log", 0),
        ("crash-reports/old.txt", 40),
        ("crash-reports/newest.txt", 32),
    ] {
        let path = home.file(&format!("game/{name}"), b"report");
        let modified = SystemTime::now() - Duration::from_secs(half_days * 43200);
        fs::OpenOptions::new()
            .write(true)
            .open(path)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(modified))
            .unwrap();
    }
    home.clear("other-caches");
    assert!(
        !home.0.path().join("game/logs/old.log").exists(),
        "14.5-day log was not cleared"
    );
    assert!(
        home.0.path().join("game/logs/recent.log").exists(),
        "13.5-day log was cleared"
    );
    assert!(
        home.0.path().join("game/logs/latest.log").exists(),
        "latest log was cleared"
    );
    assert!(
        !home.0.path().join("game/crash-reports/old.txt").exists(),
        "old crash report was not cleared"
    );
    assert!(
        home.0.path().join("game/crash-reports/newest.txt").exists(),
        "newest crash report must survive even after 14 days"
    );
}

#[test]
fn all_clears_other_caches_but_never_worlds_or_mod_packages() {
    let home = Home::new();
    home.installed_version();
    let cache = home.file("game/.horizon-cache/output", b"cache");
    let world = home.file("game/worlds/precious/level.dat", b"world");
    let package = home.file("game/horizon-mods/precious.hmod", b"mod package");
    home.clear("all");
    assert!(!cache.exists(), "Clear all skipped Other caches");
    assert_eq!(
        fs::read(world).unwrap(),
        b"world",
        "worlds selected as instance cache"
    );
    assert_eq!(
        fs::read(package).unwrap(),
        b"mod package",
        "installed packages selected as instance cache"
    );
}

#[cfg(unix)]
#[test]
fn symlink_named_instance_is_refused_without_touching_its_outside_target() {
    let home = Home::new();
    home.installed_version();
    let outside = Home::new();
    let precious = outside.file(".horizon-cache/decoy", b"outside cache");
    fs::create_dir_all(home.0.path().join("game/instances")).unwrap();
    std::os::unix::fs::symlink(
        outside.0.path(),
        home.0.path().join("game/instances/linked"),
    )
    .unwrap();
    let output = home.run(Some("other-caches"));
    assert_eq!(
        fs::read(precious).unwrap(),
        b"outside cache",
        "outside instance target was touched"
    );
    assert!(
        !output.status.success(),
        "symlink-named instance was accepted"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn hundreds_of_reports_and_jars_fit_the_descriptor_budget_and_clear_repeatedly() {
    let home = Home::new();
    home.installed_version();
    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(15 * 86400);
    for index in 0..400 {
        let path = home.file(&format!("game/logs/{index}.log"), b"old log");
        fs::OpenOptions::new()
            .write(true)
            .open(path)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(old))
            .unwrap();
        home.file(&format!("game/jars/{index}.jar"), b"unused jar");
    }
    let report = home.run(None);
    assert!(
        report.status.success(),
        "read-only report exceeded 256 FDs: {report:?}"
    );
    home.clear("other-caches");
    home.clear("other-caches");
    assert!(!home.0.path().join("game/jars/399.jar").exists());
    assert_eq!(
        fs::read_dir(home.0.path().join("game/logs"))
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "log"))
            .count(),
        1
    );
}
