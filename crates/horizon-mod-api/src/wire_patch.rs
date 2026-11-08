//! Sparse updates to ordinary Wire fields. The schema owns field ids and native destinations;
//! no engine type, address or layout is part of a patch.
use super::{Reader, Record, Wire, WireError, Writer};

type CellBytes = (u32, Vec<u8>);
const MAX_CELLS: usize = 65_536;
const MAX_VALUES: usize = 1_048_576;
const MAX_PACKET: usize = 4 + MAX_CELLS * <CellBytes as Wire>::MIN + MAX_VALUES;

// exhaustive: wasm boundary plumbing that generated code builds field by field
/// A sparse field update record. Entries are unique and sorted by their schema's field ids.
/// Each value uses its ordinary Wire codec; the receiving schema checks its concrete type.
#[derive(Default)]
pub struct RecordPatch {
    cells: Vec<CellBytes>,
    bytes: usize,
}
impl RecordPatch {
    /// Add a pre-encoded ordinary Wire value, preserving the patch if its limits are exceeded.
    /// The receiving schema validates its type, field id and write permission.
    pub fn set_encoded(&mut self, field: u32, value: &[u8]) -> Result<(), WireError> {
        let position = self.cells.binary_search_by_key(&field, |cell| cell.0);
        if position.is_err() && self.cells.len() == MAX_CELLS {
            return Err(WireError::Length);
        }
        let previous = position.as_ref().ok().map_or(0, |&i| self.cells[i].1.len());
        let remaining = MAX_VALUES - (self.bytes - previous);
        if value.len() > remaining {
            return Err(WireError::Length);
        }
        let value = value.to_vec();
        self.bytes = self.bytes - previous + value.len();
        match position {
            Ok(i) => self.cells[i].1 = value,
            Err(i) => self.cells.insert(i, (field, value)),
        }
        Ok(())
    }

    /// Iterate over source-local node identifiers and their ordinary Wire values.
    pub fn iter(&self) -> impl Iterator<Item = (u32, &[u8])> {
        self.cells.iter().map(|(id, bytes)| (*id, bytes.as_slice()))
    }

    /// Add or replace a field's encoded value, retaining the previous patch on refusal.
    /// The receiver, not the sender, owns the field id and write permission checks.
    pub fn set<T: Wire>(&mut self, field: u32, value: &T) -> Result<(), WireError> {
        let position = self.cells.binary_search_by_key(&field, |cell| cell.0);
        if position.is_err() && self.cells.len() == MAX_CELLS {
            return Err(WireError::Length);
        }
        let previous = position.as_ref().ok().map_or(0, |&i| self.cells[i].1.len());
        let remaining = MAX_VALUES - (self.bytes - previous);
        let mut out = Writer::new();
        out.limit(remaining);
        out.put(value);
        if let Some(error) = out.error() {
            return Err(error);
        }
        let bytes = out.into_bytes();
        self.bytes = self.bytes - previous + bytes.len();
        match position {
            Ok(i) => self.cells[i].1 = bytes,
            Err(i) => self.cells.insert(i, (field, bytes)),
        }
        Ok(())
    }
}
impl Wire for RecordPatch {
    const MIN: usize = <Vec<CellBytes> as Wire>::MIN;
    fn put(&self, out: &mut Writer) {
        out.put(&self.cells);
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        input.bounded(MAX_PACKET, |input| {
            let mut remaining = MAX_VALUES;
            let mut previous = None;
            let cells = input.list_with(MAX_CELLS, |input| {
                let cell: CellBytes =
                    input.bounded(remaining + CellBytes::MIN, |input| input.take())?;
                if previous.is_some_and(|previous| previous >= cell.0) {
                    return Err(WireError::Invalid(
                        "patch fields are not unique and ordered",
                    ));
                }
                previous = Some(cell.0);
                remaining -= cell.1.len();
                Ok(cell)
            })?;
            Ok(Self {
                cells,
                bytes: MAX_VALUES - remaining,
            })
        })
    }
}

// exhaustive: wasm boundary plumbing that generated code builds field by field
/// Prepared, typed assignments for one immediate apply transaction.
///
/// Planning allocates one closure per changed destination. Drop the plan on any error;
/// `commit` must only be called after every root has been validated and planned.
#[derive(Default)]
pub struct PatchPlan<'a> {
    commits: Vec<Box<dyn FnOnce() + 'a>>,
    error: Option<WireError>,
}
impl<'a> PatchPlan<'a> {
    /// Decode a complete value and retain its assignment without modifying the destination.
    pub fn prepare<T: 'a>(
        &mut self,
        destination: &'a mut T,
        bytes: &[u8],
        decode: impl FnOnce(&mut Reader<'_>) -> Result<T, WireError>,
    ) -> Result<(), WireError> {
        if let Some(error) = self.error {
            return Err(error);
        }
        let mut reader = Reader::new(bytes);
        let value = decode(&mut reader).and_then(|value| {
            if reader.rest().is_empty() {
                Ok(value)
            } else {
                Err(WireError::Trailing)
            }
        });
        match value {
            Ok(value) => {
                self.commits.push(Box::new(move || *destination = value));
                Ok(())
            }
            Err(error) => {
                self.error = Some(error);
                Err(error)
            }
        }
    }

