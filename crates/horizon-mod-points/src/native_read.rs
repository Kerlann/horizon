//! Live, read-only native row traversal. Implementations are generated from the crossing rule.
//!
//! A node is borrowed only for the duration of a visit. Implementations must re-check the
//! requested step against the current value and must not store references in locator state.
use crate::locators::{Locators, Step};
use crate::native_window::WindowBuilder;
use crate::snapshot::Window;
use mod_api::wire::Put;
use mod_api::{Row, RowError, RowWindow};

/// One live projection. Implementations must not mutate the native root during a read.
// TODO(#2273): `len` counts declaration-order children; an emptiness query has no caller.
#[allow(clippy::len_without_is_empty)]
pub trait NativeNode {
    /// Canonical type identity key.
    fn key(&self) -> u64;
    /// Optional scalar or structural header; passed only while the value is borrowed.
    fn value(
        &self,
        emit: &mut dyn FnMut(Option<&dyn Put>) -> Result<(), RowError>,
    ) -> Result<(), RowError>;
    /// Number of declaration-order children in the current live value.
    fn len(&self) -> u64;
    /// Resolve an ordinal to its locator step and live child. Refuse invalid ordinals.
    fn child(
        &self,
        ordinal: u64,
        visit: &mut dyn FnMut(Step, u32, &dyn NativeNode) -> Result<(), RowError>,
    ) -> Result<(), RowError>;
}

/// Resolve a source-local locator afresh and return a bounded window of owned rows.
/// The caller checks call and root leases before invoking this function.
pub fn read(
    root: &dyn NativeNode,
    prefix: u32,
    paths: &mut Locators,
    request: RowWindow,
) -> Result<Vec<Row>, RowError> {
    read_checked(root, prefix, paths, Window::try_from(request)?)
}

/// Like [`read`], for a window already validated at the interface boundary.
pub fn read_checked(
    root: &dyn NativeNode,
    prefix: u32,
    paths: &mut Locators,
    request: Window,
) -> Result<Vec<Row>, RowError> {
    let prefix = paths.path(prefix)?;
    let (mut out, path) = WindowBuilder::new(paths, request)?;
    if !path.starts_with(&prefix) {
        return Err(RowError::Bounds);
    }
    fn resolve(
        node: &dyn NativeNode,
        path: &[Step],
        f: &mut dyn FnMut(&dyn NativeNode) -> Result<(), RowError>,
    ) -> Result<(), RowError> {
        let Some((&step, rest)) = path.split_first() else {
            return f(node);
        };
        let candidates = match step {
            Step::Index(n) => n..n.checked_add(1).ok_or(RowError::Bounds)?,
            Step::Field(_) | Step::Variant(_) => 0..node.len(),
            Step::Root(_) => return Err(RowError::Bounds),
        };
        for ordinal in candidates {
            let mut matched = false;
            let result = node.child(ordinal, &mut |found, _, child| {
                if found == step {
                    matched = true;
                    resolve(child, rest, f)
                } else {
                    Ok(())
                }
            });
            if matched {
                return result;
            }
            result?;
        }
        Err(RowError::Bounds)
    }
    fn descend(
        node: &dyn NativeNode,
        id: u32,
        depth: u16,
        start: u64,
        level: u16,
        out: &mut WindowBuilder<'_>,
    ) -> Result<(), RowError> {
        if depth == 0 || out.remaining() == 0 {
            return Ok(());
        }
        let len = node.len();
        if start > len {
            return Err(RowError::Bounds);
        }
        for ordinal in start..len {
            if out.remaining() == 0 {
                break;
            }
            let mut visited = false;
            node.child(ordinal, &mut |step, field, child| {
                visited = true;
                let mut child_id = None;
                child.value(&mut |value| {
                    child_id = Some(out.child(id, step, field, child.key(), level + 1, value)?);
                    Ok(())
                })?;
                descend(
                    child,
                    child_id.ok_or(RowError::Unsupported)?,
                    depth - 1,
                    0,
                    level + 1,
                    out,
                )
            })?;
            // A hidden field is skipped in a scan, but never shifted onto when it is requested.
            if !visited && level == 0 && ordinal == start {
                return Err(RowError::Bounds);
            }
        }
        Ok(())
    }
    if request.rows() == 0 {
        return Ok(Vec::new());
    }
    let (parent, step) = out.root_entry()?;
    let field = match step {
        Step::Root(n) | Step::Field(n) | Step::Variant(n) => n,
        Step::Index(n) => u32::try_from(n).map_err(|_| RowError::Bounds)?,
    };
    resolve(root, &path[prefix.len()..], &mut |node| {
        node.value(&mut |value| {
            out.root(parent, field, node.key(), value)?;
            Ok(())
        })?;
        descend(
            node,
            request.node(),
            request.depth(),
            request.start() as u64,
            0,
            &mut out,
        )
    })?;
    Ok(out.finish())
}
