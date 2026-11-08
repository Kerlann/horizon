//! The real catalog's traits as method tables: which methods cross as callables, each gap with
//! its reason. `TRAIT_TABLE=path` writes the full per-method table.
use horizon_mod_api_gen::{
    crossing::Rule,
    native_callable::{method_gap, methods},
};

#[test]
fn catalog_traits_cross_per_method() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let entries = horizon_mod_api_gen::internals::rule_catalog(&root).unwrap();
    let rule = Rule::new(&entries);
    let mut table = String::new();
    let mut crossing = std::collections::BTreeMap::new();
    for entry in entries.iter().filter(|entry| entry.kind == "trait") {
        let methods = methods(&rule, &entry.path);
        let mut count = 0;
        table.push_str(&format!("TRAIT {}\n", entry.path));
        for method in &methods {
            let gap = method_gap(&rule, &entry.path, &method.name.to_string());
            count += usize::from(gap.is_none());
            table.push_str(&format!(
                "  {}{} {}\n",
                method.name,
                if method.exclusive { " (&mut self)" } else { "" },
                gap.as_deref().unwrap_or("crosses")
            ));
        }
        crossing.insert(entry.path.clone(), (count, methods.len()));
    }
    if let Ok(path) = std::env::var("TRAIT_TABLE") {
        std::fs::write(path, table).unwrap();
    }
    // The 57+1 `&dyn EventLoopCtl` points: both methods cross.
    assert_eq!(
        crossing["horizon_client::game::event_loop::EventLoopCtl"],
        (2, 2)
    );
    assert_eq!(
        crossing["horizon_client_input::clipboard::Clipboard"],
        (2, 2)
    );
}

#[test]
fn catalog_callbacks_resolve_json_paths_and_name_the_engine_gap() {
    use horizon_mod_api_gen::native_callable::invocation;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let rule = Rule::new(&horizon_mod_api_gen::internals::rule_catalog(&root).unwrap());
    let callback = |module: &str, owner: Option<&str>, ty: &str, receiver: bool| {
        invocation(
            &rule,
            module,
            owner,
            &syn::parse_str(ty).unwrap(),
            quote::quote!(callee),
            receiver.then(|| quote::quote!(receiver)),
        )
        .map(|_| ())
    };
    // JSON crosses as text when written as the std path. The real module writes the alias
    // `Json` (`use serde_json::Value as Json;`), which the catalog's import facts do not
    // record for an external crate's type, so that spelling is still unresolved.
    assert_eq!(
        callback(
            "horizon_client::game::test_environments",
            None,
            "&dyn Fn(&str) -> Option<serde_json::Value>",
            false
        ),
        Ok(())
    );
    // `UiResources` owns GPU atlases: no Wire value, so the argument tuple is the gap.
    let gap = callback(
        "horizon_client::game::resource_load",
        None,
        "&mut dyn FnMut(UiResources, &std::path::Path)",
        false,
    )
    .unwrap_err();
    assert!(gap.contains("argument tuple"), "{gap}");
    // `fn(&mut Self, PlayerRef)`: the receiver binds (see tests/native_bound.rs), but `PlayerRef` is
    // a `mod_api` layout, which the rule never widens, so its argument has no codec yet.
    let player = "fn(&mut Self, mod_api::PlayerRef)";
    let module = "horizon_client::game::lan_actions::tick";
    let gap = callback(module, Some("Game"), player, true).unwrap_err();
    assert!(gap.contains("argument tuple"), "{gap}");
}
