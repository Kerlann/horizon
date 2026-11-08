fn main() {
    let root =
        std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("../..");
    let out_dir = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    horizon_mod_api_gen::generate_points(&root, &out_dir).expect("generating the points");
    let fixture = horizon_mod_api_gen::internals::Internal {
        path: "horizon_client::Choice".into(),
        kind: "enum",
        cfg: String::new(),
        signature: "pub enum Choice { Empty, One(u32), Named { value: String } }".into(),
        hash: String::new(),
        shape_only: false,
        opaque_reason: None,
        refs: Default::default(),
    };
    let pair = horizon_mod_api_gen::internals::Internal {
        path: "horizon_client::Pair".into(),
        kind: "struct",
        cfg: String::new(),
        signature: "pub struct Pair { pub a: u32, pub b: u32 }".into(),
        hash: String::new(),
        shape_only: false,
        opaque_reason: None,
        refs: Default::default(),
    };
    let later = horizon_mod_api_gen::internals::Internal {
        path: "horizon_client::Later".into(),
        kind: "enum",
        cfg: String::new(),
        signature: "pub enum Later { Both { a: u32, b: u32 } }".into(),
        hash: String::new(),
        shape_only: false,
        opaque_reason: None,
        refs: Default::default(),
    };
    let leaf = horizon_mod_api_gen::internals::Internal {
        path: "horizon_client::Leaf".into(),
        kind: "struct",
        cfg: String::new(),
        signature: "pub struct Leaf { pub n: u32 }".into(),
        hash: String::new(),
        shape_only: false,
        opaque_reason: None,
        refs: Default::default(),
    };
    let holder = horizon_mod_api_gen::internals::Internal {
        path: "horizon_client::Holder".into(),
        kind: "struct",
        cfg: String::new(),
        signature: "pub struct Holder { pub child: Box<Leaf>, pub items: &'static [u32], pub shared: &'static Leaf }".into(),
        hash: String::new(),
        shape_only: false,
        opaque_reason: None,
        refs: Default::default(),
    };
    let rule = horizon_mod_api_gen::crossing::Rule::new(&[fixture, pair, later, leaf, holder]);
    for ty in [
        "horizon_client::Pair",
        "horizon_client::Later",
        "horizon_client::Holder",
        "Box<Vec<u32>>",
        "&'static [u32]",
        "Vec<u32>",
        "Option<String>",
        "(u32, String)",
        "std::collections::BTreeMap<u32, String>",
        "horizon_client::Choice",
    ] {
        let exposure: &dyn horizon_mod_api_gen::native_schema::Exposure =
            if ty.ends_with("Pair") || ty.ends_with("Later") {
                &OnlyB
            } else {
                &horizon_mod_api_gen::native_schema::All
            };
        let generated = horizon_mod_api_gen::native_window::generate(
            &rule,
            "horizon_client",
            None,
            &syn::parse_str(ty).unwrap(),
            quote::quote!(value),
            exposure,
        )
        .unwrap();
        let name = match ty {
            "Vec<u32>" => "vec",
            "Option<String>" => "option",
            "(u32, String)" => "tuple",
            "horizon_client::Choice" => "choice",
            "horizon_client::Pair" => "pair",
            "horizon_client::Holder" => "holder",
            "Box<Vec<u32>>" => "boxed",
            "&'static [u32]" => "slice",
            "horizon_client::Later" => "later",
            _ => "map",
        };
        std::fs::write(
            out_dir.join(format!("native_window_{name}.rs")),
            generated.to_string(),
        )
        .unwrap();
    }
}

/// Exposes only the second declared field of the sparse fixtures.
struct OnlyB;
impl horizon_mod_api_gen::native_schema::Exposure for OnlyB {
    fn name(&self) -> &str {
        "only-b"
    }
    fn field(&self, _: &str, _: Option<u32>, field: u32) -> bool {
        field == 1
    }
}
