//! The wire form of the API's plain data, for the wasm boundary: values written into and read
//! from one buffer in the guest's linear memory. Native mods never use it.
//!
//! Everything is little-endian and self-delimiting: a length is a `u32` before its items, an
//! `Option` is a tag byte, an enum is a `u32` case then its fields, a record is its fields in
//! order. A reader trusts nothing: it checks every length against the bytes left and against the
//! smallest encoding of the items it promises, every tag against the cases the type has, strings
//! as UTF-8, and nesting against [`MAX_DEPTH`]. A bad input is a [`WireError`], never a panic and
//! never an allocation larger than the input can back.
//!
//! The impls for the API's own types are generated from their definitions, so a type crosses
//! exactly as it is written and a field added to it crosses with no other change.
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::hash::Hash;
use std::sync::Arc;

pub use crate::wire_slots::{Id, Slots, Stale};
#[path = "wire_resources.rs"]
mod resources;
pub use resources::{
    ResourceCodec, ResourceScope, ScopeError, ScopeLease, ScopeToken, Unsupported, borrow_token,
};
#[path = "wire_patch.rs"]
mod patch;
pub use patch::{
    DecodedPatchCell, PatchCell, PatchFields, PatchInput, PatchPlan, PatchSet, RecordPatch,
};

/// The guest's start-up call: its input is the package list.
pub const INIT: u32 = 0xFFFF_FF00;
/// The guest's call to drop the handlers the game let go of: its input is their ids.
pub const RELEASE: u32 = 0xFFFF_FF01;
/// The game's import freeing an object the mod held: its input is the id.
pub const HOST_RELEASE: u32 = 0xFFFF_FF02;
/// The game's import taking a message from the mod (a panic's text): its input is a string.
pub const HOST_LOG: u32 = 0xFFFF_FF03;

/// The layout of every value and call that crosses, a hash the generator writes: a mod built
/// for another one is refused when it loads.
pub use crate::WIRE_LAYOUT as LAYOUT;

#[cfg(any(target_arch = "wasm32", feature = "wasm-guest"))]
pub use crate::wire_guest as guest;

/// Maximum static strings retained by either transport string table.
pub const MAX_STATIC_STRINGS: usize = 4096;
/// Maximum bytes in one static API string (the existing interning bound).
pub const MAX_STATIC_STRING_BYTES: usize = 256;

/// Intern a static API string with bounded process-lifetime storage.
pub fn intern(value: &str) -> Result<&'static str, WireError> {
    use std::sync::{Mutex, OnceLock};
    static STRINGS: OnceLock<Mutex<HashSet<&'static str>>> = OnceLock::new();
    if value.len() > MAX_STATIC_STRING_BYTES {
        return Err(WireError::Invalid("static string too long"));
    }
    let mut strings = STRINGS
        .get_or_init(|| Mutex::new(HashSet::new()))
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some(&existing) = strings.get(value) {
        return Ok(existing);
    }
    if strings.len() >= MAX_STATIC_STRINGS {
        return Err(WireError::Invalid("too many static strings"));
    }
    let value = Box::leak(value.to_owned().into_boxed_str());
    strings.insert(value);
    Ok(value)
}

/// The deepest nesting of one value (a component of a component of…) a reader accepts.
///
/// It must not refuse anything its type accepts natively. The deepest such value is a `Tag`:
/// vanilla allows 512 nested lists or compounds, with a leaf in the innermost, and a nested
/// compound costs two levels here (the tag, then its map), so 2 × 512 + 1 for the leaf, with a
/// little headroom for the value that carries the tag.
pub const MAX_DEPTH: u16 = 1040;

/// Why a buffer is not a value of the type it was read as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum WireError {
    /// The buffer ends inside a value.
    Eof,
    /// A string is not UTF-8.
    Utf8,
    /// An enum case or option tag the type does not have.
    Tag(u32),
    /// A length longer than the rest of the buffer could hold.
    Length,
    /// Values nested deeper than [`MAX_DEPTH`].
    Depth,
    /// A value its type refuses (an unknown key, a count out of range), and why.
    Invalid(&'static str),
    /// Bytes left over after the value.
    Trailing,
}

impl std::fmt::Display for WireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Eof => f.write_str("the data ends inside a value"),
            Self::Utf8 => f.write_str("a string is not UTF-8"),
            Self::Tag(tag) => write!(f, "case {tag} does not exist"),
            Self::Length => f.write_str("a length is longer than the data"),
            Self::Depth => f.write_str("values are nested too deeply"),
            Self::Invalid(why) => f.write_str(why),
            Self::Trailing => f.write_str("data left over after the value"),
        }
    }
}

