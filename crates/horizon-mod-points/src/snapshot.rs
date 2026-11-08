//! Owned, flattened rows of one raw hook call. No row contains an engine address.
use mod_api::wire::{Put, Reader, Wire, WireError, Writer};
use std::{cell::RefCell, ops::Deref};

/// A row's node id, parent id, declaration-order field/index id, type key and encoded plain value.
/// A node without a plain value has an empty value buffer and is read through its child rows.
pub type Row = (u32, Option<u32>, u32, u32, Vec<u8>);
/// The raw call's snapshot. Generic tuple/list Wire impls carry it without a second codec.
pub type Rows = Vec<Row>;

/// A stable key for the type spellings in a generated view. Calls use constant keys, not text.
pub const fn key(name: &str) -> u32 {
    let bytes = name.as_bytes();
    let mut key = 0x811c9dc5u32;
    let mut i = 0;
    while i < bytes.len() {
        key = (key ^ bytes[i] as u32).wrapping_mul(0x01000193);
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
            .0
            .get(node as usize)
            .filter(|row| row.0 == node)
            .ok_or(WireError::Invalid("unknown snapshot node"))?;
        Reader::decode(&row.4)
    }
    /// The generated type key, resolved by the per-mod view rather than sent as text per call.
    pub fn ty(&self, node: u32) -> Option<u32> {
        self.0
            .get(node as usize)
            .filter(|row| row.0 == node)
            .map(|row| row.3)
    }
}

thread_local! { static POOL: RefCell<Vec<Rows>> = const { RefCell::new(Vec::new()) }; }
fn recycle(rows: Rows) {
    let cost = |rows: &Rows| {
        rows.capacity() * std::mem::size_of::<Row>()
            + rows.iter().map(|row| row.4.capacity()).sum::<usize>()
    };
    if rows.capacity() == 0 {
        return;
    }
    POOL.with(|pool| {
        let mut pool = pool.borrow_mut();
        if cost(&rows) + pool.iter().map(cost).sum::<usize>() <= 4_194_304 {
            pool.push(rows);
        }
    });
}

/// A completed snapshot, kept until its synchronous hook returns. Its buffers are pooled by
/// call depth, so steady-state primitive snapshots do not allocate even across different places.
pub struct Built {
    rows: Rows,
    used: usize,
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
        recycle(std::mem::take(&mut self.rows));
    }
}

/// Builds a per-call snapshot with the former internals view's cell and byte budgets.
/// Refusing an oversized snapshot never truncates a row or exposes a partial tree.
pub struct Builder {
    rows: Rows,
    bytes: usize,
    used: usize,
}
impl Default for Builder {
    fn default() -> Self {
        Self {
            rows: POOL
                .with(|pool| pool.borrow_mut().pop())
                .unwrap_or_default(),
            bytes: 0,
            used: 0,
        }
    }
}
impl Drop for Builder {
    fn drop(&mut self) {
        recycle(std::mem::take(&mut self.rows));
    }
}
impl Builder {
    /// Add an owned node. Node ids are assigned in row order.
    pub fn node(&mut self, parent: Option<u32>, field: u32, ty: u32) -> Result<u32, WireError> {
        if self.used >= 65_536 {
            return Err(WireError::Length);
        }
        let id = self.used as u32;
        if parent.is_some_and(|parent| parent >= id) {
            return Err(WireError::Invalid("snapshot parent is not an earlier node"));
        }
        if self.used == self.rows.len() {
            self.rows.push((id, parent, field, ty, Vec::new()));
        }
        let row = &mut self.rows[self.used];
        row.0 = id;
        row.1 = parent;
        row.2 = field;
        row.3 = ty;
        row.4.clear();
        self.used += 1;
        Ok(id)
    }
    /// A plain value row, encoded through the same Wire implementation as an API hook value.
    pub fn value<T: Wire>(
        &mut self,
        parent: Option<u32>,
        field: u32,
        ty: u32,
        value: &T,
    ) -> Result<u32, WireError> {
        let before = self.bytes;
        let id = self.node(parent, field, ty)?;
        let mut encoded = Writer::new();
        encoded.replace(std::mem::take(&mut self.rows[id as usize].4));
        encoded.limit(1_048_576 - self.bytes);
        encoded.put(value);
        let error = encoded.error();
        let len = encoded.bytes().len();
        self.rows[id as usize].4 = encoded.into_bytes();
        if let Some(error) = error {
            self.used -= 1;
            self.bytes = before;
            return Err(error);
        }
        self.bytes += len;
        Ok(id)
    }
    /// Complete snapshot, moved into the hook call's input and recycled after its return.
    pub fn finish(mut self) -> Built {
        Built {
            rows: std::mem::take(&mut self.rows),
            used: self.used,
        }
    }
}
