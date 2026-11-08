//! Shared runtime lease protocol; kernel exclusion protects participating cleanup, not arbitrary deletion.
use crate::locks::{LockMode, Locked, Release, locked};
use cap_fs_ext::{DirExt, FollowSymlinks, MetadataExt, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions};
use sha2::{Digest, Sha256};
use std::{fs, fs::Metadata, io, path::Path, sync::Arc};

#[derive(Debug)]
pub enum Error {
    Busy,
    UnsupportedFilesystem,
    Unsafe(std::path::PathBuf),
    Io(std::io::Error),
}
impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}
impl From<fs::TryLockError> for Error {
    fn from(error: fs::TryLockError) -> Self {
        match error {
            fs::TryLockError::WouldBlock => Self::Busy,
            fs::TryLockError::Error(error) => Self::Io(error),
        }
    }
}

pub fn linked(metadata: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        // Includes junctions and other reparse points, not just symbolic links.
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

pub fn checked(path: &Path, directory: bool) -> Result<Option<Metadata>, Error> {
    match fs::symlink_metadata(path) {
        Ok(meta)
            if linked(&meta)
                || (if directory {
                    !meta.is_dir()
                } else {
                    !meta.is_file()
                }) =>
        {
            Err(Error::Unsafe(path.to_owned()))
        }
        Ok(meta) => Ok(Some(meta)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

pub struct Registry {
    directory: Dir,
    gate: Locked,
    release: Release,
    create: bool,
}

// Field order releases the tree before the last reference can release the gate.
pub struct Claim {
    tree: Locked,
    registry: Arc<Registry>,
}

impl Claim {
    pub fn files(&self) -> [&fs::File; 2] {
        [self.tree.file(), self.registry.gate.file()]
    }
}

impl Registry {
    pub fn with_release(data: &Path, release: Release) -> Result<Arc<Self>, Error> {
        checked(data, true)?;
        fs::create_dir_all(data)?;
        let data = Dir::open_ambient_dir(data.canonicalize()?, cap_std::ambient_authority())?;
        Self::from_directory(data, release, true)
    }

    /// Entry never creates registry, gate, tree or record entries.
    pub fn existing(data: &Path) -> Result<Arc<Self>, Error> {
        Self::from_directory(open_directory(data)?, Release::Close, false)
    }

    fn from_directory(data: Dir, release: Release, create: bool) -> Result<Arc<Self>, Error> {
        filesystem::local(&data)?;
        if create {
            match data.create_dir("run-leases") {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error.into()),
            }
        }
        let directory = Dir::from_std_file(retained(
            data.open_dir_nofollow("run-leases")?.into_std_file(),
        )?);
        let gate = open_lock(&directory, "gate", LockMode::Shared, release, create)?;
        Ok(Arc::new(Self {
            directory,
            gate,
            release,
            create,
        }))
    }

    /// Gate first; identity comes from a held directory, never a mutable path.
    pub fn lock(self: &Arc<Self>, tree: &Dir, mode: LockMode) -> Result<Claim, Error> {
        filesystem::local(tree)?;
        let metadata = tree.dir_metadata()?;
        let identity = format!("{}:{}", metadata.dev(), metadata.ino());
        let key = format!("{:x}", Sha256::digest(identity.as_bytes()));
        let tree = open_lock(&self.directory, &key, mode, self.release, self.create)?;
        Ok(Claim {
            tree,
            registry: self.clone(),
        })
    }
}

// A creator must be compatible with shared readers even before its handle closes.
fn initialize_lock(directory: &Dir, key: &str) -> std::io::Result<cap_std::fs::File> {
    let mut initial = OpenOptions::new();
    initial
        .read(true)
        .write(true)
        .create_new(true)
        .follow(FollowSymlinks::No);
    #[cfg(windows)]
    {
        use cap_std::fs::OpenOptionsExt;
        initial
            .access_mode(windows_sys::Win32::Foundation::GENERIC_READ)
            .share_mode(1);
    }
    directory.open_with(key, &initial)
}

fn open_lock(
    directory: &Dir,
    key: &str,
    mode: LockMode,
    release: Release,
    create: bool,
) -> Result<Locked, Error> {
    if create {
        match initialize_lock(directory, key) {
            Ok(file) => drop(file),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
    }
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    #[cfg(not(windows))]
    options.write(true);
    #[cfg(windows)]
    {
        use cap_std::fs::OpenOptionsExt;
        options
            .write(matches!(mode, LockMode::Exclusive))
            .share_mode(if matches!(mode, LockMode::Shared) {
                1
            } else {
                0
            });
    }
    let file = directory
        .open_with(key, &options)
        .map_err(|error| {
            #[cfg(windows)]
            if matches!(error.raw_os_error(), Some(32 | 33)) {
                return Error::Busy;
            }
            Error::Io(error)
        })?
        .into_std();
    if !file.metadata()?.is_file() {
        return Err(Error::Unsafe(key.into()));
    }
    let lock = locked(retained(file)?, mode)?;
    Ok(match release {
        Release::Unlock => lock,
        Release::Close => lock.close_only(),
    })
}

/// Resolve an authoritative absolute directory with no symlink/reparse traversal.
pub fn open_directory(path: &Path) -> Result<Dir, Error> {
    use std::path::Component;
    if !path.is_absolute() {
        return Err(Error::Unsafe(path.into()));
    }
    let root = path
        .ancestors()
        .last()
        .ok_or_else(|| Error::Unsafe(path.into()))?;
    let mut directory = Dir::open_ambient_dir(root, cap_std::ambient_authority())?;
    for component in path
        .strip_prefix(root)
        .map_err(|_| Error::Unsafe(path.into()))?
        .components()
    {
        let Component::Normal(name) = component else {
            return Err(Error::Unsafe(path.into()));
        };
        directory = directory.open_dir_nofollow(name)?;
    }
    Ok(Dir::from_std_file(retained(directory.into_std_file())?))
}

/// Fresh retained metadata and leases must never become later stdio replacement targets.
pub(crate) fn retained(file: fs::File) -> io::Result<fs::File> {
    #[cfg(unix)]
    {
        use std::os::fd::{AsRawFd, FromRawFd};
        if file.as_raw_fd() <= 2 {
            // SAFETY: file owns the source; fcntl returns a new exclusively owned CLOEXEC fd.
            let fd = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 3) };
            if fd < 0 {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: successful F_DUPFD_CLOEXEC returned a fresh fd; transfer sole ownership.
            return Ok(unsafe { fs::File::from_raw_fd(fd) });
        }
    }
    Ok(file)
}

pub mod cleanup;
mod filesystem;
#[cfg(test)]
mod tests;
