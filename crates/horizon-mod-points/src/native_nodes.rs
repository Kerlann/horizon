//! Borrowed projection adapters used by generated native readers.
use crate::{locators::Step, native_read::NativeNode};
use mod_api::{RowError, wire::Put};

/// Callback receiving a temporarily borrowed child projection.
pub type Visitor<'a> = dyn FnMut(Step, u32, &dyn NativeNode) -> Result<(), RowError> + 'a;
type Header<'a> = dyn FnMut(Option<&dyn Put>) -> Result<(), RowError> + 'a;

/// A plain Wire leaf, with its generated canonical schema key.
pub struct Scalar<'a, T: Put>(pub &'a T, pub u64);
impl<T: Put> NativeNode for Scalar<'_, T> {
    fn key(&self) -> u64 {
        self.1
    }
    fn value(&self, f: &mut Header<'_>) -> Result<(), RowError> {
        f(Some(self.0))
    }
    fn len(&self) -> u64 {
        0
    }
    fn child(&self, _: u64, _: &mut Visitor<'_>) -> Result<(), RowError> {
        Err(RowError::Bounds)
    }
}

/// A structural record whose child resolver is generated from its native layout.
pub struct Record<F> {
    pub key: u64,
    pub count: u64,
    pub visit: F,
}
impl<F> NativeNode for Record<F>
where
    F: Fn(u64, &mut Visitor<'_>) -> Result<(), RowError>,
{
    fn key(&self) -> u64 {
        self.key
    }
    fn value(&self, f: &mut Header<'_>) -> Result<(), RowError> {
        f(None)
    }
    fn len(&self) -> u64 {
        self.count
    }
    fn child(&self, n: u64, f: &mut Visitor<'_>) -> Result<(), RowError> {
        (self.visit)(n, f)
    }
}

/// A tagged enum node with its currently active variant as the single child.
pub struct Variant<F> {
    pub key: u64,
    pub tag: u32,
    pub visit: F,
}
impl<F> NativeNode for Variant<F>
where
    F: Fn(&mut Visitor<'_>) -> Result<(), RowError>,
{
    fn key(&self) -> u64 {
        self.key
    }
    fn value(&self, f: &mut Header<'_>) -> Result<(), RowError> {
        f(Some(&self.tag))
    }
    fn len(&self) -> u64 {
        1
    }
    fn child(&self, ordinal: u64, f: &mut Visitor<'_>) -> Result<(), RowError> {
        if ordinal != 0 {
            return Err(RowError::Bounds);
        }
        (self.visit)(&mut |_, _, node| f(Step::Variant(self.tag), self.tag, node))
    }
}

/// A borrowed UTF-16 view of a native UTF-8 string. Indexes are UTF-16 code units.
pub struct Text<'a>(pub &'a str, pub u64, pub u64);
impl NativeNode for Text<'_> {
    fn key(&self) -> u64 {
        self.1
    }
    fn value(&self, f: &mut Header<'_>) -> Result<(), RowError> {
        f(Some(&self.len()))
    }
    fn len(&self) -> u64 {
        self.0.encode_utf16().count() as u64
    }
    fn child(&self, ordinal: u64, f: &mut Visitor<'_>) -> Result<(), RowError> {
        let index = usize::try_from(ordinal).map_err(|_| RowError::Bounds)?;
        let value = self.0.encode_utf16().nth(index).ok_or(RowError::Bounds)?;
        f(
            Step::Index(ordinal),
            u32::try_from(ordinal).map_err(|_| RowError::Bounds)?,
            &Scalar(&value, self.2),
        )
    }
}

/// An indexed view over a borrowed standard collection iterator.
pub struct Collection<F> {
    pub key: u64,
    pub count: u64,
    pub visit: F,
}
impl<F> NativeNode for Collection<F>
where
    F: Fn(u64, &mut Visitor<'_>) -> Result<(), RowError>,
{
    fn key(&self) -> u64 {
        self.key
    }
    fn value(&self, f: &mut Header<'_>) -> Result<(), RowError> {
        f(Some(&self.count))
    }
    fn len(&self) -> u64 {
        self.count
    }
    fn child(&self, n: u64, f: &mut Visitor<'_>) -> Result<(), RowError> {
        if n >= self.count {
            return Err(RowError::Bounds);
        }
        let field = u32::try_from(n).map_err(|_| RowError::Bounds)?;
        (self.visit)(n, &mut |_, _, node| f(Step::Index(n), field, node))
    }
}

/// Borrowed sequence. The closure wraps one element only for the duration of a visit.
pub struct Sequence<'a, T, F> {
    pub items: &'a [T],
    pub key: u64,
    pub wrap: F,
}
/// Constructs a sequence, inferring its element and callback types together.
pub fn sequence<T, F>(items: &[T], key: u64, wrap: F) -> Sequence<'_, T, F>
where
    F: Fn(&T, &mut Visitor<'_>) -> Result<(), RowError>,
{
    Sequence { items, key, wrap }
}
impl<T, F> NativeNode for Sequence<'_, T, F>
where
    F: Fn(&T, &mut Visitor<'_>) -> Result<(), RowError>,
{
    fn key(&self) -> u64 {
        self.key
    }
    fn value(&self, f: &mut Header<'_>) -> Result<(), RowError> {
        f(Some(&self.len()))
    }
    fn len(&self) -> u64 {
        self.items.len() as u64
    }
    fn child(&self, ordinal: u64, f: &mut Visitor<'_>) -> Result<(), RowError> {
        let index = usize::try_from(ordinal).map_err(|_| RowError::Bounds)?;
        let field = u32::try_from(ordinal).map_err(|_| RowError::Bounds)?;
        let item = self.items.get(index).ok_or(RowError::Bounds)?;
        (self.wrap)(item, &mut |_, _, node| f(Step::Index(ordinal), field, node))
    }
}
