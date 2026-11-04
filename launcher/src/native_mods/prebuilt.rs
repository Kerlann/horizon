//! The prebuilt game (#589): mods that need nothing but the mod API compile against a downloaded
//! build of the game (`tools/prebuilt_game.py`, released daily by `prebuilt-game.yml`) with rustc
//! alone, in seconds, instead of building the game from source. The route is chosen before
//! anything runs: a mod set the prebuilt game can't serve, or a revision with no prebuilt
//! release, builds from source, and the build log says why. A prebuilt build that fails is an
//! error like any other build's, never retried from source.
use super::{api, dependencies, inject, Profile, Target};
use crate::i18n::{tr, trf};
use crate::rust_toolchain::{self, Toolchain};
use horizon_mods::index::native;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

const IDENTITY: &str = "horizon-prebuilt.json";
const TEMPLATE: &str = "native_main.rs.template";
const SUMS: &str = "SHA256SUMS";
/// The `fast` profile the prebuilt game is built with: the mods' code matches the game's.
const OPT_LEVEL: &str = "opt-level=3";
/// The archive's largest plausible unpacked size: a corrupt or hostile archive stops there.
const UNPACKED_LIMIT: u64 = 8 << 30;

/// Why a build compiles the game from source instead of using the prebuilt game.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum SourceReason {
    /// A mod has a helper `[[native.injection]]` or a module: they change the game's own code.
    Injections { mod_id: String },
    /// A mod has a build script, which the prebuilt route doesn't run.
    BuildScript { mod_id: String },
    /// A mod declares Cargo features, or asks the mod API for features of its own.
    Features { mod_id: String },
    /// A mod depends on a crate other than the mod API and other mods.
    Crate { mod_id: String, krate: String },
    /// The player chose a profile other than the prebuilt game's `performance` (`fast`).
    Profile { profile: &'static str },
    /// No prebuilt game for this computer.
    Host,
    /// The prebuilt game's release couldn't be downloaded.
    Download { why: String },
    /// A point injection's target isn't a point in the prebuilt game.
    NotAPoint { why: String },
    /// No prebuilt game was released for this revision and target.
    NoRelease { revision: String },
}

impl std::fmt::Display for SourceReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            Self::Injections { mod_id } => trf(
                "{mod} injects into the game's code, so the game builds from source",
                &[("mod", mod_id)],
            ),
            Self::BuildScript { mod_id } => trf(
                "{mod} has a build script, so the game builds from source",
                &[("mod", mod_id)],
            ),
            Self::Features { mod_id } => trf(
                "{mod} uses Cargo features, so the game builds from source",
                &[("mod", mod_id)],
            ),
            Self::Crate { mod_id, krate } => trf(
                "{mod} depends on the crate {crate}, so the game builds from source",
                &[("mod", mod_id), ("crate", krate)],
            ),
            Self::Profile { profile } => trf(
                "the {profile} profile builds the game from source; the prebuilt game is performance only",
                &[("profile", profile)],
            ),
            Self::Host => {
                tr("no prebuilt game for this computer, so the game builds from source").into()
            }
            Self::Download { why } => trf(
                "the prebuilt game couldn't be downloaded ({why}), so the game builds from source",
                &[("why", why)],
            ),
            Self::NotAPoint { why } => trf(
                "{why}, so the game builds from source and makes it a point",
                &[("why", why)],
            ),
            Self::NoRelease { revision } => trf(
                "no prebuilt game for {revision}, so the game builds from source",
                &[("revision", &revision[..revision.len().min(10)])],
            ),
        };
        f.write_str(&text)
    }
}

