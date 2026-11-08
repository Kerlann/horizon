//! Path-aware adapter to the common bounded snapshot builder.
use crate::{
    locators::{Locators, Step},
    snapshot::{Builder, Window},
};
use mod_api::{Row, RowError, wire::Put};

/// A source-local locator table and the common owned-output builder for one window.
pub struct WindowBuilder<'a> {
    locators: &'a mut Locators,
    window: Window,
    builder: Builder,
    used: usize,
}
impl<'a> WindowBuilder<'a> {
    /// Resolve the requested locator.
    pub fn new(locators: &'a mut Locators, window: Window) -> Result<(Self, Vec<Step>), RowError> {
        let path = locators.path(window.node())?;
        Ok((
            Self {
                locators,
                window,
                builder: window.builder(),
                used: 0,
            },
            path,
        ))
    }
    /// Return the requested node's source-local parent and final step.
    pub fn root_entry(&self) -> Result<(Option<u32>, Step), RowError> {
        self.locators.entry(self.window.node())
    }
    /// Number of rows still available in this window.
    pub fn remaining(&self) -> usize {
        self.window.rows() - self.used
    }
    /// Append the requested node.
    pub fn root(
        &mut self,
        parent: Option<u32>,
        field: u32,
        ty: u64,
        value: Option<&dyn Put>,
    ) -> Result<u32, RowError> {
        let id = self
            .builder
            .external(self.window.node(), parent, field, ty, 0, value)?;
        self.used += 1;
        Ok(id)
    }
    /// Intern and append a child at its window-relative depth.
    pub fn child(
        &mut self,
        parent: u32,
        step: Step,
        field: u32,
        ty: u64,
        depth: u16,
        value: Option<&dyn Put>,
    ) -> Result<u32, RowError> {
        if self.remaining() == 0 {
            return Err(RowError::Budget);
        }
        let id = self.locators.intern(Some(parent), step)?;
        self.builder
            .external(id, Some(parent), field, ty, depth, value)?;
        self.used += 1;
        Ok(id)
    }
    /// Complete the owned rows.
    pub fn finish(self) -> Vec<Row> {
        self.builder.finish().into_rows()
    }
}
