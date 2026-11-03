//! Source-only native mixin bundles. Reading a bundle never executes its code.
use super::{Facts, valid_id};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, io::Read, path::Path};

pub const MANIFEST: &str = "horizon-native.json";
pub const SOURCE: &str = "native/";
pub const SOURCE_MANIFEST: &str = "horizon-mixin.toml";
pub const RUST: &str = "rust";
pub const MIXINS_REV: &str = "e8cd0aa38cffef49cb2e088ef716baa107148c02";

#[derive(Debug, PartialEq, Eq)]
pub enum SourceLayoutError {
    Root(String),
    Path,
    ApiFields,
    BothRoutes,
    MultipleCrates,
}

impl std::fmt::Display for SourceLayoutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Root(path) => write!(f, "native source root must be rust, got {path:?}"),
            Self::Path => f.write_str("native manifest needs a string source path"),
            Self::ApiFields => f.write_str("[api] accepts only path"),
            Self::BothRoutes => {
                f.write_str("native manifest cannot declare both [api] and [[crate]] for rust")
            }
            Self::MultipleCrates => f.write_str("native manifest may declare only one rust crate"),
        }
    }
}

impl std::error::Error for SourceLayoutError {}

/// The launcher and archive reader share the source-root boundary.
pub fn check_source_layout(source: &toml::Table) -> Result<(), SourceLayoutError> {
    let api = source.get("api");
    if api.is_some_and(|v| {
        !v.as_table()
            .is_some_and(|t| t.len() == 1 && t.contains_key("path"))
    }) {
        return Err(SourceLayoutError::ApiFields);
    }
    let crates = source
        .get("crate")
        .map(|v| v.as_array().ok_or(SourceLayoutError::Path))
        .transpose()?;
    if api.is_some() && crates.is_some_and(|v| !v.is_empty()) {
        return Err(SourceLayoutError::BothRoutes);
    }
    if crates.is_some_and(|v| v.len() > 1) {
        return Err(SourceLayoutError::MultipleCrates);
    }
    for declaration in api.into_iter().chain(crates.into_iter().flatten()) {
        let path = declaration
            .get("path")
            .and_then(toml::Value::as_str)
            .ok_or(SourceLayoutError::Path)?;
        if path != RUST {
            return Err(SourceLayoutError::Root(path.into()));
        }
    }
    Ok(())
}

/// Why a native mod may not carry `mod.wasm`: WebAssembly mods don't run any more.
pub const WASM_GONE: &str = "mod.wasm: WebAssembly mods don't run any more, port the code to a native mod (docs/native-mods.md)";

/// Whether `path` names a mod package.
pub fn is_file(path: &Path) -> bool {
    path.extension().is_some_and(|e| e == "hmod")
}

/// Whether the mod file at `path` carries native code ([`MANIFEST`]).
pub fn holds(path: &Path) -> bool {
    std::fs::File::open(path)
        .ok()
        .and_then(|f| zip::ZipArchive::new(f).ok())
        .is_some_and(|zip| zip.index_for_name(MANIFEST).is_some())
}

/// Whether a `.hmod`'s entry `name` is its content's (`horizon-mod.toml`, `assets/`, `data/`),
/// which [`crate::Content`] reads and checks, not the native build.
fn content(name: &str) -> bool {
    name == crate::manifest::MANIFEST || name.starts_with("assets/") || name.starts_with("data/")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema: u32,
    pub id: String,
    pub version: String,
    pub game: String,
    /// Exact source revisions tested together, never mutable tags.
    pub horizon: String,
    pub mixins: String,
}

