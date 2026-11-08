//! Sparse locator paths for generated native edit planners.
//!
//! The tree contains metadata and packet bytes only, never native addresses. A caller must
//! validate the source lease before resolving the paths against the current native values.
use crate::locators::{Locators, Step};
use mod_api::wire::{RecordPatch, WireError};
use std::collections::HashMap;

/// A sparse subtree. A node with a value never has descendants.
#[derive(Default)]
pub struct PatchTree<'a> {
    value: Option<&'a [u8]>,
    children: HashMap<Step, PatchTree<'a>>,
}

impl<'a> PatchTree<'a> {
    /// Group a patch beneath the caller's allowed locator prefixes. Every cell must belong to
    /// exactly one prefix. Unknown nodes and ancestor/descendant edits are rejected.
    pub fn roots(
        locators: &Locators,
        prefixes: &[u32],
        patch: &'a RecordPatch,
    ) -> Result<Vec<Self>, WireError> {
        let paths = prefixes
            .iter()
            .map(|id| {
                locators
                    .path(*id)
                    .map_err(|_| WireError::Invalid("unknown patch root"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        for (i, a) in paths.iter().enumerate() {
            for b in &paths[..i] {
                if a.starts_with(b) || b.starts_with(a) {
                    return Err(WireError::Invalid("overlapping patch roots"));
                }
            }
        }
        let mut roots: Vec<Self> = (0..prefixes.len()).map(|_| Self::default()).collect();
        for (id, bytes) in patch.iter() {
            let path = locators
                .path(id)
                .map_err(|_| WireError::Invalid("unknown patch node"))?;
            let Some((index, prefix)) = paths
                .iter()
                .enumerate()
                .find(|(_, prefix)| path.starts_with(prefix))
            else {
                return Err(WireError::Invalid("patch node outside source"));
            };
            let mut tree = &mut roots[index];
            for step in &path[prefix.len()..] {
                if tree.value.is_some() {
                    return Err(WireError::Invalid("overlapping patch nodes"));
                }
                tree = tree.children.entry(*step).or_default();
            }
            if tree.value.is_some() || !tree.children.is_empty() {
                return Err(WireError::Invalid("overlapping patch nodes"));
            }
            tree.value = Some(bytes);
        }
        Ok(roots)
    }

    /// Encoded replacement at this node, if present.
    pub fn value(&self) -> Option<&'a [u8]> {
        self.value
    }

    /// Resolve one metadata child without borrowing any native destination.
    pub fn child(&self, step: Step) -> Option<&Self> {
        self.children.get(&step)
    }

    /// Requested child steps. The generated planner must reject any unrecognized step.
    pub fn children(&self) -> impl Iterator<Item = (Step, &Self)> {
        self.children.iter().map(|(step, tree)| (*step, tree))
    }

    /// Whether this subtree contains no requested edits.
    pub fn is_empty(&self) -> bool {
        self.value.is_none() && self.children.is_empty()
    }
}