impl std::error::Error for WireError {}

// exhaustive: wasm boundary plumbing that generated code builds field by field
/// A buffer values are written to. Reuse one to keep the allocation.
#[derive(Default)]
pub struct Writer {
    bytes: Vec<u8>,
    error: Option<WireError>,
    limit: Option<usize>,
}

impl Writer {
    pub fn new() -> Self {
        Self::default()
    }
    /// Empties it, keeping its allocation and resetting any per-call error and byte limit.
    pub fn clear(&mut self) {
        self.bytes.clear();
        self.error = None;
        self.limit = None;
    }
    /// Takes `buffer` as its storage, emptied, resetting the error and byte limit.
    pub fn replace(&mut self, mut buffer: Vec<u8>) {
        buffer.clear();
        self.bytes = buffer;
        self.error = None;
        self.limit = None;
    }
    /// The bytes written, leaving the writer's storage to the caller.
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    /// Bound this call's encoded bytes. A refusal is recorded before any growth or copy.
    /// A new writer has no limit; `clear` and `replace` remove it.
    pub fn limit(&mut self, max_bytes: usize) {
        self.limit = Some(max_bytes);
        self.can_fit(0);
    }
    fn can_fit(&mut self, bytes: usize) -> bool {
        if self.error.is_some() {
            return false;
        }
        let fits = self.bytes.len().checked_add(bytes).is_some_and(|total| {
            total <= self.limit.unwrap_or(isize::MAX as usize) && total <= isize::MAX as usize
        });
        if !fits {
            self.fail(WireError::Length);
        }
        fits
    }
    pub fn put_bytes(&mut self, bytes: &[u8]) {
        if self.can_fit(bytes.len()) {
            self.bytes.extend_from_slice(bytes);
        }
    }
    /// A string: its length, then its bytes.
    pub fn put_str(&mut self, text: &str) {
        let Some(bytes) = text.len().checked_add(4) else {
            self.fail(WireError::Length);
            return;
        };
        if !self.can_fit(bytes) {
            return;
        }
        self.put_len(text.len());
        self.put_bytes(text.as_bytes());
    }
    /// A list of `u16`: its count, then each item, in one pass (the bytes of `Vec<u16>`'s own form).
    pub(crate) fn put_u16s(&mut self, items: &[u16]) {
        let Some(bytes) = items.len().checked_mul(2).and_then(|n| n.checked_add(4)) else {
            self.fail(WireError::Length);
            return;
        };
        if !self.can_fit(bytes) {
            return;
        }
        self.put_len(items.len());
        let start = self.bytes.len();
        self.bytes.resize(start + items.len() * 2, 0);
        for (to, item) in self.bytes[start..]
            .as_chunks_mut::<2>()
            .0
            .iter_mut()
            .zip(items)
        {
            to.copy_from_slice(&item.to_le_bytes());
        }
    }
    /// A length or count.
    pub fn put_len(&mut self, len: usize) {
        match u32::try_from(len) {
            Ok(len) => self.put(&len),
            Err(_) => self.fail(WireError::Length),
        }
    }
    /// An optional borrowed value, written as the `Option` it stands for.
    pub fn put_opt<T: Put + ?Sized>(&mut self, value: Option<&T>) {
        match value {
            Some(value) => {
                self.put(&1u8);
                self.put(value);
            }
            None => self.put(&0u8),
        }
    }
    pub fn put<T: Put + ?Sized>(&mut self, value: &T) {
        if self.error.is_none() {
            value.put_to(self);
        }
    }
    /// Transfers owned resources while leaving plain data on the normal zero-copy path.
    pub fn put_owned<T: Wire>(&mut self, value: T) {
        if self.error.is_none() {
            value.put_owned(self);
        }
    }
    /// The first encoding failure; callers refuse the call rather than sending partial bytes.
    pub fn error(&self) -> Option<WireError> {
        self.error
    }
    pub fn fail(&mut self, error: WireError) {
        self.error.get_or_insert(error);
    }
    /// Resolves one resource, reserving its wire width before registry mutation.
    /// Any failure is recorded before returning, so partial messages are never sent.
    pub fn resource(&mut self, value: Box<dyn std::any::Any>) -> Result<Id, WireError> {
        self.can_fit(<Id as Wire>::MIN);
        if let Some(error) = self.error {
            return Err(error);
        }
        let result = resources::store(value);
        if let Err(error) = result {
            self.fail(error)
        }
        result
    }
    /// Reports a deliberate erased-view refusal without changing the native Option contract.
    pub fn refused(&self, handler: &str) {
        resources::refused(handler);
    }
    pub fn put_resource(&mut self, value: Box<dyn std::any::Any>) {
        if let Ok(id) = self.resource(value) {
            self.put(&id)
        }
    }
}

