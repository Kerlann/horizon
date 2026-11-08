use super::*;

fn setup() -> (tempfile::TempDir, Arc<Registry>) {
    let root = tempfile::tempdir().unwrap();
    let registry = Registry::with_release(root.path(), Release::Unlock).unwrap();
    (root, registry)
}

fn populate(registry: &Registry, count: usize) {
    for index in 0..count {
        drop(initialize_lock(&registry.directory, &format!("{index:064x}")).unwrap());
    }
}

#[test]
fn boundaries_and_permanent_gate() {
    for count in [4095, 4096, 4097] {
        let (root, registry) = setup();
        populate(&registry, count);
        let before = registry.directory.metadata("gate").unwrap();
        drop(registry);
        let cleanup = Cleanup::existing(root.path()).unwrap();
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            assert!(cleanup.directory.as_raw_fd() > 2);
            assert!(cleanup._gate.file().as_raw_fd() > 2);
        }
        assert_eq!(
            cleanup.collect().unwrap(),
            if count < 4096 { 0 } else { count }
        );
        let after = cleanup.directory.metadata("gate").unwrap();
        assert_eq!((before.dev(), before.ino()), (after.dev(), after.ino()));
        assert_eq!(
            cleanup.directory.entries().unwrap().count(),
            if count < 4096 { count + 1 } else { 1 }
        );
        assert!(matches!(Registry::existing(root.path()), Err(Error::Busy)));
        drop(cleanup);
        assert!(Registry::existing(root.path()).is_ok());
    }
}

#[test]
fn every_claim_owner_holds_the_gate_until_its_last_reference() {
    for mode in [LockMode::Shared, LockMode::Exclusive] {
        let (root, registry) = setup();
        let tree = open_directory(root.path()).unwrap();
        let claim = registry.lock(&tree, mode).unwrap();
        let other = Registry::existing(root.path()).unwrap();
        assert!(matches!(Cleanup::existing(root.path()), Err(Error::Busy)));
        drop(registry);
        assert!(matches!(Cleanup::existing(root.path()), Err(Error::Busy)));
        drop(other);
        assert!(matches!(Cleanup::existing(root.path()), Err(Error::Busy)));
        drop(claim);
        let cleanup = Cleanup::existing(root.path()).unwrap();
        // A new owner cannot get past the gate to create any tree entry.
        assert!(matches!(
            Registry::with_release(root.path(), Release::Unlock),
            Err(Error::Busy)
        ));
        assert_eq!(cleanup.collect().unwrap(), 0);
    }
}

#[test]
fn unknown_entries_never_count_or_disappear() {
    let (root, registry) = setup();
    populate(&registry, 4095);
    let odd = [
        "unknown",
        &"A".repeat(64),
        &"g".repeat(64),
        &"f".repeat(63),
        &"c".repeat(64),
    ];
    for name in odd {
        registry.directory.write(name, b"retained").unwrap();
    }
    registry.directory.create_dir("e".repeat(64)).unwrap();
    drop(registry);
    let cleanup = Cleanup::existing(root.path()).unwrap();
    assert_eq!(cleanup.collect().unwrap(), 0);
    drop(initialize_lock(&cleanup.directory, &format!("{:064x}", 4095)).unwrap());
    assert_eq!(cleanup.collect().unwrap(), 4096);
    for name in odd {
        assert_eq!(cleanup.directory.read(name).unwrap(), b"retained");
    }
    assert!(cleanup.directory.metadata("e".repeat(64)).unwrap().is_dir());
}

#[test]
fn existing_never_creates_missing_namespace_or_gate() {
    let root = tempfile::tempdir().unwrap();
    assert!(Cleanup::existing(root.path()).is_err());
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    fs::create_dir(root.path().join("run-leases")).unwrap();
    assert!(Cleanup::existing(root.path()).is_err());
    assert_eq!(
        fs::read_dir(root.path().join("run-leases"))
            .unwrap()
            .count(),
        0
    );
}

#[cfg(unix)]
#[test]
fn redirects_and_hardlinks_are_retained() {
    use std::os::unix::fs::symlink;
    let (root, registry) = setup();
    populate(&registry, 4096);
    let outside = root.path().join("outside");
    fs::write(&outside, b"untouched").unwrap();
    let leases = root.path().join("run-leases");
    let link = leases.join("f".repeat(64));
    symlink(&outside, &link).unwrap();
    let hard = leases.join("d".repeat(64));
    fs::hard_link(&outside, &hard).unwrap();
    drop(registry);
    let cleanup = Cleanup::existing(root.path()).unwrap();
    assert_eq!(cleanup.collect().unwrap(), 4096);
    assert!(fs::symlink_metadata(link).unwrap().file_type().is_symlink());
    assert_eq!(fs::read(hard).unwrap(), b"untouched");
    drop(cleanup);
    fs::remove_file(leases.join("gate")).unwrap();
    symlink(&outside, leases.join("gate")).unwrap();
    assert!(Cleanup::existing(root.path()).is_err());
    assert_eq!(fs::read(&outside).unwrap(), b"untouched");
    fs::rename(&leases, root.path().join("moved")).unwrap();
    symlink(root.path().join("moved"), &leases).unwrap();
    assert!(Cleanup::existing(root.path()).is_err());
}

#[cfg(windows)]
#[test]
fn windows_open_reader_blocks_exclusive_gate_and_collection_shares_no_gate() {
    let (root, registry) = setup();
    let reader = initialize_lock(&registry.directory, "gate");
    assert!(reader.is_err()); // permanent gate is never recreated
    let mut options = OpenOptions::new();
    options.read(true);
    let reader = registry.directory.open_with("gate", &options).unwrap();
    drop(registry);
    assert!(matches!(Cleanup::existing(root.path()), Err(Error::Busy)));
    drop(reader);
    let cleanup = Cleanup::existing(root.path()).unwrap();
    assert!(cleanup.directory.open("gate").is_err());
}
