//! Diagnostic only: optimistic decoded-value reuse. Never merge into production.
use crate::wire::{Reader, Wire, WireError};
use std::{
    any::{Any, TypeId},
    cell::RefCell,
    collections::HashMap,
    hash::{BuildHasherDefault, Hasher},
};

const FORCE_MISS: bool = true;
const MAX_BYTES: usize = 4096;
const MAX_ENTRIES: usize = 4096;
#[derive(Default)]
struct Identity(u64);
impl Hasher for Identity {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        self.0 = u64::from_le_bytes(bytes.try_into().unwrap());
    }
    fn write_u64(&mut self, value: u64) {
        self.0 = value;
    }
}
struct Entry {
    bytes: Vec<u8>,
    value: Box<dyn Any>,
    ty: TypeId,
}
#[derive(Default)]
struct Cache {
    rows: HashMap<u64, Entry, BuildHasherDefault<Identity>>,
}
thread_local! { static CACHE: RefCell<Cache> = RefCell::new(Cache::default()); }

// A cheap discriminator, never an equality proof. A collision is only a cache miss.
fn hash(bytes: &[u8]) -> u64 {
    let mut value = bytes.len() as u64;
    for offset in [
        0,
        bytes.len().saturating_sub(24),
        bytes.len().saturating_sub(16),
        bytes.len().saturating_sub(8),
    ] {
        let mut word = [0; 8];
        let part = &bytes[offset..bytes.len().min(offset + 8)];
        word[..part.len()].copy_from_slice(part);
        value = value.rotate_left(9) ^ u64::from_le_bytes(word).wrapping_mul(0x517cc1b727220a95);
    }
    value
}

pub(super) fn take<T: Wire + 'static>(
    input: &mut Reader<'_>,
) -> Result<(T, Option<Vec<u8>>), WireError> {
    let bytes = input.rest();
    if bytes.len() <= MAX_BYTES {
        let hash = hash(bytes);
        let cached = CACHE.with(|cache| {
            let mut cache = cache.borrow_mut();
            let entry = cache.rows.get(&hash)?;
            (!FORCE_MISS && entry.ty == TypeId::of::<T>() && entry.bytes == bytes)
                .then(|| cache.rows.remove(&hash).unwrap())
        });
        if let Some(entry) = cached {
            input.take_bytes(bytes.len())?;
            #[cfg(feature = "wire-pool-profile")]
            super::RUNTIME.with(|rt| rt.borrow_mut().pool_counts[1] += 1);
            return Ok((*entry.value.downcast::<T>().unwrap(), Some(entry.bytes)));
        }
    }
    let value = input.take()?;
    let key = (input.rest().is_empty() && bytes.len() <= MAX_BYTES).then(Vec::new);
    #[cfg(feature = "wire-pool-profile")]
    if key.is_some() {
        super::RUNTIME.with(|rt| rt.borrow_mut().pool_counts[2] += 1);
    }
    Ok((value, key))
}

pub(super) fn retain<T: 'static>(value: T, key: Option<Vec<u8>>, bytes: &[u8]) {
    let Some(mut key) = key else {
        return;
    };
    if bytes.len() > MAX_BYTES {
        return;
    }
    key.clear();
    key.extend_from_slice(bytes);
    let hash = hash(&key);
    let entry = Entry {
        bytes: key,
        value: Box::new(value),
        ty: TypeId::of::<T>(),
    };
    // Move/drop displaced values outside the RefCell borrow, including cap eviction.
    let old = CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        let all = if cache.rows.len() >= MAX_ENTRIES {
            Some(std::mem::take(&mut cache.rows))
        } else {
            None
        };
        (all, cache.rows.insert(hash, entry))
    });
    drop(old);
}
