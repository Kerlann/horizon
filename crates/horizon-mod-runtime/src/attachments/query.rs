//! Owned snapshot batches and lock-free borrowed typed traversal.
use std::{fmt, hash::Hash, slice, sync::Arc};

use super::{
    AttachmentError, AttachmentType,
    index::{AttachmentIndex, AttachmentRow},
};

/// A failed row read or write through a retained batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum AttachmentRowError {
    /// The stored binding could not be read as the requested type.
    Value(AttachmentError),
    /// The holder unloaded, was replaced, or belongs to another index.
    StaleHolder,
}

impl fmt::Display for AttachmentRowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Value(error) => write!(f, "attachment row read failed: {error:?}"),
            Self::StaleHolder => f.write_str("attachment row holder is stale"),
        }
    }
}
impl std::error::Error for AttachmentRowError {}

/// An immutable bucket snapshot. Construction moves the Arc returned by
/// `AttachmentIndex::snapshot`; it copies neither rows nor values.
/// Later mutations affect only fresh batches, not these captured bindings.
#[derive(Clone)]
#[non_exhaustive]
pub struct AttachmentBatch<H> {
    rows: Arc<Vec<AttachmentRow<H>>>,
}

impl<H> AttachmentBatch<H> {
    /// Wrap a captured index bucket without copying its contents.
    pub fn new(rows: Arc<Vec<AttachmentRow<H>>>) -> Self {
        Self { rows }
    }

    /// Bind the key once, checking its policy before any row is read.
    /// The query borrows only the key, never the index or host.
    pub fn query<T: Send + Sync + 'static>(
        self,
        key: &AttachmentType<T>,
    ) -> Result<AttachmentQuery<'_, H, T>, AttachmentError> {
        key.validate_policy()?;
        Ok(AttachmentQuery { batch: self, key })
    }
}

/// Typed view of a captured batch. Saved values decode lazily into the entry's
/// read cache; errors preserve saved data and never initialize a default.
/// This cache does not claim the canonical stored type, dirty or sync the holder.
#[non_exhaustive]
pub struct AttachmentQuery<'key, H, T: 'static> {
    batch: AttachmentBatch<H>,
    key: &'key AttachmentType<T>,
}

impl<H: Copy, T: Send + Sync + 'static> AttachmentQuery<'_, H, T> {
    /// Traverse captured index order without locks, holder lookups, value clones
    /// or per-item allocation. A first saved decode may allocate in the codec/cache.
    pub fn iter(&self) -> AttachmentIter<'_, H, T> {
        AttachmentIter {
            rows: self.batch.rows.iter(),
            key: self.key,
        }
    }
}

/// Borrowing iterator over a batch, not an iterator owning its borrowed values.
#[non_exhaustive]
pub struct AttachmentIter<'q, H, T: 'static> {
    rows: slice::Iter<'q, AttachmentRow<H>>,
    key: &'q AttachmentType<T>,
}

impl<'q, H: Copy, T: Send + Sync + 'static> Iterator for AttachmentIter<'q, H, T> {
    type Item = Result<AttachmentRowRef<'q, H, T>, AttachmentRowError>;

    fn next(&mut self) -> Option<Self::Item> {
        self.rows.next().map(|row| {
            row.entry
                .read_validated(self.key)
                .map(|value| AttachmentRowRef {
                    holder: row.holder,
                    value,
                    row,
                })
                .map_err(AttachmentRowError::Value)
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.rows.size_hint()
    }
}
impl<H: Copy, T: Send + Sync + 'static> ExactSizeIterator for AttachmentIter<'_, H, T> {}

/// A Copy holder identity and a value borrowed directly from its captured entry.
/// Interior mutability inside T is not deep-frozen by a snapshot.
#[non_exhaustive]
pub struct AttachmentRowRef<'q, H, T> {
    /// Ready-to-use holder; no conversion or resolution is done during traversal.
    pub holder: H,
    /// The captured value, not a clone of T.
    pub value: &'q T,
    row: &'q AttachmentRow<H>,
}

