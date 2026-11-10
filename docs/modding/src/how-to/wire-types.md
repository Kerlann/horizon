# Wire types

Native and wasm mods share `mod_api::wire::{Wire, Reader, Writer, WireError}`.
Derive `mod_api::Wire` explicitly on a type whose values cross a target boundary.
The build never adds a derive. Types used only inside one target do not need it.
Every field of a derived type must also implement `Wire`.

```rust
#[derive(mod_api::Wire)]
pub struct ChemicalAmount {
    pub chemical: String,
    pub amount: u64,
}
```

Records encode fields in declaration order. Enums encode a case index followed by
its fields. Lengths and numbers are little-endian; collections are length-prefixed.
Decoding returns `WireError` for invalid data rather than accepting an unknown tag,
invalid UTF-8 or an unsupported length.

## Checked representations

Use `#[wire(via = String)]` (or another `Wire` type) when a checked constructor owns
validation. Implement `From<&YourType> for String` and `TryFrom<String> for YourType`.
The derive writes the representation and maps a failed conversion to
`WireError::Invalid`. Its layout is the representation's layout.

## Layout identity

`Wire::layout` computes an encoding identity recursively, including field names,
case names and child layouts. It is a runtime method, not a constant, because
recursive types need a visited-type stack. Container layout follows encoding:
`Vec<T>` and `VecDeque<T>` agree, as do `String` and `Box<str>`; `Box<T>` is
transparent. Changing a crossing field requires rebuilding dependents; the wire
format has no compatibility versions or default fields.

`cargo horizon-mod build` writes `layouts.json` at the package root, mapping mod IDs
to public type paths and their 16-digit hexadecimal layout identities. It includes
the mod dependencies it compiled against. A cached host probe under
`target/horizon-mod/` computes these values, for both native and wasm builds. When
there are no concrete crossing types, no probe is built and no file is written.
Private types without a public path and uninstantiated generic declarations are
not listed; a public re-export can give a private module's type a public path.

A mixed-target dependent with a different layout is refused naming the library
and differing type. Even matching layouts do not enable mixed native and wasm
dependencies yet: those edges are refused explicitly. See the
[extension design](../concepts/mod-extensions.md#layout-hash-and-versioning) for the
complete layout contract.

## Hand-written implementations

The trait is unsealed. A hand-written implementation must supply `put`, `take`,
`MIN`, and `LAYOUT_SEED`; compound encodings must override the hidden `layout`
method to incorporate their children and handle recursive visits. Never use a
constant leaf seed for a compound encoding: field changes must change its identity.
The derive handles this bookkeeping automatically and is preferred.

A hand decoder owns tag validation and must use `Reader::nested` for recursive
values. `Reader` checks byte bounds and UTF-8, but cannot enforce invariants that
an implementation neglects. Keep validation at the checked constructor boundary.

Public declarations used in layout discovery must be unconditional. The layout probe refuses
`#[cfg]` and `#[cfg_attr]` on public types, modules, and re-exports, naming the declaration.
Keep target-specific implementation details private; publish an unconditional Wire representation.
