use super::*;
use std::io::{Cursor, Write};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = std::env::current_dir()
            .unwrap()
            .join("target/jar-resolve-tests")
            .join(format!(
                "{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn jar() -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, contents) in [
        ("assets/example/test.png", b"texture".as_slice()),
        ("data/example/test.json", b"{}"),
        ("META-INF/neoforge.mods.toml", b"modId='example'"),
        ("Example.class", b"dummy"),
    ] {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(contents).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

fn reference(bytes: &[u8]) -> JarRef {
    JarRef::new(
        "example".into(),
        "1.0".into(),
        format!("{:x}", Sha512::digest(bytes)),
    )
    .unwrap()
}

fn response(jar: &JarRef) -> serde_json::Value {
    serde_json::json!({"files": [{"url": "https://cdn.modrinth.com/data/example.jar", "hashes": {"sha512": jar.sha512}}]})
}

#[test]
fn cache_hit_needs_no_network_or_consent() {
    let dir = Scratch::new();
    let bytes = jar();
    let jar = reference(&bytes);
    fs::write(jar.path(&dir.0), bytes).unwrap();
    resolve_with(
        &[jar],
        &dir.0,
        |_| panic!("lookup"),
        |_| panic!("consent"),
        |_, _| panic!("download"),
    )
    .unwrap();
}

#[test]
fn miss_looks_up_exact_file_asks_once_and_installs() {
    let dir = Scratch::new();
    let bytes = jar();
    let jar = reference(&bytes);
    let result = resolve_with(
        &[jar.clone(), jar.clone()],
        &dir.0,
        |j| parse_response(j, &response(j)),
        |missing| {
            assert_eq!(missing, std::slice::from_ref(&jar));
            true
        },
        |file, cache| download::save(file, Cursor::new(&bytes), cache, &|_| {}).map(|_| ()),
    );
    result.unwrap();
    assert_eq!(fs::read(jar.path(&dir.0)).unwrap(), bytes);
}

#[test]
fn corrupt_cache_is_refused_without_network() {
    let dir = Scratch::new();
    let jar = reference(&jar());
    fs::write(jar.path(&dir.0), b"wrong").unwrap();
    let result = resolve_with(
        &[jar],
        &dir.0,
        |_| panic!("lookup"),
        |_| panic!("consent"),
        |_, _| panic!("download"),
    );
    assert!(result.unwrap_err().contains("wrong SHA-512"));
}

#[test]
fn modrinth_response_must_name_exact_hash_and_cdn() {
    let jar = reference(&jar());
    assert!(parse_response(&jar, &serde_json::json!({"files": [{"url": "https://cdn.modrinth.com/wrong", "hashes": {"sha512": "wrong"}}]})).unwrap().is_none());
    assert!(parse_response(&jar, &serde_json::json!({"files": [{"url": "https://example.com/file", "hashes": {"sha512": jar.sha512}}]})).is_err());
    assert!(JarRef::new("x".into(), "1".into(), "../bad".into()).is_err());
}

#[test]
fn mismatch_is_refused_and_partial_file_removed() {
    let dir = Scratch::new();
    let jar = reference(&jar());
    let result = resolve_with(
        std::slice::from_ref(&jar),
        &dir.0,
        |j| parse_response(j, &response(j)),
        |_| true,
        |file, cache| download::save(file, Cursor::new(b"wrong"), cache, &|_| {}).map(|_| ()),
    );
    assert!(result.is_err());
    assert!(!jar.path(&dir.0).exists());
    assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 0);
}

#[test]
fn no_match_names_the_jar_and_cache() {
    let dir = Scratch::new();
    let jar = reference(&jar());
    let error = resolve_with(
        &[jar],
        &dir.0,
        |_| Ok(None),
        |_| panic!("consent"),
        |_, _| panic!("download"),
    )
    .unwrap_err();
    assert!(error.contains("example 1.0"));
    assert!(error.contains(&dir.0.display().to_string()));
}

#[test]
fn declining_consent_does_not_download() {
    let dir = Scratch::new();
    let jar = reference(&jar());
    assert!(resolve_with(
        &[jar],
        &dir.0,
        |j| parse_response(j, &response(j)),
        |_| false,
        |_, _| panic!("download")
    )
    .is_err());
    assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 0);
}
