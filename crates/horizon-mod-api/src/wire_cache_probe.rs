//! Diagnostic only. Optimistic admission and scene-only revision footer; never merge.
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
    fn write(&mut self, b: &[u8]) {
        self.0 = u64::from_le_bytes(b.try_into().unwrap());
    }
    fn write_u64(&mut self, v: u64) {
        self.0 = v;
    }
}
struct Entry {
    bytes: Vec<u8>,
    value: Box<dyn Any>,
    ty: TypeId,
    rebase: bool,
}
#[derive(Default)]
struct Cache {
    rows: HashMap<u64, Entry, BuildHasherDefault<Identity>>,
}
thread_local! { static CACHE:RefCell<Cache>=RefCell::new(Cache::default()); }
fn hash(bytes: &[u8], rebase: bool) -> u64 {
    let mut value = bytes.len() as u64;
    if rebase {
        // Captured scene schema: the final 17 bytes are birth, changes and None.
        for offset in [0, bytes.len() - 17] {
            let word = u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap());
            value = value.rotate_left(9) ^ word.wrapping_mul(0x517cc1b727220a95);
        }
    } else {
        for offset in [
            0,
            bytes.len().saturating_sub(24),
            bytes.len().saturating_sub(16),
            bytes.len().saturating_sub(8),
        ] {
            let mut word = [0; 8];
            let part = &bytes[offset..bytes.len().min(offset + 8)];
            word[..part.len()].copy_from_slice(part);
            value =
                value.rotate_left(9) ^ u64::from_le_bytes(word).wrapping_mul(0x517cc1b727220a95);
        }
    }
    value
}
pub(super) fn take<T: Wire + 'static>(
    input: &mut Reader<'_>,
    repair: Option<fn(&mut T, u64)>,
) -> Result<(T, Option<(Vec<u8>, bool)>), WireError> {
    let bytes = input.rest();
    let rebase = repair.is_some();
    let eligible =
        bytes.len() <= MAX_BYTES && (!rebase || bytes.len() >= 24 && bytes.last() == Some(&0));
    if eligible {
        // Perturb only the lookup key for the forced-miss control, retaining actual lookup work.
        let hash = hash(bytes, rebase) ^ if FORCE_MISS { 1 << 63 } else { 0 };
        let cached = CACHE.with(|cache| {
            let mut cache = cache.borrow_mut();
            let e = cache.rows.get(&hash)?;
            let equal = e.bytes.len() == bytes.len()
                && if rebase {
                    let end = bytes.len() - 9;
                    e.bytes[..end] == bytes[..end] && e.bytes[end + 8..] == bytes[end + 8..]
                } else {
                    e.bytes == bytes
                };
            (e.rebase == rebase && e.ty == TypeId::of::<T>() && equal)
                .then(|| cache.rows.remove(&hash).unwrap())
        });
        if let Some(e) = cached {
            let mut value = *e.value.downcast::<T>().unwrap();
            if let Some(repair) = repair {
                repair(
                    &mut value,
                    u64::from_le_bytes(bytes[bytes.len() - 9..bytes.len() - 1].try_into().unwrap()),
                );
            }
            input.take_bytes(bytes.len())?;
            #[cfg(feature = "wire-pool-profile")]
            super::RUNTIME.with(|rt| rt.borrow_mut().pool_counts[1] += 1);
            return Ok((value, Some((e.bytes, rebase))));
        }
    }
    let value = input.take()?;
    let key = (eligible && input.rest().is_empty()).then(|| (Vec::new(), rebase));
    #[cfg(feature = "wire-pool-profile")]
    if key.is_some() {
        super::RUNTIME.with(|rt| rt.borrow_mut().pool_counts[2] += 1);
    }
    Ok((value, key))
}
pub(super) fn retain<T: 'static>(value: T, key: Option<(Vec<u8>, bool)>, bytes: &[u8]) {
    let Some((mut key, rebase)) = key else {
        return;
    };
    if bytes.len() > MAX_BYTES || rebase && (bytes.len() < 24 || bytes.last() != Some(&0)) {
        return;
    }
    key.clear();
    key.extend_from_slice(bytes);
    let hash = hash(&key, rebase);
    let entry = Entry {
        bytes: key,
        value: Box::new(value),
        ty: TypeId::of::<T>(),
        rebase,
    };
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
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ModBlockEntity,
        containers::{ModBlockEntitySnapshot, ModRevision},
        wire::Writer,
    };
    fn bytes(v: &ModBlockEntity) -> Vec<u8> {
        let mut w = Writer::new();
        w.put(v);
        w.into_bytes()
    }
    fn repair(v: &mut ModBlockEntity, changes: u64) {
        v.revision = ModRevision::from_snapshot(v.revision.identity(), changes);
    }
    #[test]
    fn exact_hit_and_revision_repair_preserve_other_fields() {
        let original = ModBlockEntity::new("probe:entity".into(), Default::default());
        let first = bytes(&original);
        let (mut v, key) = take::<ModBlockEntity>(&mut Reader::new(&first), Some(repair)).unwrap();
        v.data.push(1);
        v.data.push(2);
        let post = bytes(&v);
        assert_eq!(v.revision().changes(), 2);
        retain(v, key, &post);
        let mut native = original;
        native.apply_snapshot(Reader::decode::<ModBlockEntitySnapshot>(&post).unwrap());
        assert_eq!(native.revision().changes(), 1);
        let next = bytes(&native);
        let (v, key) = take::<ModBlockEntity>(&mut Reader::new(&next), Some(repair)).unwrap();
        assert_eq!(bytes(&v), next);
        assert_eq!(v.data, [1, 2]);
        super::super::RUNTIME.with(|rt| assert_eq!(rt.borrow().pool_counts[1], 1));
        retain(v, key, &next);
        // A real body-field difference must miss, not get repaired away.
        native.data[0] = 9;
        let changed = bytes(&native);
        let (v, _) = take::<ModBlockEntity>(&mut Reader::new(&changed), Some(repair)).unwrap();
        assert_eq!(v.data, [9, 2]);
        super::super::RUNTIME.with(|rt| assert_eq!(rt.borrow().pool_counts[1], 1));
    }
}
