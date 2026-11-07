//! Guest exports for validated, API-typed function-slot declarations.
use super::GamePoint;
use quote::{format_ident, quote};
use std::path::Path;

/// An already-validated API head/return declaration, projected from the package manifest.
/// `with` names a function in the guest crate root.
pub struct GuestInject<'a> {
    pub id: &'a str,
    pub at: &'a str,
    pub at_return: bool,
    pub with: &'a str,
}

/// Generate the one listeners export from a mod's declarations and its pinned game's catalog.
/// Manifest parsing and target/kind validation belong to the caller, not this generator.
pub fn guest_exports(root: &Path, entries: &[GuestInject<'_>]) -> Result<String, String> {
    emit(&super::game_points(root)?, entries)
}

fn emit(points: &[GamePoint], entries: &[GuestInject<'_>]) -> Result<String, String> {
    let mut subscriptions = Vec::new();
    let mut calls = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        let point = points
            .iter()
            .find(|point| super::plain_path(&point.target) == super::plain_path(entry.at))
            .filter(|point| !entry.at_return || point.at_return)
            .ok_or_else(|| {
                format!(
                    "E_INJECT_NO_SLOT: {} has no API {} slot",
                    entry.at,
                    if entry.at_return { "return" } else { "head" }
                )
            })?;
        let name = entry.id;
        let record = format_ident!("{}", point.record);
        let record = if point.args.iter().any(|(_, ty)| ty.starts_with('&')) {
            quote!(mod_points::#record<'_>)
        } else {
            quote!(mod_points::#record)
        };
        let with = format_ident!("{}", entry.with);
        let call = if point.host == "none" {
            quote!(|_: (), args: &mut #record| crate::#with(args))
        } else {
            quote!(|host, args: &mut #record| crate::#with(host, args))
        };
        subscriptions.push(quote!((#name.into(), <#record as mod_points::__private::wire::Lift<'_>>::SHAPE.into())));
        let index = index as u32;
        calls.push(quote!(#index => mod_points::__private::wire::fire(host, &fields, #call),));
    }
    let code = quote! {
        struct HorizonInjects;
        impl mod_points::__private::wire::Guest for HorizonInjects {
            fn subscribed() -> Vec<(String, String)> { vec![#(#subscriptions),*] }
            fn fire(place: u32, host: mod_points::__private::wire::HostKind, fields: Vec<mod_points::__private::wire::Value>) -> Vec<mod_points::__private::wire::Value> {
                match place { #(#calls)* _ => panic!("the game fired an undeclared inject") }
            }
        }
        mod_points::__private::wire::bindings::export_listeners!(HorizonInjects with_types_in mod_points::__private::wire::bindings);
    };
    syn::parse2(code)
        .map(|file| prettyplease::unparse(&file))
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, process::Command};

    #[test]
    fn manifest_functions_are_dispatched_by_export_index() {
        let point = GamePoint {
            name: "read".into(),
            target: "game::read".into(),
            side: "none",
            host: "none",
            record: "ReadArgs".into(),
            args: vec![("value".into(), "u32".into())],
            returns: "none",
            at_return: true,
            raw: false,
            listener: None,
            return_listener: None,
            record_item: String::new(),
            api: Default::default(),
            wire_shape: String::new(),
        };
        let entries = [
            GuestInject {
                id: "before_id",
                at: "game::read",
                at_return: false,
                with: "before",
            },
            GuestInject {
                id: "after_id",
                at: "game::read",
                at_return: true,
                with: "after",
            },
        ];
        let generated = emit(&[point], &entries).unwrap();
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../../target/guest-injects-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let source = format!(
            r#"
            #[macro_export] macro_rules! export_listeners {{ ($($t:tt)*) => {{}}; }}
            mod mod_points {{
                pub struct ReadArgs(pub u32);
                pub mod __private {{ pub mod wire {{
                    pub type Value = u32; pub type HostKind = ();
                    pub trait Lift<'a> {{ const SHAPE: &'static str; }}
                    impl<'a> Lift<'a> for super::super::ReadArgs {{ const SHAPE: &'static str = "value:u32"; }}
                    pub trait Guest {{ fn subscribed() -> Vec<(String,String)>; fn fire(place:u32, host:HostKind, fields:Vec<Value>) -> Vec<Value>; }}
                    pub fn fire(_: (), fields: &[u32], call: impl FnOnce((), &mut super::super::ReadArgs)) -> Vec<u32> {{ let mut args = super::super::ReadArgs(fields[0]); call((), &mut args); vec![args.0] }}
                    pub mod bindings {{ pub use crate::export_listeners; }}
                }} }}
            }}
            fn before(args: &mut mod_points::ReadArgs) {{ args.0 += 7; }}
            fn after(args: &mut mod_points::ReadArgs) {{ args.0 *= 2; }}
            {generated}
            fn main() {{
                use mod_points::__private::wire::Guest;
                assert_eq!(HorizonInjects::subscribed(), [("before_id".into(), "value:u32".into()), ("after_id".into(), "value:u32".into())]);
                assert_eq!(HorizonInjects::fire(0, (), vec![3]), [10]);
                assert_eq!(HorizonInjects::fire(1, (), vec![3]), [6]);
            }}
        "#
        );
        fs::write(root.join("guest.rs"), source).unwrap();
        let binary = root.join(format!("guest{}", std::env::consts::EXE_SUFFIX));
        let built = Command::new("rustc")
            .args(["--edition=2024"])
            .arg(root.join("guest.rs"))
            .arg("-o")
            .arg(&binary)
            .output()
            .unwrap();
        assert!(
            built.status.success(),
            "{}",
            String::from_utf8_lossy(&built.stderr)
        );
        assert!(Command::new(binary).status().unwrap().success());
        assert!(
            emit(&[], &entries)
                .unwrap_err()
                .contains("E_INJECT_NO_SLOT")
        );
        fs::remove_dir_all(root).unwrap();
    }
}