/// A mutable hook argument, erased without changing its wire representation.
/// Replies are decoded completely before any field is assigned.
pub trait Record {
    /// Writes the argument into a hook call.
    fn encode(&self, out: &mut Writer);
    /// Applies one complete reply, refusing malformed or trailing bytes atomically.
    fn apply(&mut self, bytes: &[u8]) -> Result<(), WireError>;
}

// exhaustive: wasm boundary plumbing that generated code builds field by field
/// A record projected from caller-owned state without a second wire DTO. Both closures may
/// borrow the same interior-mutable native state; the apply closure must validate every edit
/// before committing it, exactly as required by `Record::apply`.
pub struct Projected<E, A> {
    encode: E,
    apply: A,
}
impl<E, A> Projected<E, A> {
    /// Bind an ordinary encoder and an atomic reply application to their caller-owned state.
    pub fn new(encode: E, apply: A) -> Self {
        Self { encode, apply }
    }
}
impl<E, A> Record for Projected<E, A>
where
    E: Fn(&mut Writer),
    A: FnMut(&[u8]) -> Result<(), WireError>,
{
    fn encode(&self, out: &mut Writer) {
        (self.encode)(out);
    }
    fn apply(&mut self, bytes: &[u8]) -> Result<(), WireError> {
        (self.apply)(bytes)
    }
}

// exhaustive: wasm boundary plumbing that generated code builds field by field
/// A call record refused before it can be encoded, such as an oversized owned snapshot.
/// It follows the normal Writer error path instead of sending a truncated call.
pub struct Refused(pub WireError);
impl Record for Refused {
    fn encode(&self, out: &mut Writer) {
        out.fail(self.0);
    }
    fn apply(&mut self, _: &[u8]) -> Result<(), WireError> {
        Err(self.0)
    }
}

impl<T: Wire> Record for T {
    fn encode(&self, out: &mut Writer) {
        out.put(self);
    }
    fn apply(&mut self, bytes: &[u8]) -> Result<(), WireError> {
        let mut input = Reader::new(bytes);
        let value = input.take()?;
        if !input.rest().is_empty() {
            return Err(WireError::Trailing);
        }
        *self = value;
        Ok(())
    }
}

// exhaustive: wasm boundary plumbing that generated code builds field by field
/// A hook record's writable fields, in declaration order. Sharing these tuple codecs avoids
/// generating and compiling a separate native codec for every named point record.
pub struct Fields<T>(pub T);

// exhaustive: wasm boundary plumbing that generated code builds field by field
/// A field which crosses with a record but is not a declared writeback destination.
pub struct ReadOnly<T>(pub T);

