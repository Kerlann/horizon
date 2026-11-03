//! `cargo horizon-mod`: make Horizon content mods (`docs/modpacks.md`): blocks,
//! items, data and assets, no code. Code mods are native (docs/native-mods.md).
//!
//! - `new <path> [--name "Name"]`: a content mod from the template; with
//!   `--template native [--horizon <commit>]`, a native trait mod's bundle.
//! - `build`: the `.hmod` in `target/horizon-mod/`, with the mod's `assets/`
//!   and `data/`.
//! - `run [--game <horizon-client>] [-- <game args>]`: build, then play with
//!   the mod.
//! - `check <file> [--json]`: what the registry checks of a submission
//!   (`.hmod`, `.hpack`, resource pack `.zip`, `.hshader`).
//! - `registry build|review|check|list`: the registry repository's CI.
use horizon_mods::{Content, Manifest, Package, manifest::MANIFEST};
mod check;
mod registry;

use std::{
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};

fn main() -> ExitCode {
    // Cargo runs `cargo-horizon-mod horizon-mod <args>`.
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "horizon-mod") {
        args.remove(0);
    }
    let result = match args.first().map(String::as_str) {
        Some("new") => new(&args[1..]),
        Some("build") => build().map(|p| {
            println!("Built {}", p.display());
        }),
        Some("run") => run(&args[1..]),
        Some("check") => check(&args[1..]),
        Some("registry") => registry::main(&args[1..]),
        _ => Err("usage: cargo horizon-mod new <path> [--name \"Name\"] [--template content|native [--horizon <commit>]] | build | run [--game <horizon-client>] [-- <game args>] | check <file> [--kind <kind>] [--key <public key> --signature <file.minisig>] [--json] | registry build|review|check|list".into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// The value after `flag` in `args`.
fn flag<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    let i = args.iter().position(|a| a == flag)?;
    args.get(i + 1).map(String::as_str)
}

fn new(args: &[String]) -> Result<(), String> {
    let path = args
        .first()
        .filter(|a| !a.starts_with("--"))
        .ok_or("cargo horizon-mod new <path>")?;
    let dir = Path::new(path);
    if dir.exists() {
        return Err(format!("{} already exists", dir.display()));
    }
    let base = dir
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("the path needs a folder name")?;
    let id: String = base
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    let name = flag(args, "--name").map_or_else(|| base.replace(['-', '_'], " "), str::to_string);
    let fill = |t: &str| t.replace("{id}", &id).replace("{name}", &name);
    match flag(args, "--template") {
        None | Some("content") => {}
        Some("native") => return new_native(dir, &id, flag(args, "--horizon"), fill),
        Some(_) => return Err("--template is content or native".into()),
    }
    // A block copying a vanilla one, with its model, name, loot, recipe and tag.
    let files = [
        (
            MANIFEST,
            include_str!("../template/content/horizon-mod.toml"),
        ),
        (
            "data/{id}/horizon/block/glowing_amethyst.json",
            include_str!("../template/content/block.json"),
        ),
        (
            "assets/{id}/blockstates/glowing_amethyst.json",
            include_str!("../template/content/blockstate.json"),
        ),
        (
            "assets/{id}/models/block/glowing_amethyst.json",
            include_str!("../template/content/model.json"),
        ),
        (
            "assets/{id}/items/glowing_amethyst.json",
            include_str!("../template/content/item.json"),
        ),
        (
            "assets/{id}/lang/en_us.json",
            include_str!("../template/content/lang.json"),
        ),
        (
            "data/{id}/loot_table/blocks/glowing_amethyst.json",
            include_str!("../template/content/loot.json"),
        ),
        (
            "data/{id}/recipe/glowing_amethyst.json",
            include_str!("../template/content/recipe.json"),
        ),
        (
            "data/minecraft/tags/block/mineable/pickaxe.json",
            include_str!("../template/content/pickaxe.json"),
        ),
        (".gitignore", include_str!("../template/gitignore")),
    ];
    for (file, text) in files {
        let file = fill(file);
        let file = file.as_str();
        let to = dir.join(file);
        std::fs::create_dir_all(to.parent().expect("a file in the mod"))
            .map_err(|e| e.to_string())?;
        std::fs::write(&to, fill(text)).map_err(|e| format!("{}: {e}", to.display()))?;
    }
    let manifest =
        Manifest::parse(&std::fs::read_to_string(dir.join(MANIFEST)).map_err(|e| e.to_string())?)?;
    Content::read(dir, &manifest)?;
    println!(
        "Created {} (mod id {id}). Next: cd {path} && cargo horizon-mod run",
        dir.display()
    );
    Ok(())
}

/// A native trait mod's bundle in `dir` (docs/native-mods.md, "Trait mods"): its
/// `horizon-mixin.toml` and an `[api]` crate with one hook. It is what the launcher
/// builds (`native-build <dir>`) and packs as `native/` of a `.hmod`
/// (docs/api/mod-files.md). `horizon` pins the Horizon commit it is made for.
fn new_native(
    dir: &Path,
    id: &str,
    horizon: Option<&str>,
    fill: impl Fn(&str) -> String,
) -> Result<(), String> {
    // `[mod] id`, as `horizon-native.json` needs it: the template keeps to its characters.
    if id.len() > 64 {
        return Err(format!("mod id {id}: 64 characters at most"));
    }
    let horizon = horizon.ok_or("native projects require --horizon <40 hex digits>")?;
    if !horizon_mods::index::native::revision(horizon) {
        return Err("--horizon: a Horizon commit is 40 lowercase hex digits".into());
    }
    let compatibility = format!("[compatibility]\nhorizon_revision = \"{horizon}\"\n");
    let rev = format!(", rev = \"{horizon}\"");
    let files = [
        (
            "native/horizon-mixin.toml",
            include_str!("../template/native/horizon-mixin.toml"),
        ),
        (
            "native/rust/Cargo.toml",
            include_str!("../template/native/api.toml"),
        ),
        (
            "native/rust/src/lib.rs",
            include_str!("../template/native/lib.rs"),
        ),
        (
            MANIFEST,
            include_str!("../template/content/horizon-mod.toml"),
        ),
        (".gitignore", include_str!("../template/gitignore")),
    ];
    for (file, text) in files {
        let to = dir.join(file);
        std::fs::create_dir_all(to.parent().expect("a file in the mod"))
            .map_err(|e| e.to_string())?;
        let text = fill(text)
            .replace("{compatibility}", &compatibility)
            .replace("{rev}", &rev);
        std::fs::write(&to, text).map_err(|e| format!("{}: {e}", to.display()))?;
    }
    let manifest = horizon_mods::index::native::Manifest {
        schema: 1,
        id: id.into(),
        version: "0.1.0".into(),
        game: "26.3".into(),
        horizon: horizon.into(),
        mixins: horizon_mods::index::native::MIXINS_REV.into(),
    };
    std::fs::write(
        dir.join(horizon_mods::index::native::MANIFEST),
        serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    for folder in ["assets", "data", "native/data"] {
        std::fs::create_dir_all(dir.join(folder)).map_err(|e| e.to_string())?;
    }
    println!(
        "Created {} (native mod id {id}). Next: horizon-launcher native-build {} --instance <instance> --trust-native-code",
        dir.display(),
        dir.display()
    );
    Ok(())
}

/// Packs the content mod in the current folder into `target/horizon-mod/<id>.hmod`: the
/// manifest and the files in `assets/` and `data/`.
fn build() -> Result<PathBuf, String> {
    build_in(Path::new("."))
}

fn build_in(root: &Path) -> Result<PathBuf, String> {
    if root.join(horizon_mods::index::native::MANIFEST).is_file() {
        let text = std::fs::read(root.join(horizon_mods::index::native::MANIFEST))
            .map_err(|e| e.to_string())?;
        let manifest: horizon_mods::index::native::Manifest =
            serde_json::from_slice(&text).map_err(|e| e.to_string())?;
        let bytes = horizon_mods::index::native::Bundle::pack_directory(root, &manifest)?;
        let path = root
            .join("target/horizon-mod")
            .join(format!("{}.hmod", manifest.id));
        std::fs::create_dir_all(path.parent().expect("package directory"))
            .map_err(|e| e.to_string())?;
        let partial = path.with_extension("hmod.partial");
        std::fs::write(&partial, bytes).map_err(|e| e.to_string())?;
        std::fs::rename(partial, &path).map_err(|e| e.to_string())?;
        return Ok(path);
    }
    let manifest_text = std::fs::read_to_string(root.join(MANIFEST))
        .map_err(|e| format!("{MANIFEST}: {e} (run this in a mod's folder)"))?;
    let manifest = Manifest::parse(&manifest_text)?;
    if root.join("Cargo.toml").exists() {
        return Err("a Cargo.toml: code mods are native now (docs/native-mods.md)".into());
    }
    // Content owns the packaged path list; the writer checks exactly those entries.
    let content = Content::read(root, &manifest)?;
    let mut reader = content.reader()?;
    let mut files = Vec::with_capacity(content.files.len());
    for name in &content.files {
        files.push((name.as_str(), reader.read(name)?));
    }
    let files: Vec<(&str, &[u8])> = files.iter().map(|(n, b)| (*n, b.as_slice())).collect();
    let dir = root.join("target/horizon-mod");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("{}.hmod", manifest.id));
    // Write then rename, so a running game never reads half a file.
    let partial = path.with_extension("hmod.partial");
    let bytes = Package::write_files(&manifest_text, &files)?;
    std::fs::write(&partial, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&partial, &path).map_err(|e| e.to_string())?;
    Ok(path)
}

/// `check <file>`: the registry's checks of a submission, printed; fails
/// when one does.
fn check(args: &[String]) -> Result<(), String> {
    let file = args
        .first()
        .filter(|a| !a.starts_with("--"))
        .ok_or("cargo horizon-mod check <file>")?;
    let path = Path::new(file);
    let kind = match flag(args, "--kind") {
        Some(k) => horizon_mods::index::Kind::from(k.to_string()),
        None => check::kind_of(path)
            .ok_or("pass --kind mod|modpack|resourcepack: the extension doesn't say")?,
    };
    let signature = flag(args, "--signature")
        .map(|p| std::fs::read_to_string(p).map_err(|e| format!("{p}: {e}")))
        .transpose()?;
    let report = check::check(path, kind, flag(args, "--key"), signature.as_deref());
    if args.iter().any(|a| a == "--json") {
        let json = match &report {
            Ok(r) => serde_json::json!({"ok": true, "report": r.json()}),
            Err(e) => serde_json::json!({"ok": false, "error": e}),
        };
        println!("{json}");
    }
    let report = report?;
    if !args.iter().any(|a| a == "--json") {
        print!("{}", report.text());
    }
    Ok(())
}

fn run(args: &[String]) -> Result<(), String> {
    let path = build()?;
    let game = flag(args, "--game")
        .map(str::to_string)
        .or_else(|| std::env::var("HORIZON_CLIENT").ok())
        .unwrap_or_else(|| "horizon-client".into());
    let extra = args.iter().skip_while(|a| *a != "--").skip(1);
    println!("Starting {game} with {}", path.display());
    let status = Command::new(&game)
        .arg("--dev-mod")
        .arg(&path)
        .args(extra)
        .status()
        .map_err(|e| {
            format!("{game}: {e} (pass --game <path to horizon-client> or set HORIZON_CLIENT)")
        })?;
    status
        .success()
        .then_some(())
        .ok_or_else(|| format!("the game exited with {status}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_content_template_builds_and_checks_with_no_code() {
        let dir = std::env::temp_dir().join(format!("horizon-mod-content-{}", std::process::id()));
        let path = dir.join("gems");
        let new_args = [
            path.display().to_string(),
            "--template".into(),
            "content".into(),
        ];
        new(&new_args).unwrap();
        assert!(!path.join("Cargo.toml").exists());
        let hmod = build_in(&path).unwrap();
        assert_eq!(hmod, path.join("target/horizon-mod/gems.hmod"));
        let package = Package::read(&hmod).unwrap();
        let content = Content::read(&hmod, &package.manifest).unwrap();
        assert!(
            content
                .files
                .contains(&"data/gems/horizon/block/glowing_amethyst.json".into())
        );
        assert_eq!(content.files.len(), 8, "{:?}", content.files);
        let report = check::check(&hmod, check::kind_of(&hmod).unwrap(), None, None).unwrap();
        assert!(
            report.text().contains("no code: content only"),
            "{}",
            report.text()
        );
        let executable = path.join("assets/gems/mod.wasm");
        std::fs::write(&executable, b"\0asm").unwrap();
        assert!(
            build_in(&path)
                .unwrap_err()
                .contains("unsupported executable file")
        );
        std::fs::remove_file(executable).unwrap();
        // Code is native now.
        std::fs::write(path.join("Cargo.toml"), "").unwrap();
        assert!(build_in(&path).unwrap_err().contains("native"));
        assert!(
            new(&[
                dir.join("x").display().to_string(),
                "--template".into(),
                "lua".into()
            ])
            .is_err()
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn native_build_and_check_share_the_content_executable_boundary() {
        use horizon_mods::manifest::ContentFileError;
        use std::io::Write;

        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target")
            .join(format!("native-content-contract-{}", std::process::id()));
        let path = dir.join("mixed");
        let _ = std::fs::remove_dir_all(&dir);
        new(&[
            path.display().to_string(),
            "--template".into(),
            "native".into(),
            "--horizon".into(),
            "0123456789abcdef0123456789abcdef01234567".into(),
        ])
        .unwrap();
        let asset = "assets/mixed/program.bin";
        std::fs::create_dir_all(path.join("assets/mixed")).unwrap();
        std::fs::write(path.join(asset), b"ordinary asset").unwrap();
        // Only selected content ships, not the Git project's executable hooks.
        std::fs::create_dir_all(path.join(".git/hooks")).unwrap();
        std::fs::write(path.join(".git/hooks/pre-commit"), b"#!/bin/sh\nexit 0\n").unwrap();
        let hmod = build_in(&path).unwrap();
        let kind = check::kind_of(&hmod).unwrap();
        assert!(check::check(&hmod, kind.clone(), None, None).is_ok());

        // Model an externally supplied archive with executable bytes under an asset name.
        let original = std::fs::read(&hmod).unwrap();
        let mut input = zip::ZipArchive::new(std::io::Cursor::new(original)).unwrap();
        let mut output = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default().unix_permissions(0o644);
        for i in 0..input.len() {
            let mut entry = input.by_index(i).unwrap();
            let name = entry.name().unwrap().into_owned();
            output.start_file(&name, options).unwrap();
            if name == asset {
                output.write_all(b"MZ-not-an-asset").unwrap();
            } else {
                std::io::copy(&mut entry, &mut output).unwrap();
            }
        }
        std::fs::write(&hmod, output.finish().unwrap().into_inner()).unwrap();
        let expected = ContentFileError::Executable(asset.into()).to_string();
        assert_eq!(
            check::check(&hmod, kind, None, None).err().unwrap(),
            expected
        );
        std::fs::remove_file(&hmod).unwrap();
        std::fs::write(path.join(asset), b"MZ-not-an-asset").unwrap();
        assert_eq!(build_in(&path).unwrap_err(), expected);
        assert!(!hmod.exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_native_template_packs_into_an_hmod_the_registry_accepts() {
        let dir = std::env::temp_dir().join(format!("horizon-mod-native-{}", std::process::id()));
        let path = dir.join("machine-mod");
        let commit = "0123456789abcdef0123456789abcdef01234567";
        let args = [
            path.display().to_string(),
            "--template".into(),
            "native".into(),
            "--horizon".into(),
            commit.into(),
        ];
        new(&args).unwrap();
        let api = std::fs::read_to_string(path.join("native/rust/Cargo.toml")).unwrap();
        assert!(api.contains(&format!("rev = \"{commit}\"")), "{api}");
        let hmod = build_in(&path).unwrap();
        let bundle = horizon_mods::index::native::Bundle::read(&hmod).unwrap();
        assert_eq!(bundle.manifest.id, "machine_mod");
        let names: Vec<_> = bundle.files.keys().map(String::as_str).collect();
        assert_eq!(
            names,
            [
                "horizon-native.json",
                "native/horizon-mixin.toml",
                "native/rust/Cargo.toml",
                "native/rust/src/lib.rs"
            ]
        );
        assert!(path.join("horizon-mod.toml").is_file());
        for old in ["api", "mixins", "content", "horizon-mixin.toml"] {
            assert!(!path.join(old).exists());
        }
        assert_eq!(
            check::kind_of(&hmod),
            Some(horizon_mods::index::Kind::NativeMod)
        );
        assert_eq!(check::kind_of(&dir.join("machine_mod.hnmod")), None);
        let report = check::check(&hmod, check::kind_of(&hmod).unwrap(), None, None).unwrap();
        assert!(report.text().contains("machine_mod"), "{}", report.text());
        assert!(
            new(&[
                dir.join("bad").display().to_string(),
                "--template".into(),
                "native".into(),
                "--horizon".into(),
                "main".into(),
            ])
            .unwrap_err()
            .contains("40 lowercase hex")
        );
        assert!(
            new(&[
                dir.join("free").display().to_string(),
                "--template".into(),
                "native".into(),
            ])
            .unwrap_err()
            .contains("--horizon")
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
