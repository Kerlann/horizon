use mod_api::wire::{
    DecodedPatchCell, MAX_DEPTH, PatchCell, PatchFields, Reader, Record, RecordPatch, WireError,
    Writer,
};

fn bytes(patch: &RecordPatch) -> Vec<u8> {
    let mut out = Writer::new();
    out.put(patch);
    out.into_bytes()
}
fn apply(bytes: &[u8], count: &mut u32, enabled: &mut bool) -> Result<(), WireError> {
    PatchFields::new(
        (PatchCell::new(1, count), (PatchCell::new(4, enabled), ())),
        8,
    )
    .apply(bytes)
}

#[test]
fn sparse_patches_validate_every_cell_before_any_commit() {
    let (mut count, mut enabled) = (7, false);
    let mut patch = RecordPatch::default();
    patch.set(1, &99u32).unwrap();
    patch.set(4, &2u8).unwrap(); // A malformed bool after a valid first cell.
    assert!(apply(&bytes(&patch), &mut count, &mut enabled).is_err());
    assert_eq!((count, enabled), (7, false));
    patch.set(4, &1u16).unwrap(); // Valid bool prefix, but trailing data within that cell.
    assert_eq!(
        apply(&bytes(&patch), &mut count, &mut enabled),
        Err(WireError::Trailing)
    );
    assert_eq!((count, enabled), (7, false));
    patch.set(4, &true).unwrap();
    apply(&bytes(&patch), &mut count, &mut enabled).unwrap();
    assert_eq!((count, enabled), (99, true));
    let mut sparse = RecordPatch::default();
    sparse.set(1, &101u32).unwrap();
    apply(&bytes(&sparse), &mut count, &mut enabled).unwrap();
    assert_eq!((count, enabled), (101, true));
}

#[test]
fn unknown_readonly_duplicate_and_trailing_edits_are_atomic() {
    for node in [0, 3, 8, u32::MAX] {
        let (mut count, mut enabled) = (7, false);
        let mut patch = RecordPatch::default();
        patch.set(1, &99u32).unwrap();
        patch.set(node, &true).unwrap();
        assert!(apply(&bytes(&patch), &mut count, &mut enabled).is_err());
        assert_eq!((count, enabled), (7, false));
    }
    for nodes in [[1u32, 1], [4, 1]] {
        let (mut count, mut enabled) = (7, false);
        let mut out = Writer::new();
        out.put(&nodes.map(|node| (node, vec![0u8])).to_vec());
        assert!(apply(out.bytes(), &mut count, &mut enabled).is_err());
        assert_eq!((count, enabled), (7, false));
    }
    let mut patch = RecordPatch::default();
    patch.set(1, &99u32).unwrap();
    let mut extra = bytes(&patch);
    extra.push(0);
    let (mut count, mut enabled) = (7, false);
    assert_eq!(
        apply(&extra, &mut count, &mut enabled),
        Err(WireError::Trailing)
    );
    assert_eq!((count, enabled), (7, false));
}

#[test]
fn refused_values_preserve_the_patch_and_decode_limits_precede_materialization() {
    let mut patch = RecordPatch::default();
    patch.set(1, &7u32).unwrap();
    let before = bytes(&patch);
    assert_eq!(patch.set(1, &"x".repeat(1_048_576)), Err(WireError::Length));
    assert_eq!(bytes(&patch), before);
    let mut too_many = vec![0; 4 + 65_537 * 8];
    too_many[..4].copy_from_slice(&65_537u32.to_le_bytes());
    assert!(matches!(
        Reader::decode::<RecordPatch>(&too_many),
        Err(WireError::Length)
    ));
    let mut out = Writer::new();
    out.put(&vec![(1u32, vec![0u8; 1_048_576]), (2u32, vec![0u8])]);
    assert!(matches!(
        Reader::decode::<RecordPatch>(out.bytes()),
        Err(WireError::Length)
    ));
}

#[test]
fn native_decoder_plans_before_commit_and_rejects_trailing_data() {
    #[derive(Debug, PartialEq)]
    struct Native(u32, bool);
    fn decode(input: &mut Reader<'_>) -> Result<Native, WireError> {
        input.nested(|input| Ok(Native(input.take()?, input.take()?)))
    }
    let (mut first, mut native) = (7u32, Native(8, false));
    let mut patch = RecordPatch::default();
    patch.set(1, &99u32).unwrap();
    patch.set(2, &(42u32, 2u8)).unwrap();
    let mut fields = PatchFields::new(
        (
            PatchCell::new(1, &mut first),
            (DecodedPatchCell::new(2, &mut native, decode), ()),
        ),
        3,
    );
    assert!(fields.apply(&bytes(&patch)).is_err());
    assert_eq!((first, &native), (7, &Native(8, false)));
    patch.set(2, &(42u32, true, 0u8)).unwrap();
    let mut fields = PatchFields::new(
        (
            PatchCell::new(1, &mut first),
            (DecodedPatchCell::new(2, &mut native, decode), ()),
        ),
        3,
    );
    assert_eq!(fields.apply(&bytes(&patch)), Err(WireError::Trailing));
    assert_eq!((first, &native), (7, &Native(8, false)));
    patch.set(2, &(42u32, true)).unwrap();
    PatchFields::new(
        (
            PatchCell::new(1, &mut first),
            (DecodedPatchCell::new(2, &mut native, decode), ()),
        ),
        3,
    )
    .apply(&bytes(&patch))
    .unwrap();
    assert_eq!((first, native), (99, Native(42, true)));
}

#[test]
fn bounded_readers_preserve_following_fields_and_the_depth_budget() {
    let mut out = Writer::new();
    out.put(&"abc");
    out.put(&9u32);
    let mut input = Reader::new(out.bytes());
    assert_eq!(
        input.bounded(7, |input| input.take::<String>()).unwrap(),
        "abc"
    );
    assert_eq!(input.take::<u32>().unwrap(), 9);
    fn chain(input: &mut Reader<'_>, depth: u16) -> Result<u8, WireError> {
        input.nested(|input| {
            input.bounded(1, |input| {
                if depth == 0 {
                    input.take()
                } else {
                    chain(input, depth - 1)
                }
            })
        })
    }
    assert_eq!(chain(&mut Reader::new(&[7]), MAX_DEPTH - 1), Ok(7));
    assert_eq!(
        chain(&mut Reader::new(&[7]), MAX_DEPTH),
        Err(WireError::Depth)
    );
}
