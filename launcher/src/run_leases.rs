//! Shared tree-lock primitives. Persistent files are lock inodes, never busy markers.
use crate::native_cache::{Error, LockMode, Release};
use cap_std::fs::Dir;
use std::{fs, path::Path, process::Command, sync::Arc};

pub(crate) const SOURCE: &str = "HORIZON_RUNTIME_SOURCE";

pub(crate) fn mark(command: &mut Command, source: &Path) {
    command.env(SOURCE, source);
}

// Outer None inherits the environment; inner None explicitly removes the variable.
fn override_value<'a>(command: &'a Command, key: &str) -> Option<Option<&'a std::ffi::OsStr>> {
    command
        .get_envs()
        .find(|(name, _)| *name == key)
        .map(|(_, value)| value)
}

// This adapter owns localized launcher errors; the kernel protocol lives below game/launcher.
pub(crate) struct Registry(Arc<horizon_mod_package::runtime_leases::Registry>);
pub(crate) use horizon_mod_package::runtime_leases::Claim;
impl Registry {
    fn at(data: &Path) -> Result<Arc<Self>, Error> {
        Self::with_release(data, Release::Unlock)
    }
    pub(crate) fn with_release(data: &Path, release: Release) -> Result<Arc<Self>, Error> {
        horizon_mod_package::runtime_leases::Registry::with_release(data, release)
            .map(|registry| Arc::new(Self(registry)))
            .map_err(Into::into)
    }
    pub(crate) fn open() -> Result<Arc<Self>, Error> {
        Self::at(&crate::backend::data_dir())
    }
    pub(crate) fn lock(self: &Arc<Self>, tree: &Dir, mode: LockMode) -> Result<Claim, Error> {
        self.0.lock(tree, mode).map_err(Into::into)
    }
}

/// Launcher-owned primitives-in-use; this is not a standalone cleanup safety guarantee.
pub(crate) fn shared(command: &Command) -> Result<Vec<Claim>, Error> {
    let logs = override_value(command, "HORIZON_HOME").map_or_else(
        || std::env::var_os("HORIZON_HOME").map(std::path::PathBuf::from),
        |value| value.map(std::path::PathBuf::from),
    );
    let source = override_value(command, SOURCE)
        .flatten()
        .map(Path::new)
        .or_else(|| {
            let program = Path::new(command.get_program());
            program.is_absolute().then(|| program.parent()).flatten()
        });
    let mut paths = Vec::new();
    if let Some(home) = command.get_current_dir() {
        paths.push(home.canonicalize()?);
    }
    if let Some(logs) = logs.filter(|path| !path.as_os_str().is_empty()) {
        fs::create_dir_all(&logs)?;
        paths.push(logs.canonicalize()?);
    }
    if let Some(source) = source {
        let source = source.canonicalize()?;
        if let Some(parent) = source.parent() {
            paths.push(parent.to_owned());
        }
        paths.push(source);
    }
    // Native clients read prepared data from a different installed version. Hold that
    // tree explicitly as well as the executable's tree for the child's entire lifetime.
    // prepare_command also runs non-game commands (including hooks and tools), whose
    // arguments must not be interpreted by the game's parser.
    if command.get_args().any(|argument| argument == "--resources") {
        let arguments = horizon_mod_package::selection::LaunchArguments::for_command(command)?;
        for argument in arguments.rest {
            if let horizon_mod_package::selection::Argument::Value(flag, resources) = argument {
                if flag == "--resources" {
                    paths.push(
                        command
                            .get_current_dir()
                            .unwrap_or(Path::new("."))
                            .join(resources)
                            .canonicalize()?,
                    );
                }
            }
        }
    }
    paths.sort();
    paths.dedup();
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    let registry = Registry::open()?;
    paths
        .into_iter()
        .map(|path| {
            let directory = Dir::open_ambient_dir(path, cap_std::ambient_authority())?;
            registry.lock(&directory, LockMode::Shared)
        })
        .collect()
}

#[cfg(test)]
mod tests;
