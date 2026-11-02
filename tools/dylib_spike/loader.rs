//! Linux-only spike executable. All hook/render dispatch remains in entry::main_with.
use horizon_client::mod_api::{Hooks, Mods};
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut hooks: Vec<Box<dyn Hooks>> = Vec::new();
    let mut ids = Vec::new();
    if let Some(folder) = std::env::var_os("HORIZON_DYLIB_MODS") {
        let folder = Path::new(&folder);
        // This is the launcher's resolved dependency order, never directory iteration order.
        let order = std::fs::read_to_string(folder.join("order"))?;
        for id in order.lines() {
            if id.is_empty()
                || !id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
                || ids.contains(&id)
            {
                return Err(format!("invalid or duplicate mod id {id:?}").into());
            }
            let actual = std::fs::read_to_string(folder.join(format!("{id}.json")))?;
            if actual != include_str!("identity.json") {
                return Err(format!(
                    "{id}: crate/toolchain/profile/game build mismatch; rebuild the mod"
                )
                .into());
            }
            // SAFETY: full-trust code only. The driver pins rustc, lock, flags and exact game
            // artifacts before compilation; the identity check above rejects stale artifacts
            // before dlopen (including library initializers). No library is ever unloaded,
            // even if the constructor panics. The symbol has Rust ABI, not C ABI.
            let library = Box::leak(Box::new(unsafe {
                libloading::Library::new(folder.join(format!("{id}.so")))?
            }));
            let entry: libloading::Symbol<fn() -> Box<dyn Hooks>> =
                unsafe { library.get(b"horizon_mod_entry\0")? };
            hooks.push(entry());
            eprintln!("DYLIB_SPIKE loaded {id}");
            ids.push(id);
        }
        let names: Vec<&'static str> = ids
            .into_iter()
            .map(|id| &*Box::leak(id.to_owned().into_boxed_str()))
            .collect();
        horizon_client::mod_api::name_mods(Box::leak(names.into_boxed_slice()));
    }
    let mods: Mods = hooks.into();
    horizon_client::entry::main_with(mods);
    Ok(())
}