pub fn revision(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl Manifest {
    pub fn check(&self) -> Result<(), String> {
        if self.schema != 1
            || !valid_id(&self.id)
            || self.version.is_empty()
            || self.game.is_empty()
            || !revision(&self.horizon)
            || !revision(&self.mixins)
        {
            return Err(
                "invalid native manifest: schema 1, id, version, game and exact revisions required"
                    .into(),
            );
        }
        Ok(())
    }

    /// Carries the exact native compatibility contract in the index's API field.
    pub fn api(&self) -> String {
        format!("native-1/{}/{}", self.horizon, self.mixins)
    }

    pub fn facts(&self) -> Facts {
        Facts {
            id: Some(self.id.clone()),
            version: Some(self.version.clone()),
            api: self.api(),
            game: Some(self.game.clone()),
            ..Facts::default()
        }
    }
}

pub struct Bundle {
    pub manifest: Manifest,
    /// Validated relative paths: Rust and TOML, plus files under `native/data/` or a
    /// `data/` folder deeper in (a crate's). A `.hmod`'s content files are not among them.
    pub files: BTreeMap<String, Vec<u8>>,
}

/// Whether `rest` is one of the text files a crate folder usually carries, which a crate may need
/// to compile (`#![doc = include_str!("../README.md")]`): `README*`, `LICENSE*`, `COPYING*`,
/// `NOTICE*` and `.gitignore`, in the bundle's folder or a top-level folder of it.
fn text(rest: &str) -> bool {
    let parts: Vec<&str> = rest.split('/').collect();
    let name = parts[parts.len() - 1];
    parts.len() <= 2
        && (["README", "LICENSE", "LICENCE", "COPYING", "NOTICE"]
            .iter()
            .any(|p| name.starts_with(p))
            || name == ".gitignore")
}

/// Whether a bundle may carry the file `rest` (its path under `native/`): sources and
/// manifests, data, and the text files of a crate folder.
pub fn allowed(rest: &str) -> bool {
    rest == SOURCE_MANIFEST
        || rest.starts_with("data/")
        || rest.strip_prefix("rust/").is_some_and(|path| {
            path.ends_with(".rs")
                || path.ends_with(".toml")
                || path.starts_with("data/")
                || (!path.contains('/') && text(path))
        })
        || (!rest.contains('/') && text(rest))
}

impl Bundle {
    pub fn read(path: &Path) -> Result<Self, String> {
        let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
        Self::read_zip(zip::ZipArchive::new(file).map_err(|e| e.to_string())?)
    }

    /// Pack a canonical project, shared by cargo-horizon-mod and the launcher.
    pub fn pack_directory(root: &Path, manifest: &Manifest) -> Result<Vec<u8>, String> {
        fn walk(dir: &Path, root: &Path, files: &mut Vec<(String, Vec<u8>)>) -> Result<(), String> {
            for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
                let entry = entry.map_err(|e| e.to_string())?;
                let path = entry.path();
                let kind = entry.file_type().map_err(|e| e.to_string())?;
                if kind.is_symlink() {
                    return Err(format!(
                        "{} is a link: not allowed in a mod",
                        path.display()
                    ));
                }
                if kind.is_dir() {
                    if entry.file_name() != "target" && entry.file_name() != ".git" {
                        walk(&path, root, files)?;
                    }
                } else {
                    let rest = path
                        .strip_prefix(root)
                        .map_err(|e| e.to_string())?
                        .to_string_lossy()
                        .replace('\\', "/");
                    if allowed(&rest) {
                        let size = entry.metadata().map_err(|e| e.to_string())?.len();
                        if files.len() >= 511
                            || size
                                + files
                                    .iter()
                                    .map(|(_, bytes)| bytes.len() as u64)
                                    .sum::<u64>()
                                > super::MAX_FILE
                        {
                            return Err("native bundle exceeds its entry or size limit".into());
                        }
                        files.push((
                            format!("{SOURCE}{rest}"),
                            std::fs::read(&path).map_err(|e| e.to_string())?,
                        ));
                    } else if rest.ends_with(".rs") || rest.ends_with(".toml") {
                        return Err(format!("unexpected native source: {rest}"));
                    }
                }
            }
            Ok(())
        }
        for old in ["mixins", "api", "content", SOURCE_MANIFEST] {
            if root.join(old).exists() {
                return Err(format!(
                    "unexpected project path: {old}; native sources belong in native/"
                ));
            }
        }
        let mut files = Vec::new();
        walk(&root.join(SOURCE), &root.join(SOURCE), &mut files)?;
        let content_manifest = root.join(crate::manifest::MANIFEST);
        if content_manifest.is_file() {
            let text = std::fs::read_to_string(content_manifest).map_err(|e| e.to_string())?;
            let content = crate::Content::read(root, &crate::Manifest::parse(&text)?)?;
            files.push((crate::manifest::MANIFEST.into(), text.into_bytes()));
            let mut reader = content.reader()?;
            for name in &content.files {
                files.push((name.clone(), reader.read(name)?));
            }
        }
        files.sort_by(|a, b| a.0.cmp(&b.0));
        Self::write_files(manifest, &files)
    }

    /// Deterministic package writer; producers pass archive-relative paths, including content.
    /// Reuse the reader's path, identity and expansion limits before returning any bytes.
    pub fn write_files(
        manifest: &Manifest,
        files: &[(String, Vec<u8>)],
    ) -> Result<Vec<u8>, String> {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default().unix_permissions(0o644);
        use std::io::Write;
        zip.start_file(MANIFEST, options)
            .map_err(|e| e.to_string())?;
        zip.write_all(&serde_json::to_vec_pretty(manifest).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        let has_content = files
            .iter()
            .any(|(name, _)| name == crate::manifest::MANIFEST);
        for (name, bytes) in files {
            if has_content {
                crate::manifest::check_file(name, 0, || Ok(bytes.as_slice()))
                    .map_err(|e| e.to_string())?;
            }
            zip.start_file(name, options).map_err(|e| e.to_string())?;
            zip.write_all(bytes).map_err(|e| e.to_string())?;
        }
        let bytes = zip.finish().map_err(|e| e.to_string())?.into_inner();
        Self::read_zip(
            zip::ZipArchive::new(std::io::Cursor::new(&bytes)).map_err(|e| e.to_string())?,
        )?;
        Ok(bytes)
    }

    fn read_zip<R: Read + std::io::Seek>(mut zip: zip::ZipArchive<R>) -> Result<Self, String> {
        let mut files = BTreeMap::new();
        let mut content_id = None;
        let mut total = 0u64;
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
            let name = entry.name().map_err(|e| e.to_string())?.into_owned();
            if entry.is_dir() {
                continue;
            }
            if entry
                .unix_mode()
                .is_some_and(|m| m & 0o170000 != 0 && m & 0o170000 != 0o100000)
                || name.contains('\\')
                || name
                    .split('/')
                    .any(|p| p.is_empty() || p == "." || p == "..")
                || name.starts_with('/')
                || name.contains(':')
            {
                return Err(format!("invalid native bundle path: {name}"));
            }
            if name == crate::manifest::WASM {
                return Err(WASM_GONE.into());
            }
            if content(&name) {
                if name == crate::manifest::MANIFEST {
                    let mut text = String::new();
                    (&mut entry)
                        .take(1 << 20)
                        .read_to_string(&mut text)
                        .map_err(|e| format!("{name}: {e}"))?;
                    content_id = Some(crate::Manifest::parse(&text)?.id);
                }
                continue;
            }
            if files.len() >= 512 {
                return Err("native bundle has too many entries".into());
            }
            let mut bytes = Vec::new();
            (&mut entry)
                .take(super::MAX_FILE + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            total += bytes.len() as u64;
            if total > super::MAX_FILE {
                return Err("native bundle expands beyond 64 MiB".into());
            }
            if files.insert(name, bytes).is_some() {
                return Err("duplicate native bundle path".into());
            }
        }
        let manifest: Manifest =
            serde_json::from_slice(files.get(MANIFEST).ok_or("missing horizon-native.json")?)
                .map_err(|e| e.to_string())?;
        manifest.check()?;
        if let Some(id) = content_id.filter(|id| *id != manifest.id) {
            return Err(format!(
                "its {} is mod {id}, its {MANIFEST} {}: one file is one mod",
                crate::manifest::MANIFEST,
                manifest.id
            ));
        }
        let prefix = SOURCE;
        for name in files.keys().filter(|n| n.as_str() != MANIFEST) {
            let Some(rest) = name.strip_prefix(prefix) else {
                return Err(format!("unexpected native source: {name}"));
            };
            if !allowed(rest) {
                return Err(format!("unexpected native source: {name}"));
            }
        }
        let source = files
            .get(&format!("{prefix}horizon-mixin.toml"))
            .ok_or("missing horizon-mixin.toml")?;
        let source: toml::Table =
            toml::from_str(std::str::from_utf8(source).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        check_source_layout(&source).map_err(|e| e.to_string())?;
        if source
            .get("mod")
            .and_then(|v| v.get("id"))
            .and_then(|v| v.as_str())
            != Some(&manifest.id)
            || source
                .get("mod")
                .and_then(|v| v.get("version"))
                .and_then(|v| v.as_str())
                != Some(&manifest.version)
            || source
                .get("compatibility")
                .and_then(|v| v.get("horizon_revision"))
                .and_then(|v| v.as_str())
                != Some(&manifest.horizon)
        {
            return Err("native manifest and mixin identity/revision differ".into());
        }
        Ok(Self { manifest, files })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn manifest() -> Manifest {
        Manifest {
            schema: 1,
            id: "veinminer".into(),
            version: "0.3.0".into(),
            game: "26.3".into(),
            horizon: "a".repeat(40),
            mixins: "b".repeat(40),
        }
    }
    fn read(name: &str, extras: &[(&str, &[u8])], mixin: &str) -> Result<Bundle, String> {
        let path =
            std::env::temp_dir().join(format!("native-bundle-{name}-{}.hmod", std::process::id()));
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file(MANIFEST, options).unwrap();
        zip.write_all(&serde_json::to_vec(&manifest()).unwrap())
            .unwrap();
        zip.start_file("native/horizon-mixin.toml", options)
            .unwrap();
        zip.write_all(mixin.as_bytes()).unwrap();
        for (name, bytes) in extras {
            zip.start_file(*name, options).unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
        let result = Bundle::read(&path);
        std::fs::remove_file(path).unwrap();
        result
    }
    fn mixin() -> String {
        format!(
            "[mod]\nid = \"veinminer\"\nversion = \"0.3.0\"\n[compatibility]\nhorizon_revision = \"{}\"\n",
            "a".repeat(40)
        )
    }
    #[test]
    fn source_routes_are_exclusive_and_canonical_through_the_archive_reader() {
        for declaration in [
            "[api]\npath = 'rust'\n",
            "[[crate]]\nid = 'code'\npath = 'rust'\n",
        ] {
            read("route-valid", &[], &(mixin() + declaration)).unwrap();
        }
        let both = "[api]\npath = 'rust'\n[[crate]]\nid = 'code'\npath = 'rust'\n";
        assert_eq!(
            check_source_layout(&both.parse().unwrap()),
            Err(SourceLayoutError::BothRoutes)
        );
        for (i, declaration) in [
            both,
            "[api]\npath = 'api'\n",
            "[[crate]]\nid = 'a'\npath = '.'\n",
            "[[crate]]\nid = 'a'\npath = '../rust'\n",
            "[api]\npath = 1\n",
            "[[crate]]\nid = 'a'\npath = 'rust'\n[[crate]]\nid = 'b'\npath = 'rust'\n",
        ]
        .iter()
        .enumerate()
        {
            assert!(read(&format!("route-{i}"), &[], &(mixin() + declaration)).is_err());
        }
        for (i, path) in [
            "mixins/veinminer/hook.rs",
            "native/veinminer/hook.rs",
            "native/api/lib.rs",
            "api/lib.rs",
            "native/hook.rs",
        ]
        .iter()
        .enumerate()
        {
            assert!(read(&format!("old-root-{i}"), &[(path, b"source")], &mixin()).is_err());
        }
    }

    #[test]
    fn shared_writer_roundtrips_and_enforces_reader_limits() {
        let mut files = vec![
            ("native/horizon-mixin.toml".into(), mixin().into_bytes()),
            ("native/rust/src/lib.rs".into(), b"fn hook() {}".to_vec()),
        ];
        let bytes = Bundle::write_files(&manifest(), &files).unwrap();
        let bundle =
            Bundle::read_zip(zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap()).unwrap();
        assert_eq!(bundle.files["native/rust/src/lib.rs"], b"fn hook() {}");
        files.extend((0..512).map(|i| (format!("native/data/{i}.bin"), vec![0])));
        assert!(
            Bundle::write_files(&manifest(), &files)
                .unwrap_err()
                .contains("too many entries")
        );
    }

    #[test]
    fn archive_with_an_unknown_manifest_field_is_refused() {
        let path = std::env::temp_dir().join(format!("native-unknown-{}.hmod", std::process::id()));
        for name in ["hooks", "banana"] {
            let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
            zip.start_file(MANIFEST, zip::write::SimpleFileOptions::default())
                .unwrap();
            let mut json = serde_json::to_value(manifest()).unwrap();
            json[name] = serde_json::json!(1);
            zip.write_all(&serde_json::to_vec(&json).unwrap()).unwrap();
            zip.finish().unwrap();
            let error = Bundle::read(&path).err().unwrap();
            assert!(error.contains("unknown field"), "{error}");
            assert!(error.contains(name), "{error}");
        }
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn package_kind_does_not_depend_on_install_folder() {
        let root = std::env::temp_dir().join(format!("native-kind-{}", std::process::id()));
        for (folder, native) in [("content", true), ("native", false)] {
            let dir = root.join(folder);
            std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join("test.hmod");
            let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
            zip.start_file(
                if native { MANIFEST } else { "horizon-mod.toml" },
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
            zip.write_all(b"{}").unwrap();
            zip.finish().unwrap();
            assert_eq!(holds(&path), native);
        }
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn compressed_manifest_still_identifies_native_package() {
        let path =
            std::env::temp_dir().join(format!("native-compression-{}.hmod", std::process::id()));
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        zip.start_file(MANIFEST, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"{}").unwrap();
        zip.finish().unwrap();
        let mut bytes = std::fs::read(&path).unwrap();
        bytes[8..10].copy_from_slice(&99u16.to_le_bytes());
        let central = bytes.windows(4).position(|w| w == b"PK\x01\x02").unwrap();
        bytes[central + 10..central + 12].copy_from_slice(&99u16.to_le_bytes());
        std::fs::write(&path, bytes).unwrap();
        assert!(holds(&path));
        assert!(Bundle::read(&path).is_err());
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn source_bundle_carries_exact_compatibility_into_index_facts() {
        let bundle = read(
            "valid",
            &[("native/rust/src/lib.rs", b"fn hook() {}")],
            &mixin(),
        )
        .unwrap();
        assert_eq!(bundle.manifest, manifest());
        let facts = bundle.manifest.facts();
        assert_eq!(
            facts.api,
            format!("native-1/{}/{}", "a".repeat(40), "b".repeat(40))
        );
        assert_eq!(facts.id.as_deref(), Some("veinminer"));
        assert!(facts.capabilities.is_empty());
    }
    #[test]
    fn accepts_binary_data_in_the_bundle_data_directory() {
        let bytes = [0, 255, 42];
        let bundle = read(
            "data",
            &[
                ("native/data/nested/table.bin", &bytes),
                ("native/rust/data/model.bin", &bytes),
                ("native/rust/README.md", b"# engine"),
                ("native/rust/LICENSE-MIT", b"MIT"),
                ("native/.gitignore", b"target/"),
            ],
            &mixin(),
        )
        .unwrap();
        assert_eq!(bundle.files["native/data/nested/table.bin"], bytes);
    }
    #[test]
    fn rejects_path_traversal_and_non_source_payloads() {
        for (i, name) in [
            "../escape.rs",
            "/absolute.rs",
            "mixins/other/hook.rs",
            "native/code.dll",
            "native/data/../code.dll",
            "native/database/file.bin",
            "native/rust/data.bin",
            "native/rust/src/data/blob.bin",
            "native/rust/src/README.md",
            "native/rust/README.dll/x.bin",
            "native/rust/Cargo.lock",
            "mixins/other/data/file.bin",
            "mixins\\veinminer\\hook.rs",
        ]
        .iter()
        .enumerate()
        {
            assert!(read(&format!("bad-path-{i}"), &[(name, b"bad")], &mixin()).is_err());
        }
    }
    #[test]
    fn one_hmod_carries_content_beside_its_native_code() {
        let toml = r#"id = "veinminer"
name = "Veinminer"
version = "0.3.0"
api = "0.2"
capabilities = ["registries"]
"#;
        let bundle = read(
            "unified",
            &[
                ("horizon-mod.toml", toml.as_bytes()),
                ("assets/veinminer/lang/en_us.json", b"{}"),
                ("data/veinminer/tags/item/ores.json", b"{}"),
                ("native/rust/src/lib.rs", b"fn hook() {}"),
            ],
            &mixin(),
        )
        .unwrap();
        let names: Vec<&str> = bundle.files.keys().map(String::as_str).collect();
        assert_eq!(
            names,
            [
                MANIFEST,
                "native/horizon-mixin.toml",
                "native/rust/src/lib.rs"
            ]
        );
        let other = toml.replace(r#""veinminer""#, r#""other""#);
        let e = read(
            "two-ids",
            &[("horizon-mod.toml", other.as_bytes())],
            &mixin(),
        )
        .err()
        .unwrap();
        assert!(e.contains("one file is one mod"), "{e}");
        let e = read("wasm", &[("mod.wasm", b"\0asm")], &mixin())
            .err()
            .unwrap();
        assert_eq!(e, WASM_GONE);
    }
    #[test]
    fn rejects_missing_or_mismatched_mixin_identity_without_panicking() {
        assert!(read("missing-mod", &[], "[unrelated]\nvalue = 1").is_err());
        assert!(
            read(
                "wrong-revision",
                &[],
                &mixin().replace(&"a".repeat(40), &"c".repeat(40))
            )
            .is_err()
        );
        assert!(read("wrong-version", &[], &mixin().replace("0.3.0", "0.2.0")).is_err());
        assert!(
            Manifest {
                horizon: "nightly".into(),
                ..manifest()
            }
            .check()
            .is_err()
        );
    }
}
