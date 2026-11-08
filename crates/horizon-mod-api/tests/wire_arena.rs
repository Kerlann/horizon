use mod_api::wire::{ArenaOp, ArenaWriter, Reader, Wire, WireError, Writer};

fn memory(bytes: &mut Vec<u8>, operation: ArenaOp<'_>) -> Result<(), WireError> {
    match operation {
        ArenaOp::Read(len, writer) => writer.put_bytes(&bytes[..len]),
        ArenaOp::Write(offset, data) => {
            bytes.truncate(offset);
            bytes.extend_from_slice(data);
        }
    }
    Ok(())
}
struct ObservesBuffer;
impl Wire for ObservesBuffer {
    const MIN: usize = 8;
    fn put(&self, out: &mut Writer) {
        out.put(&42u32);
        out.put(&(out.bytes().len() as u32));
        let prefix = std::mem::take(out).into_bytes();
        out.put_bytes(&prefix);
        out.limit(12);
    }
    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        input.take_bytes(8)?;
        Ok(Self)
    }
}

#[test]
fn custom_buffer_observation_and_writer_auto_traits_are_unchanged() {
    struct Stored {
        writer: Writer,
    }
    fn send_sync<T: Send + Sync>() {}
    send_sync::<Writer>();
    let stored = Stored {
        writer: Writer::new(),
    };
    assert!(stored.writer.bytes().is_empty());
    let mut bytes = Vec::new();
    let mut sink = |operation: ArenaOp<'_>| memory(&mut bytes, operation);
    let mut arena = ArenaWriter::new(&mut sink);
    arena.put(&(9u32, ObservesBuffer));
    assert_eq!(arena.len(), 12);
    assert_eq!(arena.error(), None);
    arena.put(&1u8);
    assert_eq!(arena.error(), Some(WireError::Length));
    drop(arena);
    drop(sink);
    assert_eq!(bytes, [9, 0, 0, 0, 42, 0, 0, 0, 8, 0, 0, 0]);
}

#[test]
fn direct_snapshot_codec_matches_buffered_codec_and_stops_on_error() {
    use mod_api::{Tag, containers::ModBlockEntityFields};
    let mut snapshot = ModBlockEntityFields::default();
    snapshot
        .nbt
        .insert("text".into(), Tag::String("example".into()));
    snapshot.items = vec![None, None];
    snapshot.data = vec![-1, 2, 3];
    let mut expected = Writer::new();
    expected.put(&snapshot);
    let mut bytes = Vec::new();
    let mut sink = |operation: ArenaOp<'_>| memory(&mut bytes, operation);
    let mut arena = ArenaWriter::new(&mut sink);
    arena.put(&snapshot);
    assert_eq!(arena.error(), None);
    drop(arena);
    drop(sink);
    assert_eq!(bytes, expected.bytes());
    let mut calls = 0;
    let mut sink = |_: ArenaOp<'_>| {
        calls += 1;
        Err(WireError::Eof)
    };
    let mut arena = ArenaWriter::new(&mut sink);
    arena.put(&snapshot);
    arena.put_owned(7u64);
    assert_eq!(arena.error(), Some(WireError::Eof));
    assert!(arena.is_empty());
    drop(arena);
    drop(sink);
    assert_eq!(calls, 1);
}
