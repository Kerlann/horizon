//! Conservative lock-entry collection under the permanent namespace gate.
use super::*;

/// Exclusive namespace ownership, acquired without creating or upgrading a gate.
pub struct Cleanup {
    pub(crate) data: Dir,
    directory: Dir,
    _gate: Locked,
}

impl Cleanup {
    pub fn existing(data: &Path) -> Result<Self, Error> {
        let data = open_directory(data)?;
        filesystem::local(&data)?;
        let directory = Dir::from_std_file(retained(
            data.open_dir_nofollow("run-leases")?.into_std_file(),
        )?);
        let gate = open_lock(
            &directory,
            "gate",
            LockMode::Exclusive,
            Release::Close,
            false,
        )?;
        Ok(Self {
            data,
            directory,
            _gate: gate,
        })
    }

    /// Removes only recognized, unchanged regular lock entries, at 4,096 entries.
    /// The held exclusive gate, not this scan, authorizes collection.
    pub fn collect(&self) -> Result<usize, Error> {
        let mut known = Vec::new();
        for entry in self.directory.entries()? {
            let entry = entry?;
            let name = entry.file_name();
            let Some(key) = name.to_str() else { continue };
            if key.len() != 64
                || !key
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                continue;
            }
            let Ok(meta) = self.directory.symlink_metadata(&name) else {
                continue;
            };
            if regular(&meta) {
                known.push((name, meta));
            }
        }
        if known.len() < 4096 {
            return Ok(0);
        }
        let mut removed = 0;
        for (name, before) in known {
            let Ok(after) = self.directory.symlink_metadata(&name) else {
                continue;
            };
            if regular(&after) && before.dev() == after.dev() && before.ino() == after.ino() {
                self.directory.remove_file(&name)?;
                removed += 1;
            }
        }
        Ok(removed)
    }
}

fn regular(meta: &cap_std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use cap_std::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return false;
        }
    }
    meta.is_file() && !meta.file_type().is_symlink() && meta.nlink() == 1 && meta.len() == 0
}

#[cfg(test)]
mod tests;
