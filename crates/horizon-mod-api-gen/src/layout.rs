//! Catalog-owned native hook placement, also used by the one-shot source splitter.
use crate::catalog::{Catalog, DECLARATIONS};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path};
use syn::{Item, spanned::Spanned};

/// Return catalog areas and lossless item line ranges for the source splitter.
pub fn native_hook_layout(root: &Path, source: &str) -> Result<Value, String> {
    let mut areas = BTreeMap::new();
    for area in Catalog::modules(&root.join(DECLARATIONS))? {
        let path = root.join(DECLARATIONS).join(format!("{area}.rs"));
        let catalog = Catalog::parse(&[(
            path.display().to_string(),
            fs::read_to_string(&path).map_err(|e| e.to_string())?,
        )])?;
        for target in catalog.hooks.iter().map(|h| &h.target).chain(
            catalog
                .helpers
                .iter()
                .flat_map(|h| std::iter::once(&h.target).chain(h.client_target.iter())),
        ) {
            if target.starts_with("horizon_client::game::native_hooks::") {
                let name = target.rsplit("::").next().unwrap().to_owned();
                if let Some(previous) = areas.insert(name.clone(), area.clone())
                    && previous != area
                {
                    return Err(format!("{name}: belongs to both {previous} and {area}"));
                }
            }
        }
    }
    let file = syn::parse_file(source).map_err(|e| e.to_string())?;
    let mut items = Vec::new();
    for item in file.items {
        let span = item.span();
        if let Item::Impl(item) = &item {
            let methods = item
                .items
                .iter()
                .map(|method| {
                    let name = match method {
                        syn::ImplItem::Fn(method) => method.sig.ident.to_string(),
                        _ => String::new(),
                    };
                    let span = method.span();
                    json!({"name": name, "start": span.start().line, "end": span.end().line})
                })
                .collect::<Vec<_>>();
            items.push(
                json!({"start": span.start().line, "end": span.end().line, "methods": methods}),
            );
        } else {
            items.push(json!({"start": span.start().line, "end": span.end().line}));
        }
    }
    Ok(json!({"areas": areas, "items": items}))
}

#[cfg(test)]
mod tests {
    #[test]
    fn native_hooks_live_in_their_catalog_area() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let layout = super::native_hook_layout(&root, "").unwrap();
        let areas = layout["areas"].as_object().unwrap();
        let dir = root.join("crates/horizon-client/src/game/native_hooks");
        let mut found = std::collections::BTreeSet::new();
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|ext| ext != "rs") {
                continue;
            }
            let file = syn::parse_file(&std::fs::read_to_string(&path).unwrap()).unwrap();
            for item in file.items {
                if let syn::Item::Impl(item) = item {
                    for method in item.items {
                        if let syn::ImplItem::Fn(method) = method {
                            let name = method.sig.ident.to_string();
                            if let Some(area) = areas.get(&name) {
                                assert_eq!(
                                    path.file_stem().unwrap().to_str().unwrap(),
                                    area.as_str().unwrap(),
                                    "{name}"
                                );
                                found.insert(name);
                            }
                        }
                    }
                }
            }
        }
        assert_eq!(found.len(), areas.len(), "a catalog target is missing");
    }
}