/// The first reason `projects` can't build against the prebuilt game with `profile`, or `None`
/// when they can: the profile is `performance`, the one the game is prebuilt with, and each mod is
/// a crate that depends on the mod API (with its default features) and on other mods, with no
/// injections, build script or features of its own.
pub(super) fn refusal(projects: &[api::Project<'_>], profile: Profile) -> Option<SourceReason> {
    if profile != Profile::Performance {
        return Some(SourceReason::Profile {
            profile: profile.name(),
        });
    }
    projects.iter().find_map(|project| {
        let mod_id = || project.bundle.manifest.id.clone();
        if project.bundle.needs_mixins() {
            return Some(SourceReason::Injections { mod_id: mod_id() });
        }
        let cargo = &project.cargo;
        let package = cargo.get("package");
        let has_script = package.and_then(|p| p.get("build")).is_some()
            || project.bundle.dir.join("build.rs").is_file()
            || cargo
                .get("build-dependencies")
                .and_then(toml::Value::as_table)
                .is_some_and(|t| !t.is_empty());
        if has_script {
            return Some(SourceReason::BuildScript { mod_id: mod_id() });
        }
        if cargo
            .get("features")
            .and_then(toml::Value::as_table)
            .is_some_and(|t| !t.is_empty())
        {
            return Some(SourceReason::Features { mod_id: mod_id() });
        }
        for (_, dependencies) in project.dependency_tables() {
            for (key, value) in dependencies {
                let krate = value
                    .get("package")
                    .and_then(toml::Value::as_str)
                    .unwrap_or(key);
                if krate != api::API_PACKAGE {
                    return Some(SourceReason::Crate {
                        mod_id: mod_id(),
                        krate: krate.to_owned(),
                    });
                }
                let own_features = value
                    .get("features")
                    .and_then(toml::Value::as_array)
                    .is_some_and(|f| !f.is_empty())
                    || value.get("default-features").and_then(toml::Value::as_bool) == Some(false);
                if own_features {
                    return Some(SourceReason::Features { mod_id: mod_id() });
                }
            }
        }
        None
    })
}

/// Whether the prebuilt `game` has a point at each of `injections`' targets: a target that isn't
/// one builds from source, which makes it one. A method that isn't its point's is an error on
/// any route, naming the injection.
pub(super) fn serves_points(
    game: &Path,
    injections: &[inject::PointInjection],
) -> Result<Result<(), SourceReason>, String> {
    if injections.is_empty() {
        return Ok(Ok(()));
    }
    let points = inject::listed(&game.join(inject::POINTS))?;
    match inject::check(injections, &points) {
        Ok(()) => Ok(Ok(())),
        Err(error @ inject::PointError::NotAPoint { .. }) => Ok(Err(SourceReason::NotAPoint {
            why: error.to_string(),
        })),
        Err(error) => Err(error.to_string()),
    }
}

/// The prebuilt game for `revision` and `target`, downloaded and checked once, then kept in the
/// launcher's data (`downloading` is told before a download starts); `Err(reason)` inside when
/// there is none to use, or its download failed. An archive that arrives must match the release's
/// `SHA256SUMS` before it is unpacked, and its identity this revision, this computer and the
/// launcher's Rust: a mismatch is an error, never a source build.
pub(super) fn fetch(
    revision: &str,
    target: Target,
    downloading: impl FnOnce(),
) -> Result<Result<PathBuf, SourceReason>, String> {
    let Some(host) = rust_toolchain::host() else {
        return Ok(Err(SourceReason::Host));
    };
    let name = native::prebuilt_archive(host.target, matches!(target, Target::Server));
    let dir = super::root()
        .join("prebuilt")
        .join(format!("{revision}-{}", name.trim_end_matches(".tar.gz")));
    if dir.join(IDENTITY).is_file() {
        return Ok(Ok(dir));
    }
    let release = format!(
        "https://github.com/{}/releases/download/{}{revision}",
        native::PREBUILT_REPO,
        native::PREBUILT_TAG
    );
    // A download that fails builds from source, saying why; a file that arrives wrong is refused.
    let download = |why: String| Ok(Err(SourceReason::Download { why }));
    let sums = match crate::backend::get_unless_missing(&format!("{release}/{SUMS}")) {
        Ok(Some(sums)) => sums,
        Ok(None) => {
            return Ok(Err(SourceReason::NoRelease {
                revision: revision.to_owned(),
            }))
        }
        Err(why) => return download(why),
    };
    let mut text = String::new();
    if let Err(e) = sums.into_reader().read_to_string(&mut text) {
        return download(e.to_string());
    }
    let Some(expected) = expected_sum(&text, &name) else {
        return Ok(Err(SourceReason::NoRelease {
            revision: revision.to_owned(),
        }));
    };
    let parent = dir.parent().expect("under prebuilt/");
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let partial = parent.join(format!(".{name}.partial"));
    let _ = fs::remove_dir_all(&partial);
    fs::create_dir_all(&partial).map_err(|e| e.to_string())?;
    downloading();
    let file = partial.join(&name);
    let mut body = match crate::backend::get(&format!("{release}/{name}")) {
        Ok(response) => response.into_reader(),
        Err(why) => return download(why),
    };
    let mut out = fs::File::create(&file).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 1 << 20];
    loop {
        let read = match body.read(&mut buffer) {
            Ok(read) => read,
            Err(e) => {
                let _ = fs::remove_dir_all(&partial);
                return download(e.to_string());
            }
        };
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        std::io::Write::write_all(&mut out, &buffer[..read]).map_err(|e| e.to_string())?;
    }
    drop(out);
    let actual = format!("{:x}", hasher.finalize());
    if actual != expected {
        let _ = fs::remove_dir_all(&partial);
        return Err(trf(
            "The prebuilt game {name} doesn't match its release's checksum. Try again.",
            &[("name", &name)],
        ));
    }
    let unpacked = partial.join("game");
    let reader = flate2::read::GzDecoder::new(fs::File::open(&file).map_err(|e| e.to_string())?)
        .take(UNPACKED_LIMIT);
    let mut tar = tar::Archive::new(reader);
    for entry in tar.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        let kind = entry.header().entry_type();
        if kind.is_file() || kind.is_dir() {
            entry
                .unpack_in(&unpacked)
                .map_err(|e| format!("Could not unpack the prebuilt game: {e}"))?;
        }
    }
    check_identity(&unpacked, revision, host.target, target)?;
    let _ = fs::remove_dir_all(&dir);
    fs::rename(&unpacked, &dir).map_err(|e| e.to_string())?;
    let _ = fs::remove_dir_all(&partial);
    Ok(Ok(dir))
}