// hidden: wasm boundary plumbing used by generated code, not a mod's API
#[doc(hidden)]
pub trait FieldSet {
    type Value;
    fn encode(&self, out: &mut Writer);
    fn take(input: &mut Reader<'_>) -> Result<Self::Value, WireError>;
    fn commit(&mut self, value: Self::Value);
}

impl FieldSet for () {
    type Value = ();
    fn encode(&self, _: &mut Writer) {}
    fn take(_: &mut Reader<'_>) -> Result<(), WireError> {
        Ok(())
    }
    fn commit(&mut self, _: ()) {}
}

impl<H: Wire, T: FieldSet> FieldSet for (&mut H, T) {
    type Value = (H, T::Value);
    fn encode(&self, out: &mut Writer) {
        out.put(&*self.0);
        self.1.encode(out);
    }
    fn take(input: &mut Reader<'_>) -> Result<Self::Value, WireError> {
        Ok((input.take()?, T::take(input)?))
    }
    fn commit(&mut self, (head, tail): Self::Value) {
        *self.0 = head;
        self.1.commit(tail);
    }
}

impl<H: Wire, T: FieldSet> FieldSet for (ReadOnly<&H>, T) {
    type Value = (H, T::Value);
    fn encode(&self, out: &mut Writer) {
        out.put(self.0.0);
        self.1.encode(out);
    }
    fn take(input: &mut Reader<'_>) -> Result<Self::Value, WireError> {
        Ok((input.take()?, T::take(input)?))
    }
    fn commit(&mut self, (_, tail): Self::Value) {
        self.1.commit(tail);
    }
}

impl<T: FieldSet> Record for Fields<T> {
    fn encode(&self, out: &mut Writer) {
        self.0.encode(out);
    }
    fn apply(&mut self, bytes: &[u8]) -> Result<(), WireError> {
        let mut input = Reader::new(bytes);
        let value = T::take(&mut input)?;
        if !input.rest().is_empty() {
            return Err(WireError::Trailing);
        }
        self.0.commit(value);
        Ok(())
    }
}

// exhaustive: wasm boundary plumbing that generated code builds field by field
// hidden: wasm boundary plumbing used by generated code, not a mod's API
/// A checked append-only delta. Only the API's type-owned validation can create it.
#[doc(hidden)]
pub struct Append<T>(Vec<T>);
impl<T> Append<T> {
    pub(crate) fn new(values: Vec<T>) -> Self {
        Self(values)
    }
    pub(crate) fn into_vec(self) -> Vec<T> {
        self.0
    }
}
/// What can be written: every [`Wire`] value, and the borrowed forms a call passes
/// (`&str`, `&[T]`, `Option<&T>`), which are written as the owned value they stand for.
pub trait Put {
    /// Writes it.
    fn put_to(&self, out: &mut Writer);
}

impl<T: Wire> Put for T {
    fn put_to(&self, out: &mut Writer) {
        self.put(out);
    }
}

impl Put for str {
    fn put_to(&self, out: &mut Writer) {
        out.put_str(self);
    }
}

impl Wire for &str {
    const MIN: usize = 4;
    fn put(&self, out: &mut Writer) {
        out.put_str(self);
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        intern(&input.take::<String>()?)
    }
}

impl<T: Put> Put for [T] {
    fn put_to(&self, out: &mut Writer) {
        out.put_len(self.len());
        for item in self {
            out.put(item);
        }
    }
}

// exhaustive: wasm boundary plumbing that generated code builds field by field
/// A buffer values are read from.
pub struct Reader<'a> {
    data: &'a [u8],
    depth: u16,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, depth: 0 }
    }
    /// The bytes not yet read.
    pub fn rest(&self) -> &'a [u8] {
        self.data
    }
    pub fn take_bytes(&mut self, len: usize) -> Result<&'a [u8], WireError> {
        let (head, tail) = self.data.split_at_checked(len).ok_or(WireError::Eof)?;
        self.data = tail;
        Ok(head)
    }
    /// A length read before `items` values of at least `min` bytes each: refused when the rest
    /// of the buffer could not hold them, so no allocation outruns its input.
    pub fn take_len(&mut self, min: usize) -> Result<usize, WireError> {
        let len = u32::take(self)? as usize;
        match len.checked_mul(min.max(1)) {
            Some(needed) if needed <= self.data.len() || min == 0 && len <= self.data.len() => {
                Ok(len)
            }
            _ => Err(WireError::Length),
        }
    }
    pub fn take<T: Wire>(&mut self) -> Result<T, WireError> {
        T::take(self)
    }
    /// Read one value from at most `max_bytes`, preserving the enclosing reader's depth and
    /// leaving the following value untouched. No nested allocation can outrun this byte window.
    pub fn bounded<T>(
        &mut self,
        max_bytes: usize,
        read: impl FnOnce(&mut Self) -> Result<T, WireError>,
    ) -> Result<T, WireError> {
        let data = self.data;
        let len = data.len().min(max_bytes);
        let mut inner = Self {
            data: &data[..len],
            depth: self.depth,
        };
        let value = read(&mut inner);
        self.data = &data[len - inner.data.len()..];
        value
    }
    /// A list of `u16` written by [`Writer::put_u16s`], read in one pass: the length is checked
    /// against the bytes left before anything is allocated.
    pub(crate) fn take_u16s(&mut self) -> Result<Vec<u16>, WireError> {
        let len = self.take_len(2)?;
        let bytes = self.take_bytes(len * 2)?;
        Ok(bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .collect())
    }
    /// A normal Wire list, with its element cap checked before allocating storage. `read`
    /// may impose a shared payload budget across its elements without a second list codec.
    pub fn list_with<T: Wire>(
        &mut self,
        max_items: usize,
        mut read: impl FnMut(&mut Self) -> Result<T, WireError>,
    ) -> Result<Vec<T>, WireError> {
        let len = self.take_len(T::MIN)?;
        if len > max_items {
            return Err(WireError::Length);
        }
        let mut items = Vec::with_capacity(len);
        for _ in 0..len {
            items.push(read(self)?);
        }
        Ok(items)
    }
    pub fn take_resource<T: 'static>(&mut self) -> Result<T, WireError> {
        resources::load(self.take()?)
    }
    /// Reads one level down: the depth the value may nest to is [`MAX_DEPTH`].
    pub fn nested<T>(
        &mut self,
        read: impl FnOnce(&mut Self) -> Result<T, WireError>,
    ) -> Result<T, WireError> {
        if self.depth >= MAX_DEPTH {
            return Err(WireError::Depth);
        }
        self.depth += 1;
        let value = read(self);
        self.depth -= 1;
        value
    }
    /// The whole of `data` as one `T`.
    pub fn decode<T: Wire>(data: &'a [u8]) -> Result<T, WireError> {
        let mut reader = Self::new(data);
        let value = T::take(&mut reader)?;
        match reader.data.is_empty() {
            true => Ok(value),
            false => Err(WireError::Trailing),
        }
    }
}

