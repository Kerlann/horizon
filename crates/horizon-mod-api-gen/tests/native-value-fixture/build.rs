use horizon_mod_api_gen::{crossing::Rule, internals::Internal, native_value::codec};
use quote::quote;
use std::{collections::BTreeMap, fs, path::PathBuf};

fn entry(name: &str, kind: &'static str, signature: &str) -> Internal {
    Internal {
        path: format!("horizon_client::model::{name}"),
        kind,
        signature: signature.into(),
        cfg: String::new(),
        hash: String::new(),
        shape_only: false,
        opaque_reason: None,
        refs: BTreeMap::new(),
    }
}
fn main() {
    let mut entries = [
        entry(
            "Record",
            "struct",
            "struct Record { choice: Choice, maybe: Option<Box<u32>>, list: Vec<u8>, array: [u16; 2], deque: VecDeque<u8>, set: BTreeSet<u8>, hash_set: HashSet<u8>, map: BTreeMap<u8, u16>, hash_map: HashMap<u8, u16> }",
        ),
        entry(
            "Choice",
            "enum",
            "enum Choice { Empty, Pair(u32, bool), Named { flag: bool } }",
        ),
    ];
    for name in ["VecDeque", "BTreeSet", "HashSet", "BTreeMap", "HashMap"] {
        entries[0]
            .refs
            .insert(name.into(), format!("std::collections::{name}"));
    }
    let rule = Rule::new(&entries);
    let ty = syn::parse_str("Record").unwrap();
    let shape = rule.shape("horizon_client::model", None, &ty);
    let codec = codec(&rule, &shape).unwrap();
    let decode = codec.decode;
    let encode = codec.encode;
    let file = quote! {
        fn decode_record(input: &mut mod_api::wire::Reader<'_>) -> Result<model::Record, mod_api::wire::WireError> { Ok(#decode) }
        fn encode_record(value: &model::Record, out: &mut mod_api::wire::Writer) { #encode }
    };
    fs::write(
        PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("codec.rs"),
        file.to_string(),
    )
    .unwrap();
}