/// `name`'s sum in a `sha256sum` listing.
fn expected_sum(listing: &str, name: &str) -> Option<String> {
    listing.lines().find_map(|line| {
        let (sum, file) = line.split_once(char::is_whitespace)?;
        (file.trim_start().trim_start_matches('*') == name).then(|| sum.to_ascii_lowercase())
    })
}

#[derive(serde::Deserialize)]
struct Identity {
    commit: String,
    rustc: String,
    target: String,
    side: String,
    link_args: Vec<String>,
}

fn identity(dir: &Path) -> Result<Identity, String> {
    let path = dir.join(IDENTITY);
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Whether the unpacked game in `dir` is the one asked for: built from `revision` for `host` and
/// `target`'s side by the launcher's Rust, whose compiler is the only one that reads its metadata.
fn check_identity(dir: &Path, revision: &str, host: &str, target: Target) -> Result<(), String> {
    let found = identity(dir)?;
    let release = format!("release: {}", rust_toolchain::VERSION);
    let matches = found.commit == revision
        && found.target == host
        && found.side == target.name()
        && found.rustc.lines().any(|line| line == release);
    if matches {
        Ok(())
    } else {
        Err(trf(
            "The prebuilt game for {revision} was made for another build ({commit}, {target}, {side}, {rustc}).",
            &[
                ("revision", revision),
                ("commit", &found.commit),
                ("target", &found.target),
                ("side", &found.side),
                ("rustc", found.rustc.lines().next().unwrap_or_default()),
            ],
        ))
    }
}

/// The library file a crate named `krate` left in `deps`: its metadata, or the game's own dylib.
fn library(deps: &Path, krate: &str) -> Result<PathBuf, String> {
    let game = ["so", "dll", "dylib"]
        .iter()
        .flat_map(|ext| [format!("lib{krate}.{ext}"), format!("{krate}.{ext}")]);
    let prefix = format!("lib{krate}-");
    fs::read_dir(deps)
        .map_err(|e| e.to_string())?
        .flatten()
        .map(|entry| entry.path())
        .find(|path| {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            (name.starts_with(&prefix) && name.ends_with(".rmeta"))
                || game.clone().any(|g| g == name)
        })
        .ok_or_else(|| trf("The prebuilt game has no {crate}.", &[("crate", krate)]))
}

/// Rust's standard library as a dynamic library: what the prebuilt game and its executable load.
fn standard_library(rust: &Toolchain) -> Result<PathBuf, String> {
    let dir = rust
        .target_libdir()
        .ok_or("no Rust standard library for this computer")?;
    fs::read_dir(&dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .flatten()
        .map(|entry| entry.path())
        .find(|path| {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            (name.starts_with("libstd-") || name.starts_with("std-"))
                && [".so", ".dll", ".dylib"]
                    .iter()
                    .any(|ext| name.ends_with(ext))
        })
        .ok_or_else(|| format!("{}: no dynamic standard library", dir.display()))
}

/// rustc with the prebuilt game's settings, its libraries and `out`'s on the search path, and the
/// native libraries the game's build scripts link (`native/`: the `windows` crates' import
/// libraries).
fn rustc(rust: &Toolchain, game: &Path, out: &Path, edition: &str) -> Command {
    let mut cmd = rust.command(&rust.rustc());
    rust_toolchain::low_priority(&mut cmd);
    cmd.args([
        "--edition",
        edition,
        "-C",
        "prefer-dynamic",
        "-C",
        OPT_LEVEL,
    ])
    .arg("-L")
    .arg(format!("dependency={}", game.join("deps").display()))
    .arg("-L")
    .arg(format!("native={}", game.join("native").display()))
    .arg("-L")
    .arg(format!("dependency={}", out.display()));
    cmd
}

/// Compiles the mods (`resolution`'s order, each with its installed mod dependencies) and the
/// launcher's executable against the prebuilt game in `game`, into `out`. Returns the executable
/// and the dynamic libraries it loads, to be installed beside it.
pub(super) fn compile(
    rust: &Toolchain,
    game: &Path,
    out: &Path,
    projects: &[api::Project<'_>],
    declared: &[Vec<dependencies::Dependency>],
    resolution: &dependencies::Resolution,
) -> Result<(PathBuf, Vec<PathBuf>), String> {
    let _ = fs::remove_dir_all(out);
    fs::create_dir_all(out).map_err(|e| e.to_string())?;
    let deps = game.join("deps");
    let api_library = library(&deps, "mod_api")?;
    let mut rlibs: std::collections::HashMap<&str, PathBuf> = std::collections::HashMap::new();
    for &index in &resolution.order {
        let project = &projects[index];
        let id = project.bundle.manifest.id.as_str();
        let edition = project
            .cargo
            .get("package")
            .and_then(|p| p.get("edition"))
            .and_then(toml::Value::as_str)
            .unwrap_or("2015");
        let lib = project.cargo.get("lib");
        let krate = lib
            .and_then(|l| l.get("name"))
            .and_then(toml::Value::as_str)
            .map_or_else(|| project.package.replace('-', "_"), str::to_owned);
        let source = project.bundle.dir.join(
            lib.and_then(|l| l.get("path"))
                .and_then(toml::Value::as_str)
                .unwrap_or("src/lib.rs"),
        );
        let mut cmd = rustc(rust, game, out, edition);
        cmd.args(["--crate-type", "rlib", "--crate-name", &krate])
            .arg("-C")
            .arg(format!("metadata={id}"));
        for api in project.api_dependencies() {
            cmd.arg("--extern")
                .arg(format!("{api}={}", api_library.display()));
        }
        for dependency in &declared[index] {
            let Some(rlib) = rlibs.get(dependency.id.as_str()) else {
                continue;
            };
            cmd.arg("--extern").arg(format!(
                "{}={}",
                dependency.id.replace('-', "_"),
                rlib.display()
            ));
            if dependency.optional {
                cmd.arg("--cfg")
                    .arg(format!("feature=\"{}\"", dependency.id));
            }
        }
        cmd.arg("--out-dir").arg(out).arg(&source);
        super::run(cmd, &format!("Compiling {id}"), |_| {})?;
        rlibs.insert(id, out.join(format!("lib{krate}.rlib")));
    }
    let template = fs::read_to_string(game.join(TEMPLATE)).map_err(|e| e.to_string())?;
    let manifests: Vec<_> = resolution
        .order
        .iter()
        .map(|&i| &projects[i].bundle.manifest)
        .collect();
    let main = out.join("main.rs");
    fs::write(&main, api::main_rs(&template, &manifests)).map_err(|e| e.to_string())?;
    let game_library = library(&deps, "horizon_client")?;
    let binary = out.join(crate::backend::CLIENT);
    let mut cmd = rustc(rust, game, out, "2024");
    cmd.args(["--crate-type", "bin", "--crate-name", "horizon_native_game"])
        .arg("--extern")
        .arg(format!("horizon_client={}", game_library.display()))
        .arg("--extern")
        .arg(format!("mod_api={}", api_library.display()))
        .arg("--extern")
        .arg(format!(
            "horizon_mods={}",
            library(&deps, "horizon_mods")?.display()
        ));
    for (position, &index) in resolution.order.iter().enumerate() {
        let id = projects[index].bundle.manifest.id.as_str();
        cmd.arg("--extern")
            .arg(format!("mod_{position}={}", rlibs[id].display()));
    }
    // The game's libraries load from beside the executable.
    let origin = match std::env::consts::OS {
        "linux" => Some("-Wl,-rpath,$ORIGIN"),
        "macos" => Some("-Wl,-rpath,@executable_path"),
        _ => None,
    };
    for arg in origin
        .into_iter()
        .map(str::to_owned)
        .chain(identity(game)?.link_args)
    {
        cmd.arg("-C").arg(format!("link-arg={arg}"));
    }
    cmd.arg("-o").arg(&binary).arg(&main);
    super::run(cmd, "Linking the game", |_| {})?;
    Ok((binary, vec![game_library, standard_library(rust)?]))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What [`refusal`] says of one mod whose Cargo.toml is `cargo`.
    fn refusal_of(cargo: &str) -> Option<SourceReason> {
        let name = std::thread::current().name().unwrap().replace(':', "-");
        let dir = super::super::tests::dir(&name);
        super::super::tests::write_bundle(&dir, "example", "");
        fs::write(
            dir.join("Cargo.toml"),
            format!("[package]\nname='example-mod'\nversion='1.0.0'\nedition='2024'\n{cargo}\n"),
        )
        .unwrap();
        let bundle = super::super::bundle_in(&dir).unwrap().unwrap();
        refusal(
            &[api::Project::read(&bundle).unwrap()],
            Profile::Performance,
        )
    }

    #[test]
    fn only_the_performance_profile_takes_the_prebuilt_game() {
        for profile in [Profile::Dev, Profile::Release] {
            assert_eq!(
                refusal(&[], profile),
                Some(SourceReason::Profile {
                    profile: profile.name()
                })
            );
        }
        assert_eq!(refusal(&[], Profile::Performance), None);
    }

    #[test]
    fn mods_on_the_mod_api_alone_take_the_prebuilt_game() {
        assert_eq!(refusal_of("[dependencies]\nhorizon-mod-api='0.1'"), None);
        assert_eq!(
            refusal_of("[dependencies.api]\npackage='horizon-mod-api'\nversion='0.1'"),
            None
        );
    }

    #[test]
    fn anything_the_prebuilt_game_cant_build_names_why() {
        let id = || "example".to_owned();
        for (cargo, reason) in [
            (
                "[dependencies]\nhorizon-mod-api='0.1'\nserde='1'",
                SourceReason::Crate {
                    mod_id: id(),
                    krate: "serde".into(),
                },
            ),
            (
                "[dependencies]\nhorizon-mod-api='0.1'\n[target.'cfg(unix)'.dependencies]\nlibc='0.2'",
                SourceReason::Crate {
                    mod_id: id(),
                    krate: "libc".into(),
                },
            ),
            (
                "[dependencies]\nhorizon-mod-api={version='0.1',features=['testing']}",
                SourceReason::Features { mod_id: id() },
            ),
            (
                "[dependencies]\nhorizon-mod-api='0.1'\n[features]\nextra=[]",
                SourceReason::Features { mod_id: id() },
            ),
            (
                "build='make.rs'\n[dependencies]\nhorizon-mod-api='0.1'",
                SourceReason::BuildScript { mod_id: id() },
            ),
        ] {
            assert_eq!(refusal_of(cargo), Some(reason), "{cargo}");
        }
    }

    #[test]
    fn the_archive_is_found_by_name_in_the_release_sums() {
        let name = native::prebuilt_archive("x86_64-unknown-linux-gnu", true);
        assert_eq!(
            name,
            "horizon-prebuilt-v2-x86_64-unknown-linux-gnu-server.tar.gz"
        );
        let listing =
            format!("AB12  horizon-prebuilt-v2-x86_64-unknown-linux-gnu.tar.gz\ncd34 *{name}\n");
        assert_eq!(expected_sum(&listing, &name).as_deref(), Some("cd34"));
        assert_eq!(expected_sum(&listing, "horizon-prebuilt-x.tar.gz"), None);
    }
}
