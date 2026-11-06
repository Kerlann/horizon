//! Cargo-facing generation: the mod API's and the game's generated files.
use super::{Catalog, Target, free_function_source, native_hook_files, sources};
use quote::ToTokens;
use std::{fs, path::Path};

/// Generate only this crate's files, tracking every source read by the generator.
/// `root` is supplied at run time by the consuming crate's build script.
pub fn generate(root: &Path, out_dir: &Path, target: Target) -> Result<(), String> {
    track(root)?;
    let generated = sources(root, super::FillMode::Require)?;
    let outputs = match target {
        Target::Client => {
            let mut modules = String::new();
            for path in native_hook_files(root)? {
                let module = path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .ok_or_else(|| {
                        format!("{}: invalid native hook module name", path.display())
                    })?;
                if matches!(module, "mod" | "shared") {
                    continue;
                }
                syn::parse_str::<syn::Ident>(module).map_err(|e| e.to_string())?;
                let path = fs::canonicalize(&path).map_err(|e| e.to_string())?;
                modules.push_str(&format!("#[path = {path:?}] mod {module};\n"));
            }
            fs::write(out_dir.join("native_hook_modules.rs"), modules)
                .map_err(|e| e.to_string())?;
            vec![
                ("mod_api_host_generated.rs", generated.implementation),
                ("mod_api_fire_generated.rs", generated.fire),
                // The mob tick's goal read view (#1077), included beside the
                // engine's `GoalCx` in `mobs/part_goals.rs`.
                ("mod_api_goal_view_generated.rs", generated.goal_view),
            ]
        }
        Target::Api => {
            for (name, code) in super::exports::sources(root)? {
                fs::write(out_dir.join(name), code).map_err(|e| e.to_string())?;
            }
            let hooks = generated.hooks;
            let included = [
                ("mod_api_generated.rs", hooks.as_str()),
                ("mod_api_generated_host.rs", generated.host.as_str()),
                ("native_render_generated.rs", generated.render.as_str()),
            ];
            // The resolver recurses through the API's types: give it a deep stack.
            let wasm = std::thread::scope(|scope| {
                std::thread::Builder::new()
                    .stack_size(64 << 20)
                    .spawn_scoped(scope, || super::wasm::generate(root, &included))
                    .map_err(|e| e.to_string())?
                    .join()
                    .map_err(|_| "generating the wasm interface panicked".to_string())?
            })?;
            for path in &wasm.sources {
                println!("cargo:rerun-if-changed={}", path.display());
            }
            fs::write(out_dir.join("mod-api.wit"), &wasm.wit).map_err(|e| e.to_string())?;
            vec![
                ("mod_api_generated.rs", hooks),
                ("mod_api_generated_host.rs", generated.host),
                ("native_render_generated.rs", generated.render),
                ("capability_providers_generated.rs", wasm.providers),
                ("wasm_guest_generated.rs", wasm.guest),
                ("wasm_host_generated.rs", wasm.host),
            ]
        }
    };
    for (name, code) in outputs {
        fs::write(out_dir.join(name), &code).map_err(|e| e.to_string())?;
        syn::parse_file(&code).map_err(|e| format!("generated Rust: {name}: {e}"))?;
    }
    Ok(())
}

/// `horizon-mod-points`' generated file: each point's record, `PointId` and the `Points` trait,
/// from the game's source, tracking the game module's files.
pub fn generate_points(root: &Path, out_dir: &Path) -> Result<(), String> {
    let src = root.join("crates/horizon-client/src");
    println!("cargo:rerun-if-changed={}", src.join("game.rs").display());
    println!("cargo:rerun-if-changed={}", src.join("game").display());
    for path in super::points::tracked(root)? {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    let code = super::points::records(&super::points::game_points(root)?);
    let generated = syn::parse_file(&code).map_err(|e| format!("generated Rust: points: {e}"))?;
    // A single multi-megabyte line makes Clippy's source lookups prohibitively slow.
    let mut code = String::new();
    for item in generated.items {
        code.push_str(&item.to_token_stream().to_string());
        code.push('\n');
    }
    fs::write(out_dir.join("points_generated.rs"), code).map_err(|e| e.to_string())
}

pub(super) fn track(root: &Path) -> Result<(), String> {
    fn directory(path: &Path, extension: &str, recursive: bool) -> Result<(), String> {
        // Track the directory as well: adding a helper source or fragment changes the inputs.
        println!("cargo:rerun-if-changed={}", path.display());
        for entry in fs::read_dir(path).map_err(|e| e.to_string())? {
            let path = entry.map_err(|e| e.to_string())?.path();
            if path.is_dir() && recursive {
                directory(&path, extension, recursive)?;
            } else if path.extension().is_some_and(|ext| ext == extension) {
                println!("cargo:rerun-if-changed={}", path.display());
            }
        }
        Ok(())
    }
    println!(
        "cargo:rerun-if-changed={}",
        root.join("crates/horizon-mod-api/src/lib.rs").display()
    );
    for retired in super::catalog::RETIRED {
        // Only while present: Cargo reruns a build script on every build for a path that doesn't
        // exist. One that appears fails the next generation, and every fresh build, such as CI's.
        let path = root.join(retired);
        if path.exists() {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
    directory(&root.join(super::catalog::DECLARATIONS), "rs", false)?;
    directory(&root.join(super::exports::DIRECTORY), "rs", false)?;
    directory(&root.join("crates/horizon-client/src/game"), "rs", true)?;
    println!(
        "cargo:rerun-if-changed={}",
        root.join("crates/horizon-client/src/game.rs").display()
    );
    for hook in Catalog::load(root)?.hooks {
        if let Some((krate, path)) = free_function_source(&hook.target) {
            let (module, _) = path.rsplit_once("::").ok_or("target missing module")?;
            let path = root
                .join("crates")
                .join(krate)
                .join("src")
                .join(format!("{}.rs", module.replace("::", "/")));
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
    Ok(())
}
