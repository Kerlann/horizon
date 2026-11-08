//! Owned, flattened rows of one raw hook call. No row contains an engine address.
use mod_api::wire::{Put, Reader, Wire, WireError, Writer};
use std::{cell::RefCell, collections::HashMap, ops::Deref};

#[path = "snapshot_pages.rs"]
mod pages;
/// A row's node id, parent id, declaration-order field/index, type key and plain Wire value.
pub use mod_api::Row;
#[doc(hidden)]
pub use pages::{PageSource, Root, Window, with_root};
/// The raw call's snapshot. Generic tuple/list Wire impls carry it without a second codec.
pub type Rows = Vec<Row>;

/// A stable key for the type spellings in a generated view. Calls use constants, not text.
pub const fn key(name: &str) -> u64 {
    let bytes = name.as_bytes();
    let mut key = 0xcbf29ce484222325u64;
    let mut i = 0;
    while i < bytes.len() {
        key = (key ^ bytes[i] as u64).wrapping_mul(0x100000001b3);
        i += 1;
    }
    key
}

/// A read-only view of one raw call's flattened snapshot.
#[derive(Clone, Copy)]
pub struct Snapshot<'a>(&'a [Row]);
impl<'a> Snapshot<'a> {
    /// The rows of this call, owned by the call arena rather than the engine.
    pub fn new(rows: &'a [Row]) -> Self {
        Self(rows)
    }
    /// A declaration-order field or collection index of the given node.
    pub fn field(&self, parent: u32, field: u32) -> Option<u32> {
        self.0
            .iter()
            .find(|row| row.1 == Some(parent) && row.2 == field)
            .map(|row| row.0)
    }
    /// Decode a plain value from a row; a mismatched wire shape is refused.
    pub fn read<T: Wire>(&self, node: u32) -> Result<T, WireError> {
        let row = self
            .row(node)
            .ok_or(WireError::Invalid("unknown snapshot node"))?;
        Reader::decode(&row.4)
    }
    /// The generated type key, resolved by the per-mod view rather than sent as text per call.
    pub fn ty(&self, node: u32) -> Option<u64> {
        self.row(node).map(|row| row.3)
    }
    fn row(&self, node: u32) -> Option<&Row> {
        self.0
            .get(node as usize)
            .filter(|row| row.0 == node)
            .or_else(|| self.0.iter().find(|row| row.0 == node))
    }
}

type Buffers = (Rows, Vec<u16>);
thread_local! { static POOL: RefCell<Vec<Buffers>> = const { RefCell::new(Vec::new()) }; }
fn recycle(rows: Rows, depths: Vec<u16>) {
    let cost = |buffers: &Buffers| {
        buffers.0.capacity() * std::mem::size_of::<Row>()
            + buffers.1.capacity() * std::mem::size_of::<u16>()
            + buffers.0.iter().map(|row| row.4.capacity()).sum::<usize>()
    };
    if rows.capacity() == 0 && depths.capacity() == 0 {
        return;
    }
    POOL.with(|pool| {
        let mut pool = pool.borrow_mut();
        let buffers = (rows, depths);
        if cost(&buffers) + pool.iter().map(cost).sum::<usize>() <= 4_194_304 {
            pool.push(buffers);
        }
    });
}

/// A completed snapshot, kept until its synchronous hook returns. Its buffers are pooled by
/// call depth, so steady-state primitive snapshots do not allocate across different places.
pub struct Built {
    rows: Rows,
    depths: Vec<u16>,
    used: usize,
    pooled: bool,
}
impl Built {
    /// Take the owned rows out of this completed snapshot.
    pub fn into_rows(mut self) -> Rows {
        self.pooled = false;
        self.rows.truncate(self.used);
        std::mem::take(&mut self.rows)
    }
}
impl Deref for Built {
    type Target = [Row];
    fn deref(&self) -> &[Row] {
        &self.rows[..self.used]
    }
}
impl Put for Built {
    fn put_to(&self, out: &mut Writer) {
        out.put(&**self);
    }
}
impl Drop for Built {
    fn drop(&mut self) {
        if self.pooled {
            recycle(
                std::mem::take(&mut self.rows),
                std::mem::take(&mut self.depths),
            );
        }
    }
}

