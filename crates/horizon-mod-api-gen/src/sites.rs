//! List-driven call-expression slots. The staging coordinator activates these after widening.
use crate::points::plain_path;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
use syn::Expr;

/// A site refusal, kept typed until the staging boundary renders it.
#[derive(Debug)]
pub(crate) enum Cause {
    Io(std::io::Error),
    Syntax(syn::Error),
    Format(&'static str),
    Missing(&'static str),
    Ambiguous(&'static str),
    Signature(&'static str),
    Record(&'static str),
    Conflict(String),
}

/// The declaration that failed and its machine-distinguishable cause.
#[derive(Debug)]
pub(crate) struct Error {
    pub origin: Option<String>,
    pub cause: Cause,
}
impl Error {
    pub fn at(mut self, origin: impl Into<String>) -> Self {
        self.origin = Some(origin.into());
        self
    }
}
impl From<Cause> for Error {
    fn from(cause: Cause) -> Self {
        Self {
            origin: None,
            cause,
        }
    }
}
impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Cause::Io(error).into()
    }
}
impl From<syn::Error> for Error {
    fn from(error: syn::Error) -> Self {
        Cause::Syntax(error).into()
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(origin) = &self.origin {
            write!(f, "{origin}: ")?;
        }
        match &self.cause {
            Cause::Io(e) => write!(f, "{e}"),
            Cause::Syntax(e) => write!(f, "invalid syntax: {e}"),
            Cause::Format(s)
            | Cause::Missing(s)
            | Cause::Ambiguous(s)
            | Cause::Signature(s)
            | Cause::Record(s) => f.write_str(s),
            Cause::Conflict(other) => {
                write!(f, "conflicts with {other}: both select the same call")
            }
        }
    }
}
impl std::error::Error for Error {}

/// One requested call expression, with its list-file diagnostic location.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Declaration {
    pub target: String,
    pub selector: String,
    pub ordinal: Option<usize>,
    pub origin: String,
}

/// Read the sorted union of per-mod site files. Missing directories mean no sites.
pub(crate) fn listed(directory: &Path) -> Result<Vec<Declaration>, Error> {
    let contents = match fs::read_dir(directory) {
        Ok(files) => files,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(Error::from(e).at(directory.display().to_string())),
    };
    let mut files = contents
        .map(|e| e.map(|e| e.path()))
        .collect::<Result<Vec<PathBuf>, _>>()?;
    files.sort();
    let mut union = BTreeMap::new();
    for file in files
        .into_iter()
        .filter(|p| p.extension().is_some_and(|e| e == "txt"))
    {
        let text =
            fs::read_to_string(&file).map_err(|e| Error::from(e).at(file.display().to_string()))?;
        for (i, line) in text.lines().enumerate() {
            let line = line.split('#').next().unwrap_or_default().trim();
            if line.is_empty() {
                continue;
            }
            let origin = format!("{}:{}", file.display(), i + 1);
            let parse = || -> Result<_, Error> {
                let parts: Vec<_> = line.split('|').map(str::trim).collect();
                let [target, selector, ordinal] = parts.as_slice() else {
                    return Err(Cause::Format("expected target | selector | ordinal").into());
                };
                syn::parse_str::<syn::Path>(target)?;
                let call = if selector.starts_with('.') {
                    format!("__receiver{selector}()")
                } else {
                    format!("{selector}()")
                };
                if !matches!(
                    syn::parse_str::<Expr>(&call),
                    Ok(Expr::Call(_) | Expr::MethodCall(_))
                ) {
                    return Err(Cause::Format("selector must name a method or path call").into());
                }
                let ordinal = if ordinal.is_empty() {
                    None
                } else {
                    Some(
                        ordinal
                            .parse()
                            .map_err(|_| Cause::Format("ordinal must be a nonnegative integer"))?,
                    )
                };
                Ok((plain_path(target), selector.replace(' ', ""), ordinal))
            };
            let (target, selector, ordinal) = parse().map_err(|e| e.at(origin.clone()))?;
            union
                .entry((target.clone(), selector.clone(), ordinal))
                .or_insert(Declaration {
                    target,
                    selector,
                    ordinal,
                    origin,
                });
        }
    }
    Ok(union.into_values().collect())
}

/// A compiled call site's raw form, written to `points.json` beside the head/return points.
#[derive(Debug, serde::Serialize)]
pub(crate) struct Site {
    pub name: String,
    pub target: String,
    pub selector: String,
    pub ordinal: usize,
    pub hash: String,
    pub listener: String,
    pub record: String,
    pub receiver: String,
    pub args: Vec<(String, String)>,
    pub output: String,
}

#[cfg(test)]
mod tests;
