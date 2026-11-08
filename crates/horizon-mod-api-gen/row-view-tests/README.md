# Owned row view fixture

Run the generated code, not a token-string assertion:

```sh
CARGO_INCREMENTAL=0 cargo test --manifest-path crates/horizon-mod-api-gen/row-view-tests/Cargo.toml -p row-view-tests emitted_
```

The build script generates wrappers from `crossing::Rule` and `native_schema::Exposure`.
The fixture executes tuple, option, sequence, scalar, UTF-16 and enum accessors over ordinary
owned rows. It checks sparse node IDs, wrong-case refusal, bounds, constructor counts and byte caps.
The separate `horizon-mod-points` test `row_views` exercises ordinary native PageSource expiry.

`row_views::generate` takes the root Shape and desired Rust wrapper name. Each generated wrapper
contains one RowView. `new(row, expected_key)` accepts the key from the caller's exposure plan;
this generator deliberately does not define another schema hash. A selected receiver and a fully
reachable argument must be generated with their respective exposure policies. Hidden fields are
not emitted. Scalar setters merely build sparse patches; native application owns write permission.
Constructors consume ordinary encoded field bytes, not engine DTOs. The native receiver must
validate the resulting concrete value and domain before applying it.

Not integrated here: primary point staging, shared Plan assembly, callable bridges, or generated sample catalog coverage. The standalone fixture does not
change the mod-points build script.