/// Builds a per-call snapshot with the former internals view's cell and byte budgets.
/// Refusing an oversized snapshot never truncates a row or exposes a partial tree.
pub struct Builder {
    rows: Rows,
    depths: Vec<u16>,
    bytes: usize,
    used: usize,
    max_rows: usize,
    max_depth: u16,
    pooled: bool,
    external: Option<HashMap<u32, usize>>,
}
impl Default for Builder {
    fn default() -> Self {
        let (rows, depths) = POOL
            .with(|pool| pool.borrow_mut().pop())
            .unwrap_or_default();
        Self {
            rows,
            depths,
            bytes: 0,
            used: 0,
            max_rows: 65_536,
            max_depth: mod_api::wire::MAX_DEPTH,
            pooled: true,
            external: None,
        }
    }
}
impl Drop for Builder {
    fn drop(&mut self) {
        if self.pooled {
            recycle(
                std::mem::take(&mut self.rows),
                std::mem::take(&mut self.depths),
            );
        }
    }
}
impl Builder {
    // Window's constructor already owns the row/depth cap checks. Explicit owned windows do
    // not consume the hook snapshot pool: their returned Vec belongs to the interface caller.
    fn limited(window: Window) -> Self {
        Self {
            rows: Vec::new(),
            depths: Vec::new(),
            bytes: 0,
            used: 0,
            max_rows: window.rows(),
            max_depth: window.depth(),
            pooled: false,
            external: None,
        }
    }
    /// Add an owned node. Node ids are assigned in row order, with checked parent depth.
    pub fn node(&mut self, parent: Option<u32>, field: u32, ty: u64) -> Result<u32, WireError> {
        if self.external.is_some() {
            return Err(WireError::Invalid("mixed snapshot id modes"));
        }
        let id = self.used as u32;
        if parent.is_some_and(|parent| parent >= id) {
            return Err(WireError::Invalid("snapshot parent is not an earlier node"));
        }
        let depth = parent.map_or(0, |parent| self.depths[parent as usize] + 1);
        self.push(id, parent, field, ty, depth)?;
        Ok(id)
    }
    fn push(
        &mut self,
        id: u32,
        parent: Option<u32>,
        field: u32,
        ty: u64,
        depth: u16,
    ) -> Result<usize, WireError> {
        if self.used >= self.max_rows {
            return Err(WireError::Length);
        }
        if depth > self.max_depth {
            return Err(WireError::Depth);
        }
        let slot = self.used;
        if self.used == self.rows.len() {
            self.rows.push((id, parent, field, ty, Vec::new()));
            self.depths.push(depth);
        }
        let row = &mut self.rows[self.used];
        row.0 = id;
        row.1 = parent;
        row.2 = field;
        row.3 = ty;
        row.4.clear();
        self.depths[self.used] = depth;
        self.used += 1;
        Ok(slot)
    }
    /// Append a source-local sparse id to a window. Depth is relative to this window's root;
    /// its parent may lie outside the window only at depth zero. Later parents must precede
    /// children. External and ordinal id modes cannot be mixed in one builder.
    pub fn external(
        &mut self,
        id: u32,
        parent: Option<u32>,
        field: u32,
        ty: u64,
        depth: u16,
        value: Option<&dyn Put>,
    ) -> Result<u32, WireError> {
        if self.external.is_none() {
            if self.used != 0 {
                return Err(WireError::Invalid("mixed snapshot id modes"));
            }
            self.external = Some(HashMap::new());
        }
        let indices = self.external.as_ref().unwrap();
        if indices.contains_key(&id) || parent == Some(id) {
            return Err(WireError::Invalid("duplicate or cyclic snapshot id"));
        }
        match parent.and_then(|parent| indices.get(&parent)) {
            Some(slot) if self.depths[*slot] + 1 == depth => {}
            None if depth == 0 => {}
            _ => return Err(WireError::Invalid("snapshot parent depth is inconsistent")),
        }
        let slot = self.push(id, parent, field, ty, depth)?;
        if let Some(value) = value {
            self.encode_value(slot, value)?;
        }
        self.external.as_mut().unwrap().insert(id, slot);
        Ok(id)
    }
    /// A plain value row, encoded through the same Wire implementation as an API hook value.
    pub fn value<T: Wire>(
        &mut self,
        parent: Option<u32>,
        field: u32,
        ty: u64,
        value: &T,
    ) -> Result<u32, WireError> {
        let id = self.node(parent, field, ty)?;
        self.encode_value(self.used - 1, value)?;
        Ok(id)
    }
    fn encode_value(&mut self, slot: usize, value: &dyn Put) -> Result<(), WireError> {
        let mut encoded = Writer::new();
        encoded.replace(std::mem::take(&mut self.rows[slot].4));
        encoded.limit(1_048_576 - self.bytes);
        encoded.put(value);
        let error = encoded.error();
        let len = encoded.bytes().len();
        self.rows[slot].4 = encoded.into_bytes();
        if let Some(error) = error {
            self.used -= 1;
            return Err(error);
        }
        self.bytes += len;
        Ok(())
    }
    /// Complete snapshot, moved into the hook call's input and recycled after its return.
    pub fn finish(mut self) -> Built {
        Built {
            rows: std::mem::take(&mut self.rows),
            depths: std::mem::take(&mut self.depths),
            used: self.used,
            pooled: self.pooled,
        }
    }
}