/// A type with a wire form.
pub trait Wire: Sized {
    /// The fewest bytes a value takes, so a length can be checked before anything is allocated.
    const MIN: usize;
    fn put(&self, out: &mut Writer);
    /// Consumes owned data when a resource's ownership must cross the boundary.
    fn put_owned(self, out: &mut Writer) {
        self.put(out);
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError>;
}

macro_rules! numbers {
    ($($ty:ty),*) => {$(
        impl Wire for $ty {
            const MIN: usize = std::mem::size_of::<$ty>();
            fn put(&self, out: &mut Writer) {
                out.put_bytes(&self.to_le_bytes());
            }
            fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
                let bytes = input.take_bytes(std::mem::size_of::<$ty>())?;
                Ok(<$ty>::from_le_bytes(bytes.try_into().expect("a fixed-size slice")))
            }
        }
    )*};
}
numbers!(u8, i8, u16, i16, u32, i32, u64, i64, u128, i128, f32, f64);

/// A `usize` crosses as 64 bits: a guest's is 32.
impl Wire for usize {
    const MIN: usize = 8;
    fn put(&self, out: &mut Writer) {
        out.put(&(*self as u64));
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        usize::try_from(u64::take(input)?).map_err(|_| WireError::Invalid("a size too large"))
    }
}

impl Wire for bool {
    const MIN: usize = 1;
    fn put(&self, out: &mut Writer) {
        out.put(&u8::from(*self));
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        match u8::take(input)? {
            0 => Ok(false),
            1 => Ok(true),
            tag => Err(WireError::Tag(tag.into())),
        }
    }
}

impl Wire for char {
    const MIN: usize = 4;
    fn put(&self, out: &mut Writer) {
        out.put(&u32::from(*self));
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        char::from_u32(u32::take(input)?).ok_or(WireError::Invalid("not a character"))
    }
}

impl Wire for () {
    const MIN: usize = 0;
    fn put(&self, _: &mut Writer) {}
    fn take(_: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(())
    }
}

impl Wire for String {
    const MIN: usize = 4;
    fn put(&self, out: &mut Writer) {
        out.put_str(self);
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        let len = input.take_len(1)?;
        let bytes = input.take_bytes(len)?;
        std::str::from_utf8(bytes)
            .map(str::to_owned)
            .map_err(|_| WireError::Utf8)
    }
}

/// A mod's own text, owned once read: a `Cow<'static, str>` field is how the API takes either.
impl Wire for std::borrow::Cow<'_, str> {
    const MIN: usize = 4;
    fn put(&self, out: &mut Writer) {
        out.put_str(self);
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        String::take(input).map(Self::Owned)
    }
}

impl Wire for Box<str> {
    const MIN: usize = 4;
    fn put(&self, out: &mut Writer) {
        out.put_str(self);
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        String::take(input).map(String::into_boxed_str)
    }
}

impl Wire for Arc<str> {
    const MIN: usize = 4;
    fn put(&self, out: &mut Writer) {
        out.put_str(self);
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        String::take(input).map(Arc::from)
    }
}

