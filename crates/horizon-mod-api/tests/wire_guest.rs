//! The mod's side of the boundary, driven the way the game drives it, with no wasm.
#![cfg(feature = "wasm-guest")]
use mod_api::wire::{INIT, Reader, Writer, guest};

#[unsafe(no_mangle)]
fn __horizon_mod_hooks() -> Box<dyn mod_api::Hooks> {
    struct Empty;
    impl mod_api::Hooks for Empty {
        #[cfg(feature = "testing")]
        fn client_chat(
            &self,
            _: &mut dyn mod_api::ClientHost,
            event: &mut mod_api::ClientChatEvent<'_>,
        ) -> mod_api::Flow {
            CHAT.lock().unwrap().push((
                event.message.clone(),
                event.original_message.as_ptr() as usize,
                event.message.as_ptr() as usize,
            ));
            if event.message == "outer" {
                assert_eq!(chat("inner"), "i!");
                assert_eq!(event.message, "outer");
            }
            event.message.truncate(1);
            event.message.push('!');
            mod_api::Flow::Continue
        }
        fn subscribed(&self) -> &'static [Option<mod_api::HookId>] {
            &[]
        }
    }
    Box::new(Empty)
}

fn call(id: u32, input: &[u8]) -> Result<Vec<u8>, String> {
    let at = guest::input(input.len());
    // SAFETY: `input` hands out a buffer of exactly this length.
    unsafe { std::ptr::copy_nonoverlapping(input.as_ptr(), at, input.len()) };
    let (pointer, len) = guest::run(id);
    let (pointer, len) = (pointer, len);
    if len == u32::MAX {
        // SAFETY: a refusal's reason is a wire string the guest keeps until the next call.
        return Err(format!("refused at {pointer:?}"));
    }
    // SAFETY: the reply is `len` bytes the guest keeps until the next call.
    Ok(unsafe { std::slice::from_raw_parts(pointer, len as usize) }.to_vec())
}

#[cfg(feature = "testing")]
static CHAT: std::sync::Mutex<Vec<(String, usize, usize)>> = std::sync::Mutex::new(Vec::new());

#[cfg(feature = "testing")]
fn chat(message: &str) -> String {
    let mut input = Writer::new();
    input.put(&(0i32, 0u128, message.to_owned(), message.to_owned()));
    let id = guest::testing_export_id("Hooks", "client_chat").unwrap();
    let bytes = call(id, input.bytes()).unwrap();
    let (flow, player, uuid, text) =
        Reader::decode::<(mod_api::Flow, i32, u128, String)>(&bytes).unwrap();
    assert_eq!((flow, player, uuid), (mod_api::Flow::Continue, 0, 0));
    text
}

#[cfg(feature = "testing")]
#[test]
fn generated_hook_reuses_borrows_and_preserves_owned_values_through_reentry() {
    let long = "a long chat message whose allocation should be retained";
    assert_eq!(chat(long), "a!");
    assert_eq!(chat("short"), "s!");
    {
        let records = CHAT.lock().unwrap();
        let mut first = [records[0].1, records[0].2];
        let mut second = [records[1].1, records[1].2];
        first.sort();
        second.sort();
        assert_eq!(first, second, "both argument allocations stay reusable");
        assert_eq!(
            records[0].0, long,
            "a value kept by the mod is an owned copy"
        );
    }
    assert_eq!(chat("outer"), "o!");
    let records = CHAT.lock().unwrap();
    assert_eq!(records[2].0, "outer");
    assert_eq!(records[3].0, "inner");
    assert_ne!(
        records[2].2, records[3].2,
        "nested calls cannot lease an active argument"
    );
}

#[test]
fn reused_decode_storage_is_exclusive_owned_and_cleared() {
    let mut bytes = Writer::new();
    bytes.put(&vec!["a long first string".to_owned(), "second".to_owned()]);
    let first = guest::decode::<Vec<String>>(&mut Reader::new(bytes.bytes())).unwrap();
    let kept = (*first).clone();
    let pointer = first[0].as_ptr();
    let vector = first.as_ptr();
    drop(first);
    bytes.clear();
    bytes.put(&vec!["short".to_owned()]);
    let next = guest::decode::<Vec<String>>(&mut Reader::new(bytes.bytes())).unwrap();
    assert_eq!(&*next, &["short"]);
    assert_eq!(next.as_ptr(), vector, "reuse vector capacity");
    assert_eq!(next[0].as_ptr(), pointer, "reuse nested text capacity");
    let nested = guest::decode::<Vec<String>>(&mut Reader::new(bytes.bytes())).unwrap();
    assert_ne!(next.as_ptr(), nested.as_ptr(), "active leases cannot alias");
    assert_eq!(kept, ["a long first string", "second"]);
    drop(nested);
    drop(next);
    // A valid first field followed by malformed UTF-8 must never expose a partial value.
    assert!(
        guest::decode::<Vec<String>>(&mut Reader::new(&[
            2, 0, 0, 0, 1, 0, 0, 0, b'x', 1, 0, 0, 0, 255
        ]))
        .is_err()
    );
    bytes.clear();
    bytes.put(&Vec::<String>::new());
    let empty = guest::decode::<Vec<String>>(&mut Reader::new(bytes.bytes())).unwrap();
    assert!(empty.is_empty());
    assert_eq!(kept, ["a long first string", "second"]);
}

#[test]
fn machine_snapshot_reuses_lists_but_does_not_keep_old_nbt() {
    use mod_api::{ModBlockEntity, Tag, containers::ModBlockEntityFields};
    let mut snapshot = ModBlockEntity::new("test:machine".into(), ModBlockEntityFields::default());
    snapshot
        .nbt
        .insert("old".into(), Tag::String("text".into()));
    mod_api::__private::resolve_items(["minecraft:stone"]).unwrap();
    let key = mod_api::__private::item_key("minecraft:stone").unwrap();
    snapshot.items = vec![mod_api::ItemStack::new(key, 2), None, None];
    snapshot.data = vec![1, 2, 3, 4];
    let mut bytes = Writer::new();
    bytes.put(&snapshot);
    let first = guest::decode::<ModBlockEntity>(&mut Reader::new(bytes.bytes())).unwrap();
    let item_storage = first.items.as_ptr();
    let data_storage = first.data.as_ptr();
    let owned = (*first).clone();
    drop(first);
    snapshot.nbt.clear();
    snapshot.items[0] = None;
    snapshot.items.truncate(1);
    snapshot.data = vec![9];
    bytes.clear();
    bytes.put(&snapshot);
    let next = guest::decode::<ModBlockEntity>(&mut Reader::new(bytes.bytes())).unwrap();
    assert_eq!(next.items.as_ptr(), item_storage);
    assert_eq!(next.data.as_ptr(), data_storage);
    assert_eq!(next.data, [9]);
    assert_eq!(next.items, [None]);
    assert!(next.nbt.is_empty());
    assert_eq!(owned.data, [1, 2, 3, 4]);
    assert!(owned.nbt.contains_key(&"old".into()));
}

#[test]
fn start_up_installs_the_package_list_and_an_unknown_call_is_refused() {
    let mut packages = Writer::new();
    packages.put(&Vec::<mod_api::PackageInfo>::new());
    assert_eq!(call(INIT, packages.bytes()), Ok(Vec::new()));
    assert!(
        call(INIT, &[1, 2, 3]).is_err(),
        "a short package list is refused, not read"
    );
    assert!(call(0xFFFF_0000, &[]).is_err(), "no such call");
}
