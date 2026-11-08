use super::*;
use crate::{
    output::Capture,
    selection::{LaunchArguments, ModSelectionContext},
};
/// Earliest-main only: acquire before logging, mod loading or worker creation.
pub fn acquire_entry(context: &ModSelectionContext) -> io::Result<Option<Acquired>> {
    let Some(encoded) = std::env::var_os(ENTRY) else {
        return Ok(None);
    };
    let reference = record::Reference::decode(encoded.to_str().ok_or(io::ErrorKind::InvalidData)?)?;
    // SAFETY: this entry point runs before application threads and consumes private bootstrap state.
    unsafe {
        std::env::remove_var(ENTRY);
    }
    reference.acquire(context).map(Some)
}
/// First operation in ordinary and generated main. Private holder mode never returns.
pub fn bootstrap() -> (Option<Capture>, Option<LaunchArguments>) {
    enter();
    if std::env::var_os(ENTRY).is_none() {
        return (None, None);
    }
    let result = (|| -> io::Result<(Capture, LaunchArguments)> {
        let mut command = Command::new(std::env::current_exe()?);
        command.args(std::env::args_os().skip(1));
        let selection = LaunchArguments::for_command(&command)?;
        let ownership = acquire_entry(&selection.context)?.ok_or(io::ErrorKind::InvalidData)?;
        // Native exit callbacks run after main's locals drop; only kernel exit may release claims.
        let ownership = Box::leak(Box::new(ownership));
        #[cfg(unix)]
        {
            use std::os::fd::{AsRawFd, IntoRawFd};
            for target in 0..3 {
                // SAFETY: earliest-main owns its standard slots; no concurrent descriptor creation.
                if unsafe { libc::fcntl(target, libc::F_GETFD) } < 0 {
                    let file = std::fs::OpenOptions::new()
                        .read(true)
                        .write(true)
                        .open("/dev/null")?;
                    // SAFETY: both slots are valid and exclusively owned at this point.
                    if unsafe { libc::dup2(file.as_raw_fd(), target) } < 0 {
                        return Err(io::Error::last_os_error());
                    }
                    if file.as_raw_fd() == target {
                        let _ = file.into_raw_fd();
                    }
                }
            }
        }
        let capture = Capture::install(ownership._capture.as_ref().unwrap())?;
        Ok((capture, selection))
    })();
    match result {
        Ok((capture, selection)) => (Some(capture), Some(selection)),
        Err(error) => {
            eprintln!("Horizon: {error}");
            std::process::exit(78);
        }
    }
}
