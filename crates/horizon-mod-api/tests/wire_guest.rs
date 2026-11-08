//! The mod's side of the boundary, driven the way the game drives it, with no wasm.
#![cfg(feature = "wasm-guest")]
use mod_api::wire::{INIT, Reader, Writer, guest};

#[unsafe(no_mangle)]
fn __horizon_mod_hooks() -> Box<dyn mod_api::Hooks> {
    struct Empty;
    impl mod_api::Hooks for Empty {
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
