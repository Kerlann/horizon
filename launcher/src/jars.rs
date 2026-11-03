//! Resolve Java jars as read-only pack sources. Java code in them is never loaded.
#[cfg(test)]
#[path = "jars_tests.rs"]
mod tests;
use crate::browser::{download, FileRef};
use crate::i18n::{tr, trf};
use sha2::{Digest, Sha512};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

/// A manifest-validated reference to one exact jar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JarRef {
    id: String,
    version: String,
    sha512: String,
}

impl JarRef {
    /// Validate the hash before it becomes a cache file name.
    pub fn new(id: String, version: String, sha512: String) -> Result<Self, String> {
        if sha512.len() != 128 || !sha512.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(tr("A jar SHA-512 must have 128 hexadecimal digits.").into());
        }
        Ok(Self {
            id,
            version,
            sha512: sha512.to_ascii_lowercase(),
        })
    }

    /// The source mod's id, version and exact SHA-512.
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn version(&self) -> &str {
        &self.version
    }
    pub fn sha512(&self) -> &str {
        &self.sha512
    }

    fn path(&self, cache: &Path) -> PathBuf {
        cache.join(format!("{}.jar", self.sha512))
    }
}

/// One file found by its exact hash, awaiting the player's consent.
struct Pending {
    jar: JarRef,
    file: FileRef,
}

fn matches(path: &Path, want: &str) -> Result<bool, String> {
    let mut file = fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut hash = Sha512::new();
    let mut buf = [0; 65536];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
    }
    Ok(format!("{:x}", hash.finalize()) == want)
}

/// Resolve the whole set, asking for consent once before downloading missing jars.
pub fn resolve(
    jars: &[JarRef],
    cache: &Path,
    consent: impl FnOnce(&[JarRef]) -> bool,
) -> Result<(), String> {
    resolve_with(jars, cache, lookup, consent, |file, cache| {
        download::fetch(file, cache, &|_| {}).map(|_| ())
    })
}

fn resolve_with(
    jars: &[JarRef],
    cache: &Path,
    lookup: impl Fn(&JarRef) -> Result<Option<FileRef>, String>,
    consent: impl FnOnce(&[JarRef]) -> bool,
    fetch: impl Fn(&FileRef, &Path) -> Result<(), String>,
) -> Result<(), String> {
    let pending = prepare_with(jars, cache, lookup)?;
    if pending.is_empty() {
        return Ok(());
    }
    let missing: Vec<_> = pending.iter().map(|p| p.jar.clone()).collect();
    if !consent(&missing) {
        return Err(trf(
            "Jar downloads were declined. Put the requested jars in {cache}.",
            &[("cache", &cache.display().to_string())],
        ));
    }
    for item in pending {
        fetch(&item.file, cache)?;
    }
    Ok(())
}

fn prepare_with(
    jars: &[JarRef],
    cache: &Path,
    lookup: impl Fn(&JarRef) -> Result<Option<FileRef>, String>,
) -> Result<Vec<Pending>, String> {
    let mut pending: Vec<Pending> = Vec::new();
    for jar in jars {
        if pending.iter().any(|p| p.jar.sha512 == jar.sha512) {
            continue;
        }
        let path = jar.path(cache);
        if path.exists() {
            if !matches(&path, &jar.sha512)? {
                return Err(trf(
                    "{path} has the wrong SHA-512; remove it and try again.",
                    &[("path", &path.display().to_string())],
                ));
            }
            continue;
        }
        let file = lookup(jar)?.ok_or_else(|| {
            trf("No download found for {id} {version}. Put the exact jar in {cache} as {sha512}.jar.", &[("id", &jar.id), ("version", &jar.version), ("cache", &cache.display().to_string()), ("sha512", &jar.sha512)])
        })?;
        pending.push(Pending {
            jar: jar.clone(),
            file,
        });
    }
    Ok(pending)
}

fn lookup(jar: &JarRef) -> Result<Option<FileRef>, String> {
    let url = format!(
        "https://api.modrinth.com/v2/version_file/{}?algorithm=sha512",
        jar.sha512
    );
    let response = match download::agent(&url).get(&url).call() {
        Ok(response) => response,
        Err(ureq::Error::Status(404, _)) => return Ok(None),
        Err(e) => {
            return Err(trf(
                "Modrinth lookup failed: {error}",
                &[("error", &e.to_string())],
            ))
        }
    };
    let version: serde_json::Value = response.into_json().map_err(|e| {
        trf(
            "Modrinth's answer wasn't readable: {error}",
            &[("error", &e.to_string())],
        )
    })?;
    parse_response(jar, &version)
}

fn parse_response(jar: &JarRef, version: &serde_json::Value) -> Result<Option<FileRef>, String> {
    let files = version["files"]
        .as_array()
        .ok_or_else(|| tr("Modrinth's answer has no files"))?;
    let Some(file) = files.iter().find(|f| {
        f["hashes"]["sha512"]
            .as_str()
            .is_some_and(|s| s.eq_ignore_ascii_case(&jar.sha512))
    }) else {
        return Ok(None);
    };
    let url = file["url"]
        .as_str()
        .ok_or_else(|| tr("Modrinth's file has no URL"))?
        .to_owned();
    if !url.starts_with("https://cdn.modrinth.com/") {
        return Err(tr("Modrinth returned a file outside its CDN.").into());
    }
    Ok(Some(FileRef {
        url,
        name: format!("{}.jar", jar.sha512),
        size: file["size"].as_u64().unwrap_or(0),
        sha512: Some(jar.sha512.clone()),
        sha256: None,
        sha1: None,
        primary: false,
    }))
}