impl<T: Wire> Wire for Option<T> {
    const MIN: usize = 1;
    fn put(&self, out: &mut Writer) {
        match self {
            None => out.put(&0u8),
            Some(value) => {
                out.put(&1u8);
                out.put(value);
            }
        }
    }
    fn put_owned(self, out: &mut Writer) {
        match self {
            Some(value) => {
                out.put(&1u8);
                out.put_owned(value);
            }
            None => out.put(&0u8),
        }
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        match u8::take(input)? {
            0 => Ok(None),
            1 => T::take(input).map(Some),
            tag => Err(WireError::Tag(tag.into())),
        }
    }
}

impl<T: Wire, E: Wire> Wire for Result<T, E> {
    const MIN: usize = 1;
    fn put(&self, out: &mut Writer) {
        match self {
            Ok(value) => {
                out.put(&0u8);
                out.put(value);
            }
            Err(error) => {
                out.put(&1u8);
                out.put(error);
            }
        }
    }
    fn put_owned(self, out: &mut Writer) {
        match self {
            Ok(value) => {
                out.put(&0u8);
                out.put_owned(value);
            }
            Err(error) => {
                out.put(&1u8);
                out.put_owned(error);
            }
        }
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        match u8::take(input)? {
            0 => T::take(input).map(Ok),
            1 => E::take(input).map(Err),
            tag => Err(WireError::Tag(tag.into())),
        }
    }
}

impl<T: Wire> Wire for Arc<[T]> {
    const MIN: usize = 4;
    fn put(&self, out: &mut Writer) {
        out.put_len(self.len());
        for item in self.iter() {
            out.put(item);
        }
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Vec::<T>::take(input).map(Into::into)
    }
}

impl<T: Wire> Wire for Box<[T]> {
    const MIN: usize = 4;
    fn put(&self, out: &mut Writer) {
        out.put_len(self.len());
        for item in self.iter() {
            out.put(item);
        }
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Vec::<T>::take(input).map(Into::into)
    }
}

impl<T: Wire> Wire for Box<T> {
    const MIN: usize = T::MIN;
    fn put(&self, out: &mut Writer) {
        (**self).put(out);
    }
    fn put_owned(self, out: &mut Writer) {
        out.put_owned(*self);
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        input.nested(T::take).map(Box::new)
    }
}

impl<T: Wire> Wire for Arc<T> {
    const MIN: usize = T::MIN;
    fn put(&self, out: &mut Writer) {
        (**self).put(out);
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        input.nested(T::take).map(Arc::new)
    }
}

impl<T: Wire> Wire for Vec<T> {
    const MIN: usize = 4;
    fn put(&self, out: &mut Writer) {
        out.put_len(self.len());
        for item in self {
            item.put(out);
        }
    }
    fn put_owned(self, out: &mut Writer) {
        out.put_len(self.len());
        for item in self {
            out.put_owned(item);
        }
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        input.list_with(usize::MAX, T::take)
    }
}

impl<T: Wire> Wire for VecDeque<T> {
    const MIN: usize = 4;
    fn put(&self, out: &mut Writer) {
        out.put_len(self.len());
        for item in self {
            item.put(out);
        }
    }
    fn put_owned(self, out: &mut Writer) {
        out.put_len(self.len());
        for item in self {
            out.put_owned(item);
        }
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        Vec::take(input).map(Into::into)
    }
}

impl<T: Wire, const N: usize> Wire for [T; N] {
    const MIN: usize = T::MIN * N;
    fn put(&self, out: &mut Writer) {
        for item in self {
            item.put(out);
        }
    }
    fn put_owned(self, out: &mut Writer) {
        for item in self {
            out.put_owned(item);
        }
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        let mut items = Vec::with_capacity(N);
        for _ in 0..N {
            items.push(T::take(input)?);
        }
        items
            .try_into()
            .map_err(|_| WireError::Invalid("an array of the wrong length"))
    }
}

/// Sets and maps cross as a list of their entries; a key that repeats keeps the last.
macro_rules! sets {
    ($($ty:ident [$($bound:tt)+]),*) => {$(
        impl<T: Wire + $($bound)+> Wire for $ty<T> {
            const MIN: usize = 4;
            fn put(&self, out: &mut Writer) {
                out.put_len(self.len());
                for item in self {
                    item.put(out);
                }
            }
            fn put_owned(self,out:&mut Writer) { out.put_len(self.len()); for item in self { out.put_owned(item); } }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
                let len = input.take_len(T::MIN)?;
                let mut set = Self::default();
                for _ in 0..len {
                    set.insert(T::take(input)?);
                }
                Ok(set)
            }
        }
    )*};
}
sets!(BTreeSet[Ord], HashSet[Eq + Hash]);

