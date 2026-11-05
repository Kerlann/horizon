//! The staged game's internals catalog, shared by the launcher and mod tools.
use crate::manifest::Internal;
use std::{fs, path::Path};

/// One signature in `internals.json`, including guarded alternatives and lower-crate shapes.
#[derive(Clone, Debug, serde::Deserialize)]
pub struct ListedInternal {
    pub path: String,
    pub kind: String,
    #[serde(default)]
    pub cfg: String,
    pub signature: String,
    pub hash: String,
    #[serde(default)]
    pub shape_only: bool,
}

/// Why a declared internal cannot be used with this game.
#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    Missing {
        mod_id: String,
        path: String,
    },
    Changed {
        mod_id: String,
        path: String,
        signature: String,
    },
    Gated {
        mod_id: String,
        path: String,
    },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing { mod_id, path } => write!(
                f,
                "E_INTERNAL_MISSING: {mod_id} uses {path}, which this game doesn't have."
            ),
            Self::Changed {
                mod_id,
                path,
                signature,
            } => write!(
                f,
                "E_INTERNAL_CHANGED: {mod_id} uses {path}, whose signature is now {signature}; run cargo horizon-mod update to see what changed."
            ),
            Self::Gated { mod_id, path } => write!(
                f,
                "E_INTERNAL_GATED: {mod_id} uses {path}, which exists only under a cfg; target-specific internals aren't supported."
            ),
        }
    }
}
impl std::error::Error for Error {}

/// Read a catalog without compiling or executing game code.
pub fn read(path: &Path) -> Result<Vec<ListedInternal>, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Resolve an exposed, unconditional entry. Shape-only entries never grant access.
pub fn resolve<'a>(
    entries: &'a [ListedInternal],
    mod_id: &str,
    path: &str,
) -> Result<&'a ListedInternal, Error> {
    let mut gated = false;
    for entry in entries.iter().filter(|e| e.path == path && !e.shape_only) {
        if entry.cfg.is_empty() {
            return Ok(entry);
        }
        gated = true;
    }
    let (mod_id, path) = (mod_id.to_owned(), path.to_owned());
    Err(if gated {
        Error::Gated { mod_id, path }
    } else {
        Error::Missing { mod_id, path }
    })
}

/// Check a declaration's fingerprint before a mod is compiled.
pub fn check(entries: &[ListedInternal], mod_id: &str, declared: &Internal) -> Result<(), Error> {
    let entry = resolve(entries, mod_id, &declared.path)?;
    if entry.hash != declared.hash {
        return Err(Error::Changed {
            mod_id: mod_id.to_owned(),
            path: declared.path.clone(),
            signature: entry.signature.clone(),
        });
    }
    Ok(())
}
