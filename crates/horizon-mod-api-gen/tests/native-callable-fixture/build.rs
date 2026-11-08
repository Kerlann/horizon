use horizon_mod_api_gen::{crossing::Rule, internals::Internal, native_callable::codecs};
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
    let entries = [
        entry(
            "Record",
            "struct",
            "struct Record { choice: Choice, values: Vec<u16> }",
        ),
        entry(
            "Choice",
            "enum",
            "enum Choice { Empty, Number(u32), Named { flag: bool } }",
        ),
    ];
    let rule = Rule::new(&entries);
    let shape = |ty: &str| rule.shape("horizon_client::model", None, &syn::parse_str(ty).unwrap());
    let generated = codecs(
        &rule,
        &[shape("Record"), shape("Choice"), shape("Vec<u16>")],
        &shape("Record"),
    )
    .unwrap();
    let decode = generated.decode;
    let encode = generated.encode;
    let file = quote! {
        fn source<F>(root: &mod_points::snapshot::Root<'_, std::cell::RefCell<F>>) -> Result<std::sync::Arc<dyn mod_api::RowFunction>, mod_api::RowError>
        where F: FnMut((model::Record, model::Choice, Vec<u16>)) -> model::Record {
            unsafe { mod_points::callable::FunctionSource::new(root, #decode, #encode) }
        }
    };
    fs::write(
        PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("callable.rs"),
        file.to_string(),
    )
    .unwrap();
}
