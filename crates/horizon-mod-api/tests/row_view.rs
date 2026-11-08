use mod_api::wire::{Reader, RecordPatch, WireError, Writer};
use mod_api::{Row, RowError, RowPages, RowView, RowWindow};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

struct Pages(AtomicBool);
impl RowPages for Pages {
    fn window(&self, request: RowWindow) -> Result<Vec<Row>, RowError> {
        if !self.0.load(Ordering::Relaxed) {
            return Err(RowError::Stale);
        }
        if request.node != 0 || request.start != 1 {
            return Err(RowError::Bounds);
        }
        Ok(vec![
            (0, None, 0, 0, vec![]),
            (7, Some(0), 1, 0, 42u32.to_le_bytes().to_vec()),
        ])
    }
}

#[test]
fn owned_cache_and_sparse_patch() {
    let pages = Arc::new(Pages(AtomicBool::new(true)));
    let root = RowView::new(vec![(0, None, 0, 0, vec![])], Some(pages.clone()), 0).unwrap();
    let child = root.child(1).unwrap();
    assert_eq!(child.read::<u32>(), Ok(42));
    let mut patch = RecordPatch::default();
    child.set(&mut patch, &13u32).unwrap();
    let mut out = Writer::new();
    out.put(&patch);
    let cells: Vec<(u32, Vec<u8>)> = Reader::decode(out.bytes()).unwrap();
    assert_eq!(cells, vec![(7, 13u32.to_le_bytes().to_vec())]);
    pages.0.store(false, Ordering::Relaxed);
    assert_eq!(child.read::<u32>(), Ok(42));
    assert!(matches!(root.child(2), Err(RowError::Stale)));
}

#[test]
fn malformed_rows_refuse() {
    assert!(matches!(
        RowView::new(vec![], None, 1),
        Err(RowError::Bounds)
    ));
    let root = RowView::new(
        vec![
            (0, None, 0, 0, vec![]),
            (1, Some(0), 2, 0, vec![]),
            (2, Some(0), 2, 0, vec![]),
        ],
        None,
        0,
    )
    .unwrap();
    assert!(matches!(root.child(2), Err(RowError::Unsupported)));
    assert!(matches!(root.read::<u32>(), Err(RowError::Unsupported)));
}

#[test]
fn utf16_materialization_is_bounded_and_checked() {
    let root = RowView::new(
        vec![
            (0, None, 0, 3, 2u64.to_le_bytes().to_vec()),
            (1, Some(0), 0, 0, 0xd83du16.to_le_bytes().to_vec()),
            (2, Some(0), 1, 0, 0xde00u16.to_le_bytes().to_vec()),
        ],
        None,
        0,
    )
    .unwrap();
    assert_eq!(root.type_key(), 3);
    assert_eq!(root.utf16(2).unwrap(), "😀");
    assert_eq!(root.utf16(1), Err(RowError::Budget));
    let bad = RowView::new(
        vec![(0, None, 0, 0, u64::MAX.to_le_bytes().to_vec())],
        None,
        0,
    )
    .unwrap();
    assert_eq!(bad.utf16(usize::MAX), Err(RowError::Budget));
}

#[test]
fn encoded_patch_refusal_is_atomic() {
    let mut patch = RecordPatch::default();
    patch.set_encoded(3, &[9, 8]).unwrap();
    assert_eq!(
        patch.set_encoded(3, &vec![0; 1_048_577]),
        Err(WireError::Length)
    );
    let mut out = Writer::new();
    out.put(&patch);
    let cells: Vec<(u32, Vec<u8>)> = Reader::decode(out.bytes()).unwrap();
    assert_eq!(cells, vec![(3, vec![9, 8])]);
}
