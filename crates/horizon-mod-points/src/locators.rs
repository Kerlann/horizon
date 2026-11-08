//! Bounded, source-local paths for native row providers.
//!
//! A provider owns one `Locators` for the duration of a checked native root. It interns
//! paths while producing rows and resolves the returned steps against the live root on every
//! read. Steps are metadata only: they must never contain native addresses. In particular,
//! a provider must check indices and enum discriminants again during resolution. The caller
//! must validate the root's call and value leases before accessing either this table or native
//! data; this table does not itself confer a lifetime or thread-safety guarantee.

use mod_api::RowError;
use std::collections::HashMap;

/// Maximum number of source-local nodes, including roots.
pub const MAX_LOCATORS: usize = 65_536;
/// Maximum number of edges from a root.
pub const MAX_DEPTH: usize = 1_040;
/// Maximum encoded values in a single row window.
pub const MAX_WINDOW_BYTES: usize = 1_048_576;

/// A path component. Numbers are generated schema ordinals, not native offsets.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Step {
    /// One of the call's live native roots.
    Root(u32),
    /// A declared record or tuple field.
    Field(u32),
    /// An element in a sequence, string or map's stable iteration for this read.
    Index(u64),
    /// An enum or option case, checked against the live value before descending.
    Variant(u32),
}

#[derive(Clone, Copy)]
struct Entry {
    parent: Option<u32>,
    step: Step,
    depth: usize,
}

/// Interned paths with stable, source-local row identifiers.
///
/// Interning an existing path returns the same id. New entries are bounded before either
/// collection grows. Roots and descendants share a single id namespace.
#[derive(Default)]
pub struct Locators {
    entries: Vec<Entry>,
    ids: HashMap<(Option<u32>, Step), u32>,
}

impl Locators {
    /// Intern a root or child. Root steps are valid only without a parent.
    pub fn intern(&mut self, parent: Option<u32>, step: Step) -> Result<u32, RowError> {
        if matches!(step, Step::Root(_)) != parent.is_none() {
            return Err(RowError::Bounds);
        }
        let depth = match parent {
            Some(id) => self.entries.get(id as usize).ok_or(RowError::Bounds)?.depth + 1,
            None => 0,
        };
        if depth > MAX_DEPTH {
            return Err(RowError::Depth);
        }
        if let Some(id) = self.ids.get(&(parent, step)) {
            return Ok(*id);
        }
        if self.entries.len() >= MAX_LOCATORS {
            return Err(RowError::Budget);
        }
        let id = self.entries.len() as u32;
        self.entries.push(Entry {
            parent,
            step,
            depth,
        });
        self.ids.insert((parent, step), id);
        Ok(id)
    }

    /// Number of interned source-local nodes.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the source has no interned nodes.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Look up existing metadata without creating a new native destination.
    pub fn find(&self, parent: Option<u32>, step: Step) -> Option<u32> {
        self.ids.get(&(parent, step)).copied()
    }

    /// Return the parent and final step of a known source-local node.
    pub fn entry(&self, id: u32) -> Result<(Option<u32>, Step), RowError> {
        let entry = self.entries.get(id as usize).ok_or(RowError::Bounds)?;
        Ok((entry.parent, entry.step))
    }

    /// Reconstruct a path from its root. Unknown ids are refused, never treated as offsets.
    pub fn path(&self, id: u32) -> Result<Vec<Step>, RowError> {
        let mut entry = *self.entries.get(id as usize).ok_or(RowError::Bounds)?;
        let mut path = Vec::with_capacity(entry.depth + 1);
        loop {
            path.push(entry.step);
            match entry.parent {
                Some(parent) => entry = self.entries[parent as usize],
                None => break,
            }
        }
        path.reverse();
        Ok(path)
    }
}
