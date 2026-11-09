use super::*;
use std::io::{BufRead, Write};
use std::process::{Child, Stdio};

fn directory(path: &Path) -> Dir {
    fs::create_dir_all(path).unwrap();
    Dir::open_ambient_dir(path, cap_std::ambient_authority()).unwrap()
}

struct Actor(Child);
impl Drop for Actor {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn killed_holder_releases_production_home_source_and_parent_leases() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let game = root.path().join("game");
    let source = root.path().join("versions/native-fixture");
    let data = root.path().join("data");
    let resources = root.path().join("base/resources");
    fs::create_dir_all(&resources).unwrap();
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&game).unwrap();
    fs::create_dir_all(&source).unwrap();
    #[cfg(windows)]
    fs::copy(
        std::env::current_exe().unwrap(),
        source.join(crate::backend::CLIENT),
    )
    .unwrap();
    let mut actor = Actor(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "run_leases::tests::lease_holder_actor",
                "--nocapture",
            ])
            .env("HORIZON_LEASE_ACTOR_ROOT", root.path())
            .env("HORIZON_LAUNCHER_HOME", &data)
            .env("HORIZON_HOME", &home)
            .env("XDG_DATA_HOME", root.path().join("xdg-data"))
            .env("XDG_CONFIG_HOME", root.path().join("xdg-config"))
            .env("HORIZON_LAUNCHER_TRASH", root.path().join("trash"))
            .env("HORIZON_LAUNCHER_CLIPBOARD", root.path().join("clipboard"))
            .env("HORIZON_LAUNCHER_DESKTOP", root.path().join("desktop"))
            .env("HORIZON_LAUNCHER_NO_SELF_UPDATE", "1")
            .env("HORIZON_NO_AUDIO", "1")
            .current_dir(root.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let output = actor.0.stdout.take().unwrap();
    let ready = std::io::BufReader::new(output)
        .lines()
        .map_while(Result::ok)
        .any(|line| line.contains("LEASE-READY"));
    assert!(ready, "holder did not acquire its production runtime guard");
    let registry = Registry::at(&data).unwrap();
    for path in [&home, &game, &source, source.parent().unwrap(), &resources] {
        assert!(
            matches!(
                registry.lock(&directory(path), LockMode::Exclusive),
                Err(Error::Busy)
            ),
            "active tree was not protected: {}",
            path.display()
        );
    }
    actor.0.kill().unwrap(); // SIGKILL on Unix; no Drop/marker cleanup in the holder.
    actor.0.wait().unwrap();
    for path in [&home, &game, &source, source.parent().unwrap(), &resources] {
        drop(
            registry
                .lock(&directory(path), LockMode::Exclusive)
                .unwrap(),
        );
    }
}

#[test]
fn lease_holder_actor() {
    let Some(root) = std::env::var_os("HORIZON_LEASE_ACTOR_ROOT") else {
        return;
    };
    let root = std::path::PathBuf::from(root);
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .current_dir(root.join("game"))
        .env("HORIZON_HOME", root.join("home"))
        .arg("--resources")
        .arg(root.join("base/resources"));
    mark(&mut command, &root.join("versions/native-fixture"));
    let (_command, _guard) = crate::backend::prepare_command(command).unwrap();
    println!("LEASE-READY");
    std::io::stdout().flush().unwrap();
    let mut byte = String::new();
    std::io::stdin().read_line(&mut byte).unwrap();
}
