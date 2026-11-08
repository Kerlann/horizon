//! Owned navigation over bounded row windows.
use std::sync::Arc;

use crate::wire::{Reader, RecordPatch, Wire, WireError};
use crate::{Row, RowError, RowPages, RowWindow};

// exhaustive: generated boundary code builds it; its fields are private
/// A node and an owned snapshot of rows, with an optional source for further windows.
/// Cached rows remain readable after the source expires; uncached reads report its refusal.
#[derive(Clone)]
pub struct RowView {
    rows: Arc<[Row]>,
    source: Option<Arc<dyn RowPages>>,
    node: u32,
}

impl RowView {
    /// Create a view of a node in an owned window. Ambiguous node ids are rejected.
    pub fn new(
        rows: Vec<Row>,
        source: Option<Arc<dyn RowPages>>,
        node: u32,
    ) -> Result<Self, RowError> {
        if rows.iter().filter(|row| row.0 == node).count() != 1 {
            return Err(RowError::Bounds);
        }
        Ok(Self {
            rows: rows.into(),
            source,
            node,
        })
    }

    /// The source-local node id, also used for sparse patches.
    pub fn node(&self) -> u32 {
        self.node
    }

    /// The schema-provided type key for this node.
    pub fn type_key(&self) -> u64 {
        self.rows
            .iter()
            .find(|row| row.0 == self.node)
            .expect("view node exists")
            .3
    }

    /// Decode a UTF-16 string represented by a length header and indexed unit children.
    /// The limit is checked before allocating or fetching elements.
    pub fn utf16(&self, max_units: usize) -> Result<String, RowError> {
        let len = self.read::<u64>()?;
        let len = usize::try_from(len).map_err(|_| RowError::Budget)?;
        if len > max_units || len > 524_288 {
            return Err(RowError::Budget);
        }
        let mut units = Vec::with_capacity(len);
        for index in 0..len {
            units.push(self.child(index as u32)?.read::<u16>()?);
        }
        String::from_utf16(&units).map_err(|_| RowError::Unsupported)
    }

    /// Decode the node's entire ordinary Wire value.
    pub fn read<T: Wire>(&self) -> Result<T, RowError> {
        let row = self
            .rows
            .iter()
            .find(|row| row.0 == self.node)
            .ok_or(RowError::Bounds)?;
        Reader::decode(&row.4).map_err(RowError::from)
    }

    /// Add a scalar update for this node; the receiver checks permission and type.
    pub fn set<T: Wire>(&self, patch: &mut RecordPatch, value: &T) -> Result<(), WireError> {
        patch.set(self.node, value)
    }

    /// Find a field or element, fetching at most its parent and one child if absent.
    pub fn child(&self, field: u32) -> Result<Self, RowError> {
        let find = |rows: &[Row]| -> Result<Option<u32>, RowError> {
            let mut found = rows
                .iter()
                .filter(|r| r.1 == Some(self.node) && r.2 == field);
            let node = found.next().map(|r| r.0);
            if found.next().is_some() {
                return Err(RowError::Unsupported);
            }
            Ok(node)
        };
        if let Some(node) = find(&self.rows)? {
            if self.rows.iter().filter(|r| r.0 == node).count() != 1 {
                return Err(RowError::Unsupported);
            }
            return Ok(Self {
                rows: self.rows.clone(),
                source: self.source.clone(),
                node,
            });
        }
        let source = self.source.as_ref().ok_or(RowError::Bounds)?;
        let rows = source.window(RowWindow {
            node: self.node,
            start: u64::from(field),
            rows: 2,
            depth: 1,
        })?;
        let node = find(&rows)?.ok_or(RowError::Bounds)?;
        Self::new(rows, self.source.clone(), node)
    }
}
