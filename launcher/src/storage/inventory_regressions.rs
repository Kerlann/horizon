use super::*;
use crate::storage::tests::Home;

#[cfg(unix)]
#[test]
fn actor_starting_after_the_last_process_observation_cannot_authorize_cleanup() {
    use std::{
        io::{Read, Write},
        process::{Command, Stdio},
    };
    let home = Home::new();
    let cache = home.seed("cache/output");
    let storage = Storage::at(&home.0).unwrap();
    let entries = vec![Entry::fixed(
        &Root::open(&home.0).unwrap(),
        Path::new("cache"),
        Category::OtherCaches,
    )
    .unwrap()];
    let tag = format!(
        "last-scan-{}",
        home.0.file_name().unwrap().to_string_lossy()
    );
    // This is the LAST process observation: the actor starts only after its result exists.
    assert!(!backend::in_use(&tag));
    let mut actor = Command::new("sh")
        .args([
            "-c",
            "exec 3<>\"$1\"; printf ready; read -r stop",
            "late-cache-actor",
        ])
        .arg(&cache)
        .arg(backend::version_dir(&tag).join(backend::CLIENT))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut ready = [0; 5];
    actor.stdout.take().unwrap().read_exact(&mut ready).unwrap();
    assert_eq!(&ready, b"ready");
    let result = storage.apply(entries, Some(Category::OtherCaches), false);
    let retained = cache.exists();
    actor.stdin.take().unwrap().write_all(b"stop\n").unwrap();
    assert!(actor.wait().unwrap().success());
    assert!(matches!(result, Err(Error::UnprovenExclusion)));
    assert!(
        retained,
        "actor started after the final scan lost its cache"
    );
    // A later absence observation is not mutual exclusion either.
    assert!(!backend::in_use(&tag));
    let entry = Entry::fixed(
        &Root::open(&home.0).unwrap(),
        Path::new("cache"),
        Category::OtherCaches,
    )
    .unwrap();
    assert!(matches!(entry.clear(), Err(Error::UnprovenExclusion)));
    assert!(cache.exists());
}

#[cfg(unix)]
#[test]
fn entry_uses_held_directory_after_root_path_is_redirected() {
    let home = Home::new();
    let outside = Home::new();
    home.seed("cache/output");
    let precious = outside.seed("cache/precious");
    let storage = Storage::at(&home.0).unwrap();
    let entry = Entry::fixed(
        &Root::open(&home.0).unwrap(),
        Path::new("cache"),
        Category::RustCompiler,
    )
    .unwrap();
    let moved = home.0.with_extension("original");
    fs::rename(&home.0, &moved).unwrap();
    std::os::unix::fs::symlink(&outside.0, &home.0).unwrap();
    storage
        .apply(vec![entry], Some(Category::RustCompiler), false)
        .unwrap();
    assert!(precious.exists());
    assert!(!moved.join("cache/output").exists());
    fs::remove_file(&home.0).unwrap();
    fs::rename(moved, &home.0).unwrap();
}

#[cfg(unix)]
#[test]
fn hard_linked_outputs_across_entries_count_once() {
    use std::os::unix::fs::MetadataExt;
    let home = Home::new();
    let output = home.seed("native/source/first");
    fs::create_dir_all(home.0.join("native/patched")).unwrap();
    fs::hard_link(&output, home.0.join("native/patched/second")).unwrap();
    let expected = [
        output,
        home.0.join("native/source"),
        home.0.join("native/patched"),
    ]
    .into_iter()
    .map(|path| fs::metadata(path).unwrap().blocks() * 512)
    .sum::<u64>();
    let storage = Storage::at(&home.0).unwrap();
    let entries = ["native/source", "native/patched"]
        .into_iter()
        .map(|path| {
            Entry::fixed(
                &Root::open(&home.0).unwrap(),
                Path::new(path),
                Category::BuildFiles,
            )
            .unwrap()
        })
        .collect();
    let report = storage.apply(entries, None, false).unwrap();
    let row = report
        .rows
        .iter()
        .find(|row| row.category == Category::BuildFiles)
        .unwrap();
    assert_eq!(row.bytes, expected);
}

#[test]
fn referenced_jar_and_uninstrumented_unused_jar_are_kept() {
    use horizon_mod_package::{manifest::JarRef, Package};
    use sha2::{Digest, Sha512};
    use std::io::Write;
    let home = Home::new();
    fs::create_dir_all(home.0.join("jars")).unwrap();
    let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    archive
        .start_file(
            "assets/example/lang/en_us.json",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
    archive.write_all(b"{}").unwrap();
    let bytes = archive.finish().unwrap().into_inner();
    let jar = JarRef::new(
        "original".into(),
        "1.0.0".into(),
        format!("{:x}", Sha512::digest(&bytes)),
    )
    .unwrap();
    let referenced = jar.cache_path(&home.0.join("jars"));
    fs::write(&referenced, bytes).unwrap();
    let manifest = home.seed("horizon-mods/example/horizon-mod.toml");
    fs::write(&manifest, format!("id='example'\nname='Example'\nversion='1.0.0'\napi='0.2'\ncapabilities=['registries']\n[[jar]]\nid='original'\nversion='1.0.0'\nsha512='{}'\n", jar.sha512())).unwrap();
    Package::read(manifest.parent().unwrap()).unwrap();
    let unused = home.seed("jars/unused.jar");
    let storage = Storage::at(&home.0).unwrap();
    let mut entries = Vec::new();
    unused_jars(&mut entries, &Root::open(&home.0).unwrap()).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].path, unused);
    assert!(matches!(
        storage.apply(entries, Some(Category::OtherCaches), false),
        Err(Error::UnprovenExclusion)
    ));
    assert!(referenced.exists());
    assert!(unused.exists());
}
