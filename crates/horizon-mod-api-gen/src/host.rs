use super::*;

pub(super) fn generate(
    root: &Path,
    catalog: &serde_json::Value,
    code: &str,
) -> Result<(String, String), String> {
    let api_source = fs::read_to_string(root.join("crates/horizon-client/src/mod_api.rs"))
        .map_err(|e| e.to_string())?;
    let types = api_types(&api_source, code)?;
    let mut game = Vec::new();
    game_methods(
        &root.join("crates/horizon-client/src/game"),
        "horizon_client::game",
        &mut game,
    )?;
    let mut host_traits = String::new();
    let mut host_impl = String::new();
    let mut backend_reads = String::new();
    let mut backend_writes = String::new();
    let mut backend_read_impl = String::new();
    let mut backend_write_impl = String::new();
    let mut names = HashSet::new();
    let mut handles = std::collections::BTreeMap::<&str, String>::new();
    let mut helpers = catalog["helpers"]
        .as_array()
        .ok_or("catalog helpers must be an array")?
        .clone();
    let mut fragments = fs::read_dir(root.join("tools/native-hook-fragments"))
        .map_err(|e| e.to_string())?
        .map(|entry| entry.map(|entry| entry.path()).map_err(|e| e.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    fragments.sort();
    for path in fragments
        .into_iter()
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
    {
        let fragment: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).map_err(|e| e.to_string())?)
                .map_err(|e| format!("{}: {e}", path.display()))?;
        if let Some(pending) = fragment.get("helpers") {
            helpers.extend(
                pending
                    .as_array()
                    .ok_or("fragment helpers must be an array")?
                    .iter()
                    .cloned(),
            );
        }
    }
    for helper in &helpers {
        let name = helper["name"].as_str().ok_or("helper name missing")?;
        let side =
            HelperSide::parse(helper)?.ok_or_else(|| format!("helper {name}: side missing"))?;
        if !names.insert(name) {
            return Err(format!("duplicate helper {name}"));
        }
        let target = helper["target"].as_str().ok_or("helper target missing")?;
        let method = helper_target(&game, name, target)?;
        let (declaration, implementation) = host_method(method, name, &types.exports)?;
        if let Some(client_target) = helper.get("client_target") {
            let client_target = client_target
                .as_str()
                .ok_or("client_target must be a string")?;
            let client = helper_target(&game, name, client_target)?;
            let (_, client_implementation) = host_method(client, name, &types.exports)?;
            matching_helper_signatures(&implementation, &client_implementation, name)?;
        }
        let side_doc = match side {
            HelperSide::Server => "/// Side: server.\n",
            HelperSide::Client => "/// Side: client.\n",
            HelperSide::Both => "/// Side: both.\n",
        };
        let receiver = helper.get("receiver");
        let is_backend = match helper.get("backend") {
            None => false,
            Some(value) => value.as_bool().ok_or("helper backend must be a boolean")?,
        };
        if receiver.is_some() || is_backend {
            let mutable = matches!(method.sig.inputs.first(), Some(FnArg::Receiver(r)) if r.mutability.is_some());
            let (trait_methods, impl_methods) = if mutable {
                (&mut backend_writes, &mut backend_write_impl)
            } else {
                (&mut backend_reads, &mut backend_read_impl)
            };
            trait_methods.push_str(side_doc);
            trait_methods.push_str(&declaration);
            impl_methods.push_str(&implementation);
            if let Some(receiver) = receiver {
                let receiver = receiver
                    .as_str()
                    .ok_or("helper receiver must be a string")?;
                let (handle, code) = handle_method(method, name, receiver, &types.handles)?;
                let methods = handles.entry(handle).or_default();
                methods.push_str(side_doc);
                methods.push_str(&code);
            }
        } else {
            host_traits.push_str(side_doc);
            host_traits.push_str(&declaration);
            host_impl.push_str(&implementation);
        }
    }
    let handles = handles
        .iter()
        .map(|(handle, methods)| format!("impl {handle} {{\n{methods}}}\n"))
        .collect::<String>();
    struct StdTypes {
        path_buf: bool,
        arc: bool,
    }
    impl<'ast> Visit<'ast> for StdTypes {
        fn visit_type_path(&mut self, ty: &'ast syn::TypePath) {
            if ty.path.segments.len() == 1 {
                self.path_buf |= ty.path.segments[0].ident == "PathBuf";
                self.arc |= ty.path.segments[0].ident == "Arc";
            }
            syn::visit::visit_type_path(self, ty);
        }
    }
    let host_trait: syn::ItemTrait = syn::parse_str(&format!(
        "trait Host {{ {host_traits} {backend_reads} {backend_writes} }}"
    ))
    .map_err(|e| format!("generated Host: {e}"))?;
    let mut std_types = StdTypes {
        path_buf: false,
        arc: false,
    };
    std_types.visit_item_trait(&host_trait);
    let mut imports = String::new();
    if std_types.path_buf {
        imports.push_str("use std::path::PathBuf;\n");
    }
    if std_types.arc {
        imports.push_str("use std::sync::Arc;\n");
    }
    let host = format!(
        "// @generated by cargo run -p xtask -- mod-api; do not edit.\nuse super::*;\n{imports}/// Services supplied by the game to trait mods.\n/// Each method's default panics with its name, so a test double implements only what its test calls; the game implements every one.\n/// The backend accessors are the exception: they default to None, so a double without a backend answers every handle's typed refusal.\npub trait Host {{\n#[doc(hidden)] fn backend(&self) -> Option<BackendRef<'_>> {{ None }}\n#[doc(hidden)] fn backend_mut(&mut self) -> Option<BackendMutRef<'_>> {{ None }}\n{host_traits}}}\n#[doc(hidden)] pub trait Backend {{\n{backend_reads}}}\n#[doc(hidden)] pub trait BackendMut {{\n{backend_writes}}}\n{handles}"
    );
    let implementation = format!(
        "// @generated by cargo run -p xtask -- mod-api; do not edit.\nuse horizon_client::mod_api::*;\n{imports}impl Host for super::Game<'_> {{\nfn backend(&self) -> Option<BackendRef<'_>> {{ Some(BackendRef::of(self)) }}\nfn backend_mut(&mut self) -> Option<BackendMutRef<'_>> {{ Some(BackendMutRef::of(self)) }}\n{host_impl}}}\nimpl Backend for super::Game<'_> {{\n{backend_read_impl}}}\nimpl BackendMut for super::Game<'_> {{\n{backend_write_impl}}}\n"
    );
    Ok((host, implementation))
}