impl<H: Copy + Eq + Hash, T> AttachmentRowRef<'_, H, T> {
    /// Validate immediately before a write through the ordinary holder write path,
    /// under the same owning-thread access. Value replacement or attachment removal
    /// alone does not stale a row, but unloading or re-registering the holder does.
    pub fn validate_write(&self, index: &AttachmentIndex<H>) -> Result<(), AttachmentRowError> {
        index.validate_row(self.row)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attachments::{AttachmentCodec, AttachmentEntry, AttachmentPolicy, Decoded};
    use horizon_core::nbt::{Compound, Tag};
    use std::sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    struct CountingAllocator;
    thread_local! {
        static ALLOCATIONS: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
    }
    fn count_allocation() {
        let _ = ALLOCATIONS.try_with(|count| {
            if let Some(n) = count.get() {
                count.set(Some(n + 1));
            }
        });
    }
    // SAFETY: every operation delegates unchanged to the system allocator.
    unsafe impl std::alloc::GlobalAlloc for CountingAllocator {
        unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
            count_allocation();
            unsafe { std::alloc::System.alloc(layout) }
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: std::alloc::Layout) {
            unsafe { std::alloc::System.dealloc(ptr, layout) }
        }
        unsafe fn alloc_zeroed(&self, layout: std::alloc::Layout) -> *mut u8 {
            count_allocation();
            unsafe { std::alloc::System.alloc_zeroed(layout) }
        }
        unsafe fn realloc(&self, ptr: *mut u8, layout: std::alloc::Layout, size: usize) -> *mut u8 {
            count_allocation();
            unsafe { std::alloc::System.realloc(ptr, layout, size) }
        }
    }
    #[global_allocator]
    static ALLOCATOR: CountingAllocator = CountingAllocator;

    static DECODES: AtomicUsize = AtomicUsize::new(0);
    static KEY: AttachmentType<u32> = AttachmentType {
        namespace: "query",
        name: "value",
        default: || panic!("query must not initialize defaults"),
        codec: Some(AttachmentCodec {
            encode: |_| unreachable!(),
            decode: |saved| {
                DECODES.fetch_add(1, Ordering::Relaxed);
                match saved.get(&"n".into()) {
                    Some(Tag::Int(n)) => Ok(*n as u32),
                    _ => Err(AttachmentError::Decode),
                }
            },
        }),
        copy_on_death: false,
        sync: false,
    };
    fn key() -> super::super::index::AttachmentKey {
        ("query".into(), "value".into())
    }
    fn entry(value: impl Send + Sync + 'static) -> Arc<AttachmentEntry> {
        Arc::new(AttachmentEntry {
            state: super::super::Entry::Decoded(Decoded {
                value: Arc::new(value),
                encoded: None,
                policy: AttachmentPolicy {
                    copy_on_death: false,
                    sync: false,
                },
            }),
        })
    }
    fn query(index: &AttachmentIndex<u32>) -> AttachmentQuery<'static, u32, u32> {
        AttachmentBatch::new(index.snapshot(&key()))
            .query(&KEY)
            .unwrap()
    }
    fn values(query: &AttachmentQuery<'_, u32, u32>) -> Vec<(u32, u32)> {
        query
            .iter()
            .map(|row| {
                let row = row.unwrap();
                (row.holder, *row.value)
            })
            .collect()
    }

    #[test]
    fn captured_order_values_and_stale_write_authority() {
        let mut index = AttachmentIndex::default();
        let one = index.register(1, [(key(), entry(10u32))]).into_result();
        let two = index.register(2, [(key(), entry(20u32))]).into_result();
        index.register(3, [(key(), entry(30u32))]).into_result();
        let captured = query(&index);
        index.set(&one, &key(), entry(11u32)).into_result();
        index.remove(&two, &key()).into_result();
        assert_eq!(values(&captured), [(1, 10), (2, 20), (3, 30)]);
        assert_eq!(values(&query(&index)), [(1, 11), (3, 30)]);
        for row in captured.iter() {
            assert_eq!(row.unwrap().validate_write(&index), Ok(()));
        }
        index.unload(1).into_result();
        // Even reusing the identical entry pointer creates a new registration.
        let same = index.snapshot(&key())[0].entry.clone();
        index.register(3, [(key(), same)]).into_result();
        let rows: Vec<_> = captured.iter().map(Result::unwrap).collect();
        assert_eq!(
            rows[0].validate_write(&index),
            Err(AttachmentRowError::StaleHolder)
        );
        assert_eq!(
            rows[2].validate_write(&index),
            Err(AttachmentRowError::StaleHolder)
        );
        assert_eq!(rows[1].validate_write(&index), Ok(()));
        assert_eq!(values(&captured), [(1, 10), (2, 20), (3, 30)]);
        assert_eq!(
            rows[1].validate_write(&AttachmentIndex::default()),
            Err(AttachmentRowError::StaleHolder)
        );
    }

    #[test]
    fn lazy_success_only_decode_and_malformed_row_is_not_skipped() {
        DECODES.store(0, Ordering::Relaxed);
        let saved = |payload| {
            Arc::new(AttachmentEntry::raw(
                [("value".into(), Tag::Compound(payload))].into(),
            ))
        };
        let good = saved([("n".into(), Tag::Int(17))].into());
        let bad = saved(Compound::new());
        let mut index = AttachmentIndex::default();
        index.register(1, [(key(), good)]).into_result();
        index.register(2, [(key(), bad.clone())]).into_result();
        index
            .register(3, [(key(), entry("wrong type"))])
            .into_result();
        let batch = query(&index);
        assert_eq!(DECODES.load(Ordering::Relaxed), 0);
        for expected in [2, 3] {
            let mut iter = batch.iter();
            assert_eq!(*iter.next().unwrap().unwrap().value, 17);
            assert!(matches!(
                iter.next(),
                Some(Err(AttachmentRowError::Value(AttachmentError::Decode)))
            ));
            assert!(matches!(
                iter.next(),
                Some(Err(AttachmentRowError::Value(
                    AttachmentError::TypeMismatch
                )))
            ));
            assert!(iter.next().is_none());
            assert_eq!(DECODES.load(Ordering::Relaxed), expected);
        }
        let super::super::Entry::Raw { reads, .. } = &bad.state else {
            panic!("raw preserved")
        };
        assert!(reads.value.get().is_none());
    }

    #[test]
    fn decoded_thousand_row_iteration_allocates_nothing() {
        let mut index = AttachmentIndex::default();
        for holder in 0..1000 {
            index
                .register(holder, [(key(), entry(holder + 1))])
                .into_result();
        }
        let batch = query(&index);
        ALLOCATIONS.with(|n| n.set(Some(0)));
        let mut total = 0;
        let mut expected_holder = 0;
        for row in batch.iter() {
            let row = row.unwrap();
            assert_eq!(row.holder, expected_holder);
            expected_holder += 1;
            total += *row.value;
        }
        let allocations = ALLOCATIONS.with(|n| n.replace(None).unwrap());
        assert_eq!(allocations, 0);
        assert_eq!(expected_holder, 1000);
        assert_eq!(total, 500500);
    }

    #[test]
    fn final_batch_drops_mod_value_outside_index_mutex() {
        struct Probe(Arc<Mutex<AttachmentIndex<u32>>>);
        impl Drop for Probe {
            fn drop(&mut self) {
                assert!(
                    self.0.try_lock().is_ok(),
                    "index locked during mod destructor"
                );
            }
        }
        let index = Arc::new(Mutex::new(AttachmentIndex::default()));
        let token = index
            .lock()
            .unwrap()
            .register(1, [(key(), entry(Probe(index.clone())))])
            .into_result();
        let probe_key = AttachmentType {
            namespace: "query",
            name: "value",
            default: || unreachable!(),
            codec: None,
            copy_on_death: false,
            sync: false,
        };
        let batch = AttachmentBatch::new(index.lock().unwrap().snapshot(&key()))
            .query::<Probe>(&probe_key)
            .unwrap();
        let retired = { index.lock().unwrap().unregister(&token) };
        retired.into_result();
        assert_eq!(batch.iter().count(), 1);
        drop(batch);
    }
}