    /// Apply all prepared assignments, unless any preparation failed.
    pub fn commit(self) -> Result<(), WireError> {
        if let Some(error) = self.error {
            return Err(error);
        }
        for commit in self.commits {
            commit();
        }
        Ok(())
    }
}

// exhaustive: wasm boundary plumbing that generated code builds field by field
/// A native destination paired with its schema's field id. The borrow remains local to the call.
pub struct PatchCell<'a, T> {
    field: u32,
    value: &'a mut T,
}
impl<'a, T> PatchCell<'a, T> {
    /// Bind a native field to the id emitted by its schema.
    pub fn new(field: u32, value: &'a mut T) -> Self {
        Self { field, value }
    }
}

// exhaustive: wasm boundary plumbing that generated code builds field by field
/// A native destination decoded by generated code without implementing `Wire` on its type.
/// The decoder must build an owned value and use the supplied reader for all nested input.
pub struct DecodedPatchCell<'a, T, F> {
    field: u32,
    value: &'a mut T,
    decode: F,
}
impl<'a, T, F> DecodedPatchCell<'a, T, F> {
    /// Bind a destination and its ordinary Wire decoder.
    pub fn new(field: u32, value: &'a mut T, decode: F) -> Self {
        Self {
            field,
            value,
            decode,
        }
    }
}

// exhaustive: wasm boundary plumbing that generated code builds field by field
// hidden: wasm boundary plumbing used by generated code, not a mod's API
/// The checked patch input used by generated field bindings.
#[doc(hidden)]
pub struct PatchInput<'a> {
    patch: &'a RecordPatch,
    used: usize,
}
impl PatchInput<'_> {
    fn take_with<T>(
        &mut self,
        field: u32,
        decode: impl FnOnce(&mut Reader<'_>) -> Result<T, WireError>,
    ) -> Result<Option<T>, WireError> {
        match self.patch.cells.binary_search_by_key(&field, |cell| cell.0) {
            Ok(i) => {
                let mut reader = Reader::new(&self.patch.cells[i].1);
                let value = decode(&mut reader)?;
                if !reader.rest().is_empty() {
                    return Err(WireError::Trailing);
                }
                self.used += 1;
                Ok(Some(value))
            }
            Err(_) => Ok(None),
        }
    }
}

// hidden: wasm boundary plumbing used by generated code, not a mod's API
/// A generated field list's typed plan and commit operations.
#[doc(hidden)]
pub trait PatchSet {
    type Plan;
    fn plan(&self, input: &mut PatchInput<'_>) -> Result<Self::Plan, WireError>;
    fn commit(&mut self, plan: Self::Plan);
}
impl PatchSet for () {
    type Plan = ();
    fn plan(&self, _: &mut PatchInput<'_>) -> Result<(), WireError> {
        Ok(())
    }
    fn commit(&mut self, _: ()) {}
}
impl<H: Wire, T: PatchSet> PatchSet for (PatchCell<'_, H>, T) {
    type Plan = (Option<H>, T::Plan);
    fn plan(&self, input: &mut PatchInput<'_>) -> Result<Self::Plan, WireError> {
        Ok((
            input.take_with(self.0.field, |reader| reader.take())?,
            self.1.plan(input)?,
        ))
    }
    fn commit(&mut self, (head, tail): Self::Plan) {
        if let Some(value) = head {
            *self.0.value = value;
        }
        self.1.commit(tail);
    }
}

impl<H, F, T: PatchSet> PatchSet for (DecodedPatchCell<'_, H, F>, T)
where
    F: Fn(&mut Reader<'_>) -> Result<H, WireError>,
{
    type Plan = (Option<H>, T::Plan);
    fn plan(&self, input: &mut PatchInput<'_>) -> Result<Self::Plan, WireError> {
        Ok((
            input.take_with(self.0.field, &self.0.decode)?,
            self.1.plan(input)?,
        ))
    }
    fn commit(&mut self, (head, tail): Self::Plan) {
        if let Some(value) = head {
            *self.0.value = value;
        }
        self.1.commit(tail);
    }
}

// exhaustive: wasm boundary plumbing that generated code builds field by field
/// A sparse-update hook record backed by typed native fields, without a second set of DTOs.
/// Decoding and planning finish before any destination changes. Unbound, readonly, malformed
/// and trailing cells refuse the whole patch. An empty patch leaves every destination untouched.
pub struct PatchFields<T> {
    fields: T,
    nodes: u32,
}
impl<T: PatchSet> PatchFields<T> {
    /// Bind a generated field list and its id domain. Every writable id occurs once in the list.
    pub fn new(fields: T, nodes: u32) -> Self {
        Self { fields, nodes }
    }
}
impl<T: PatchSet> Record for PatchFields<T> {
    fn encode(&self, out: &mut Writer) {
        out.put(&RecordPatch::default());
    }
    fn apply(&mut self, bytes: &[u8]) -> Result<(), WireError> {
        let patch = Reader::decode::<RecordPatch>(bytes)?;
        if patch.cells.last().is_some_and(|cell| cell.0 >= self.nodes) {
            return Err(WireError::Invalid("patch field is outside the schema"));
        }
        let mut input = PatchInput {
            patch: &patch,
            used: 0,
        };
        let plan = self.fields.plan(&mut input)?;
        if input.used != patch.cells.len() {
            return Err(WireError::Invalid("patch field is not writable"));
        }
        self.fields.commit(plan);
        Ok(())
    }
}