macro_rules! maps {
    ($($ty:ident [$($bound:tt)+]),*) => {$(
        impl<K: Wire + $($bound)+, V: Wire> Wire for $ty<K, V> {
            const MIN: usize = 4;
            fn put(&self, out: &mut Writer) {
                out.put_len(self.len());
                for (key, value) in self {
                    key.put(out);
                    value.put(out);
                }
            }
            fn put_owned(self,out:&mut Writer) { out.put_len(self.len()); for (key,value) in self { out.put_owned(key); out.put_owned(value); } }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
                let len = input.take_len(K::MIN + V::MIN)?;
                let mut map = Self::default();
                for _ in 0..len {
                    let key = K::take(input)?;
                    map.insert(key, V::take(input)?);
                }
                Ok(map)
            }
        }
    )*};
}
maps!(BTreeMap[Ord], HashMap[Eq + Hash]);

macro_rules! tuples {
    ($(($($name:ident),+)),*) => {$(
        #[allow(non_snake_case)]
        impl<$($name: Wire),+> Wire for ($($name,)+) {
            const MIN: usize = 0 $(+ $name::MIN)+;
            fn put(&self, out: &mut Writer) {
                let ($($name,)+) = self;
                $($name.put(out);)+
            }
            fn put_owned(self,out:&mut Writer) { let ($($name,)+)=self; $($name.put_owned(out);)+ }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
                Ok(($($name::take(input)?,)+))
            }
        }
    )*};
}
tuples!(
    (A),
    (A, B),
    (A, B, C),
    (A, B, C, D),
    (A, B, C, D, E),
    (A, B, C, D, E, F)
);

