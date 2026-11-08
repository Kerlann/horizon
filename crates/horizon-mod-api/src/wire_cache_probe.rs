//! Diagnostic only: optimistic decoded-value reuse. Never merge into production.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ModBlockEntity, containers::ModBlockEntitySnapshot, wire::Writer};
    fn bytes(value: &ModBlockEntity) -> Vec<u8> {
        let mut out = Writer::new();
        out.put(value);
        out.into_bytes()
    }
    #[test]
    fn exact_snapshot_hits_but_native_revision_apply_changes_the_key() {
        let entity = ModBlockEntity::new("probe:entity".into(), Default::default());
        let original = bytes(&entity);
        let (value, key) = take::<ModBlockEntity>(&mut Reader::new(&original)).unwrap();
        retain(value, key, &original);
        let (mut value, key) = take::<ModBlockEntity>(&mut Reader::new(&original)).unwrap();
        super::super::RUNTIME.with(|rt| assert_eq!(rt.borrow().pool_counts[1], 1));
        value.data.push(1);
        value.data.push(2);
        let post = bytes(&value);
        assert_eq!(value.revision().changes(), 2);
        retain(value, key, &post);
        let mut native = entity;
        native.apply_snapshot(Reader::decode::<ModBlockEntitySnapshot>(&post).unwrap());
        assert_eq!(native.data, [1, 2]);
        assert_eq!(native.revision().changes(), 1);
        let next_input = bytes(&native);
        assert_ne!(next_input, post);
        take::<ModBlockEntity>(&mut Reader::new(&next_input)).unwrap();
        super::super::RUNTIME.with(|rt| assert_eq!(rt.borrow().pool_counts[1], 1));
    }
}
use crate::wire::{Reader, Wire, WireError};
use std::{
    any::{Any, TypeId},
    cell::RefCell,
    collections::HashMap,
    hash::{BuildHasherDefault, Hasher},
};

const FORCE_MISS: bool = false;
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
    sequence: u64,
    bytes: Vec<u8>,
    value: Box<dyn Any>,
    ty: TypeId,
}
#[derive(Default)]
struct Cache {
    rows: HashMap<u64, Entry, BuildHasherDefault<Identity>>,
    captures: usize,
    sequence: u64,
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
    #[cfg(not(test))]
    capture_difference::<T>(bytes);
    let value = input.take()?;
    let key = (input.rest().is_empty() && bytes.len() <= MAX_BYTES).then(Vec::new);
    #[cfg(feature = "wire-pool-profile")]
    if key.is_some() {
        super::RUNTIME.with(|rt| rt.borrow_mut().pool_counts[2] += 1);
    }
    Ok((value, key))
}

#[cfg(not(test))]
fn capture_difference<T: 'static>(bytes: &[u8]) {
    if bytes.len() < 17 || bytes.last() != Some(&0) {
        return;
    }
    let old = CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.captures >= 6 || cache.sequence < 1100 {
            return None;
        }
        let old = cache
            .rows
            .values()
            .filter(|entry| {
                let old = &entry.bytes;
                entry.ty == TypeId::of::<T>()
                    && old.len() >= 17
                    && old.last() == Some(&0)
                    && old[old.len() - 17..old.len() - 9]
                        == bytes[bytes.len() - 17..bytes.len() - 9]
            })
            .max_by_key(|entry| entry.sequence)?
            .bytes
            .clone();
        cache.captures += 1;
        Some(old)
    });
    if let Some(old) = old {
        let differences = old
            .iter()
            .zip(bytes)
            .enumerate()
            .filter_map(|(i, (a, b))| (a != b).then_some((i, *a, *b)))
            .collect::<Vec<_>>();
        let message = format!(
            "CACHE RAW DIFFERENCE cached_len={} input_len={} offsets_cached_input={differences:?} cached_tail={:?} input_tail={:?}",
            old.len(),
            bytes.len(),
            &old[old.len() - 24..],
            &bytes[bytes.len() - 24..]
        );
        let mut out = crate::wire::Writer::new();
        out.put_str(&message);
        let _ = super::host(crate::wire::HOST_LOG, out);
    }
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
    let mut entry = Entry {
        sequence: 0,
        bytes: key,
        value: Box::new(value),
        ty: TypeId::of::<T>(),
    };
    // Move/drop displaced values outside the RefCell borrow, including cap eviction.
    let old = CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        cache.sequence += 1;
        entry.sequence = cache.sequence;
        let all = if cache.rows.len() >= MAX_ENTRIES {
            Some(std::mem::take(&mut cache.rows))
        } else {
            None
        };
        (all, cache.rows.insert(hash, entry))
    });
    drop(old);
}