/// A duration as seconds and nanoseconds.
impl Wire for std::time::Duration {
    const MIN: usize = 12;
    fn put(&self, out: &mut Writer) {
        out.put(&self.as_secs());
        out.put(&self.subsec_nanos());
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        let secs = u64::take(input)?;
        let nanos = u32::take(input)?;
        match nanos < 1_000_000_000 {
            true => Ok(Self::new(secs, nanos)),
            false => Err(WireError::Invalid("nanoseconds past a second")),
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn resource_context_is_scoped_and_encoding_keeps_first_error() {
        use std::{
            any::{Any, TypeId},
            cell::RefCell,
            rc::Rc,
        };
        struct Codec(RefCell<Option<Box<dyn Any>>>);
        impl super::ResourceCodec for Codec {
            fn store(&self, value: Box<dyn Any>) -> Result<super::Id, super::WireError> {
                *self.0.borrow_mut() = Some(value);
                Ok(super::Id(7))
            }
            fn load(&self, id: super::Id, _: TypeId) -> Result<Box<dyn Any>, super::WireError> {
                if id != super::Id(7) {
                    return Err(super::WireError::Invalid("stale resource"));
                }
                self.0
                    .borrow_mut()
                    .take()
                    .ok_or(super::WireError::Invalid("stale resource"))
            }
        }
        let mut out = super::Writer::new();
        {
            let _scope = super::ResourceScope::new(Rc::new(Codec(RefCell::new(None))));
            out.put_resource(Box::new(42u32));
            assert_eq!(out.error(), None);
            assert_eq!(
                super::Reader::new(out.bytes()).take_resource::<u32>(),
                Ok(42)
            );
        }
        assert_eq!(
            super::Reader::new(out.bytes()).take_resource::<u32>(),
            Err(super::WireError::Invalid("no resource context"))
        );
        out.fail(super::WireError::Trailing);
        out.put_resource(Box::new(43u32));
        assert_eq!(out.error(), Some(super::WireError::Trailing));
        out.clear();
        assert_eq!(out.error(), None);
    }
    #[test]
    fn static_string_rows_roundtrip_through_intern() {
        let rows: Vec<(&'static str, &'static str)> =
            vec![("test:block", "test:glow"), ("test:block", "test:shade")];
        let mut out = super::Writer::new();
        out.put(&rows);
        let decoded =
            super::Reader::decode::<Vec<(&'static str, &'static str)>>(out.bytes()).unwrap();
        assert_eq!(decoded, rows);
        assert!(std::ptr::eq(decoded[0].0.as_ptr(), decoded[1].0.as_ptr()));
        let mut oversized = super::Writer::new();
        oversized.put(&"x".repeat(257));
        assert_eq!(
            super::Reader::decode::<&'static str>(oversized.bytes()),
            Err(super::WireError::Invalid("static string too long"))
        );
    }
    use super::*;

    fn round<T: Wire + PartialEq + std::fmt::Debug>(value: T) {
        let mut out = Writer::new();
        out.put(&value);
        assert_eq!(Reader::decode::<T>(out.bytes()), Ok(value));
    }

    #[test]
    fn reduction_adjustments_survive_wire_in_order() {
        use crate::{Reduction::*, ReductionChange::*};
        let changes = vec![
            (Armor, Add(2.5)),
            (Enchantments, Scale(0.5)),
            (MobEffects, AtLeast(3.0)),
            (Absorption, AtMost(4.0)),
            (Invulnerability, Add(-1.0)),
            (InnateResistance, Scale(2.0)),
        ];
        round(changes);
        assert!(Reader::decode::<crate::Reduction>(&[6]).is_err());
        assert!(Reader::decode::<crate::ReductionChange>(&[4, 0, 0, 0, 0]).is_err());
    }

    #[test]
    fn provider_particle_state_survives_the_wire() {
        round(crate::ModParticle {
            position: [1.25, -2.5, 3.75],
            previous: [0.0, -1.0, 2.0],
            velocity: [0.1, 0.2, -0.3],
            age: 7,
            lifetime: 31,
            size: 0.5,
            rgba: [0.25, 0.5, 0.75, 1.0],
            frame: 3,
            removed: true,
            state: [8.0, -9.0, 10.0, 11.0],
        });
    }

    #[test]
    fn values_survive_a_round_trip() {
        round(7u32);
        round(-3i64);
        round(1.5f64);
        round(u128::MAX);
        round(true);
        round('x');
        round(String::from("héllo"));
        round(Some(vec![1u8, 2, 3]));
        round(None::<u16>);
        round((1u8, String::from("a"), [3u16; 4]));
        round(BTreeMap::from([
            (1u8, String::from("one")),
            (2, String::from("two")),
        ]));
        round(std::time::Duration::new(5, 7));
        round(vec![Some(Box::new(4i32)), None]);
    }

    #[test]
    fn bad_input_is_an_error_not_a_panic_or_a_huge_allocation() {
        // A list that promises four billion items in six bytes.
        assert_eq!(
            Reader::decode::<Vec<u64>>(&[0xff, 0xff, 0xff, 0xff, 0, 0]),
            Err(WireError::Length)
        );
        assert_eq!(Reader::decode::<u32>(&[1, 2]), Err(WireError::Eof));
        assert_eq!(Reader::decode::<bool>(&[2]), Err(WireError::Tag(2)));
        assert_eq!(Reader::decode::<Option<u8>>(&[9]), Err(WireError::Tag(9)));
        assert_eq!(
            Reader::decode::<String>(&[2, 0, 0, 0, 0xff, 0xfe]),
            Err(WireError::Utf8)
        );
        assert_eq!(Reader::decode::<u8>(&[1, 2]), Err(WireError::Trailing));
        assert!(Reader::decode::<char>(&0xd800u32.to_le_bytes()).is_err());
        assert!(
            Reader::decode::<std::time::Duration>(
                &[0; 8]
                    .iter()
                    .chain(&[0xff; 4])
                    .copied()
                    .collect::<Vec<u8>>()
            )
            .is_err()
        );
    }

    #[test]
    fn nesting_past_the_limit_is_refused() {
        let mut bytes = vec![1u8; MAX_DEPTH as usize + 1];
        bytes.push(0);
        let mut shallow = bytes[bytes.len() - MAX_DEPTH as usize..].to_vec();
        shallow.push(0);
        // `Box<Option<…>>` chains are not a real type; a depth guard is exercised directly.
        let mut reader = Reader::new(&bytes);
        fn dive(reader: &mut Reader<'_>, left: usize) -> Result<(), WireError> {
            match left {
                0 => Ok(()),
                _ => reader.nested(|r| dive(r, left - 1)),
            }
        }
        assert_eq!(dive(&mut reader, MAX_DEPTH as usize), Ok(()));
        assert_eq!(
            dive(&mut reader, MAX_DEPTH as usize + 1),
            Err(WireError::Depth)
        );
        // The depth unwinds: a refused read leaves the reader reusable.
        assert_eq!(dive(&mut reader, 3), Ok(()));
    }
}
