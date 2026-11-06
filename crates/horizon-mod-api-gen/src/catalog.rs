//! The hook catalog as typed data: the declaration files under `crates/horizon-mod-api/src/hooks`, read once.
//!
//! The loader here is the only code that reads the declarations. Every generated output
//! (the `Hooks` dispatch, the Host traits, the game's entry points, the rustdoc, the mixin hook
//! names) reads these types.

use quote::ToTokens;
use serde::Deserialize;
use std::{collections::HashSet, fmt, fs, path::Path};

/// Which side of the connection a hook or helper runs on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Server,
    Client,
    Both,
}

/// What a hook's listeners may do with its event.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Contract {
    /// Told about a moment; nothing comes back.
    Listen,
    /// May veto it; the first listener to cancel stops the rest.
    Cancel,
    /// Every listener adds to, changes or removes from one output.
    Collect,
    /// The first listener to answer sets the output.
    Replace,
}

/// The host a hook takes when its receiver does not say which.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostAccess {
    /// The shared read-only `Host`.
    Read,
    /// A level host, for a receiver that is a level but not `GameLevel`.
    Level,
}

/// A hook fired in a load phase, which takes that phase's context instead of a host.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PhaseContext {
    Registration,
    DataLoad,
    /// A hook fired inside a menu, recipe or player-list loop of one side.
    Loop,
}

/// The kind of thing whose Parts a scoped hook calls.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PartKind {
    Block,
    BlockEntity,
    Item,
    Menu,
}

/// A scoped hook: a Part method, called on the Parts attached to what `key` names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PartScope {
    pub kind: PartKind,
    /// The hook argument naming the thing whose Parts are called.
    pub key: String,
}

/// The names a hook has in other mod APIs, for the rustdoc and for finding gaps.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Counterparts {
    pub neoforge: Vec<String>,
    pub fabric: Vec<String>,
    pub paper: Vec<String>,
}

/// One hook point: a moment the game tells mods about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hook {
    pub name: String,
    /// The rustdoc of its trait method.
    pub doc: String,
    pub side: Side,
    /// The game function that fires it.
    pub target: String,
    pub contract: Contract,
    /// The event field a cancel, collect or replace hook writes; `None` for a listener.
    pub out: Option<String>,
    /// A replace hook whose output also names the mod that answered.
    pub answered_by_mod: bool,
    pub scope: Option<PartScope>,
    pub host: Option<HostAccess>,
    pub context: Option<PhaseContext>,
    pub counterparts: Counterparts,
    /// The contract version an injection's `hook_version` names.
    pub version: u64,
    /// Whether the NeoForge counterpart event is cancellable, written by the
    /// stamper from the counterpart's javadoc; documentation metadata, not the
    /// contract (a hook that listens can still name a cancellable event).
    pub cancellable: Option<bool>,
    /// The parameters of a hook whose game target takes them as one generated `<Name>Call`
    /// (name and type, in order, without a `Ticks` parameter); empty for a flat target.
    pub call: Vec<(String, String)>,
}

/// A game function mods reach through the `Host` or a handle.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct Helper {
    pub name: String,
    pub target: String,
    pub side: Side,
    /// The both-side helper's client implementation, when it differs from `target`.
    pub client_target: Option<String>,
    /// The handle the helper is also a method of.
    pub receiver: Option<String>,
    #[serde(default)]
    pub backend: bool,
    /// The small service that owns this global helper, instead of the flat host.
    pub service: Option<String>,
}

/// A drawing surface of the game's that drawing hooks get as a trait object (the catalog's
/// `draw_contexts`): the surface's helpers become methods of a trait, implemented by a
/// game struct pairing `Game` with the surface, and a hook argument of the surface's type is
/// the trait object in the hook's event.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DrawContext {
    /// The game's type, as the host implementation names it (`crate::hud::HudBatch`).
    pub surface: String,
    /// The trait mods draw through.
    #[serde(rename = "trait")]
    pub trait_name: String,
    /// What its helpers' names start with, left off the trait's method names.
    pub prefix: String,
    pub doc: String,
}

/// Why the catalog was refused.
#[derive(Debug, PartialEq, Eq)]
pub enum CatalogError {
    Read {
        path: String,
        error: String,
    },
    Declaration {
        path: String,
        error: String,
    },
    NamelessHook,
    DuplicateHook(String),
    DuplicateHelper(String),
    InvalidHook {
        hook: String,
        error: String,
    },
    InvalidHelper {
        helper: String,
        error: String,
    },
    MissingDoc(String),
    Counterpart {
        hook: String,
        key: &'static str,
    },
    InvalidContract(String),
    AnsweredNeedsGlobalReplace(String),
    ScopeNeedsKey(String),
    ClientTargetNeedsBoth(String),
    NoHooks,
    Retired(String),
    ModLines {
        listed: Vec<String>,
        files: Vec<String>,
    },
}

impl fmt::Display for CatalogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, error } => write!(f, "{path}: {error}"),
            Self::Declaration { path, error } => write!(f, "{path}: {error}"),
            Self::Retired(path) => write!(
                f,
                "{path}: hooks are declared in Rust now, in {DECLARATIONS}/<area>.rs; move the entry there as a \
                 `#[hook(...)]` declaration (the old keys are the attribute's arguments) and delete this file"
            ),
            Self::ModLines { listed, files } => write!(
                f,
                "{DECLARATIONS}/mod.rs must list exactly the declaration files, in name order (so that the order is \
                 the directory's): it lists [{}], the directory holds [{}]",
                listed.join(", "),
                files.join(", ")
            ),
            Self::NamelessHook => f.write_str("hook name missing"),
            Self::DuplicateHook(name) => write!(f, "duplicate hook {name}"),
            Self::DuplicateHelper(name) => write!(f, "duplicate helper {name}"),
            Self::InvalidHook { hook, error } => write!(f, "hook {hook}: {error}"),
            Self::InvalidHelper { helper, error } => write!(f, "helper {helper}: {error}"),
            Self::MissingDoc(name) => write!(
                f,
                "hook {name}: doc missing: add `///` lines, the rustdoc of its trait method"
            ),
            Self::Counterpart { hook, key } => {
                write!(f, "hook {hook}: {key} must be a string or list of strings")
            }
            Self::InvalidContract(name) => write!(f, "invalid contract for {name}"),
            Self::AnsweredNeedsGlobalReplace(name) => {
                write!(f, "{name}: answered_by needs a global replace hook")
            }
            Self::ScopeNeedsKey(name) => write!(f, "{name} needs both scope and key, or neither"),
            Self::ClientTargetNeedsBoth(name) => {
                write!(f, "helper {name}: client_target requires side both")
            }
            Self::NoHooks => f.write_str("no native hooks"),
        }
    }
}

impl std::error::Error for CatalogError {}

impl From<CatalogError> for String {
    fn from(error: CatalogError) -> Self {
        error.to_string()
    }
}

/// A counterpart name or a list of them.
#[derive(Deserialize)]
#[serde(untagged)]
enum Names {
    One(String),
    Many(Vec<String>),
}

fn counterparts(
    hook: &str,
    key: &'static str,
    names: Option<serde_json::Value>,
) -> Result<Vec<String>, CatalogError> {
    let invalid = || CatalogError::Counterpart {
        hook: hook.to_owned(),
        key,
    };
    match names {
        None => Ok(Vec::new()),
        Some(value) => match serde_json::from_value(value).map_err(|_| invalid())? {
            Names::One(name) => Ok(vec![name]),
            Names::Many(names) => Ok(names),
        },
    }
}

/// A hook entry as the JSON spells it.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HookEntry {
    #[serde(rename = "name")]
    _name: String,
    doc: Option<String>,
    side: Side,
    target: String,
    mod_api: ModApiEntry,
    version: Option<u64>,
    cancellable: Option<bool>,
    host: Option<HostAccess>,
    context: Option<PhaseContext>,
    #[serde(default)]
    call: Vec<(String, String)>,
    neoforge: Option<serde_json::Value>,
    fabric: Option<serde_json::Value>,
    paper: Option<serde_json::Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModApiEntry {
    contract: Contract,
    out: Option<String>,
    scope: Option<PartKind>,
    key: Option<String>,
    answered_by: Option<AnsweredBy>,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum AnsweredBy {
    Mod,
}

impl Hook {
    fn parse(value: serde_json::Value) -> Result<Self, CatalogError> {
        let name = value
            .get("name")
            .and_then(serde_json::Value::as_str)
            .ok_or(CatalogError::NamelessHook)?
            .to_owned();
        let entry: HookEntry =
            serde_json::from_value(value).map_err(|error| CatalogError::InvalidHook {
                hook: name.clone(),
                error: error.to_string(),
            })?;
        let doc = entry
            .doc
            .filter(|doc| !doc.trim().is_empty())
            .ok_or_else(|| CatalogError::MissingDoc(name.clone()))?;
        let api = entry.mod_api;
        let scope = match (api.scope, api.key) {
            (None, None) => None,
            (Some(kind), Some(key)) => Some(PartScope { kind, key }),
            _ => return Err(CatalogError::ScopeNeedsKey(name)),
        };
        if matches!(
            (api.contract, &api.out),
            (Contract::Listen, Some(_))
                | (
                    Contract::Cancel | Contract::Collect | Contract::Replace,
                    None
                )
        ) {
            return Err(CatalogError::InvalidContract(name));
        }
        let answered_by_mod = api.answered_by.is_some();
        if answered_by_mod && (api.contract != Contract::Replace || scope.is_some()) {
            return Err(CatalogError::AnsweredNeedsGlobalReplace(name));
        }
        let counterparts = Counterparts {
            neoforge: counterparts(&name, "neoforge", entry.neoforge)?,
            fabric: counterparts(&name, "fabric", entry.fabric)?,
            paper: counterparts(&name, "paper", entry.paper)?,
        };
        Ok(Self {
            name,
            doc,
            side: entry.side,
            target: entry.target,
            contract: api.contract,
            out: api.out,
            answered_by_mod,
            scope,
            host: entry.host,
            context: entry.context,
            counterparts,
            version: entry.version.unwrap_or(1),
            cancellable: entry.cancellable,
            call: entry.call,
        })
    }
}

impl Helper {
    fn parse(value: serde_json::Value) -> Result<Self, CatalogError> {
        let name = value
            .get("name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let helper: Self =
            serde_json::from_value(value).map_err(|error| CatalogError::InvalidHelper {
                helper: name.clone(),
                error: error.to_string(),
            })?;
        if helper.client_target.is_some() && helper.side != Side::Both {
            return Err(CatalogError::ClientTargetNeedsBoth(name));
        }
        if let Some(service) = &helper.service {
            let error = if !matches!(
                service.as_str(),
                "commands" | "levels" | "messages" | "players" | "client_session"
            ) {
                Some("unknown service")
            } else if helper.receiver.is_some() || helper.backend {
                Some("a service helper has no receiver or backend")
            } else {
                None
            };
            if let Some(error) = error {
                return Err(CatalogError::InvalidHelper {
                    helper: name,
                    error: error.into(),
                });
            }
        }
        Ok(helper)
    }
}

/// Everything the generator reads from the catalog.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Catalog {
    /// In declaration order: the files in the order `hooks/mod.rs` lists them, each file's declarations in order.
    pub hooks: Vec<Hook>,
    pub helpers: Vec<Helper>,
    pub draw_contexts: Vec<DrawContext>,
}

/// Where the declarations live, relative to the checkout root.
pub const DECLARATIONS: &str = "crates/horizon-mod-api/src/hooks";

/// What the catalog was declared in before the declarations: a file here is refused, never ignored.
pub(crate) const RETIRED: &[&str] = &["native-hooks.json", "tools/native-hook-fragments"];

/// The game function a hook or helper named `name` is called from, when its declaration gives no `target`.
fn default_target(name: &str) -> String {
    format!("horizon_client::game::native_hooks::Game<'_>::native_{name}")
}

/// An attribute argument as the entry structs read it.
fn argument(expr: &syn::Expr) -> Result<serde_json::Value, String> {
    use serde_json::Value;
    match expr {
        syn::Expr::Lit(syn::ExprLit { lit, .. }) => match lit {
            syn::Lit::Str(text) => Ok(Value::String(text.value())),
            syn::Lit::Bool(flag) => Ok(Value::Bool(flag.value)),
            syn::Lit::Int(number) => number
                .base10_parse::<u64>()
                .map(Value::from)
                .map_err(|e| e.to_string()),
            _ => Err("expected a string, a bool or an integer".into()),
        },
        syn::Expr::Array(array) => array.elems.iter().map(argument).collect(),
        _ => Err("expected a literal or an array of literals".into()),
    }
}

/// The `key = value` arguments of the attribute named `attribute` on a declaration, as the JSON
/// object the entry structs deserialize; `None` when it carries no such attribute.
fn arguments(
    attrs: &[syn::Attribute],
    attribute: &str,
) -> Result<Option<serde_json::Map<String, serde_json::Value>>, String> {
    let Some(attr) = attrs.iter().find(|a| a.path().is_ident(attribute)) else {
        return Ok(None);
    };
    let mut map = serde_json::Map::new();
    attr.parse_nested_meta(|meta| {
        let key = meta
            .path
            .get_ident()
            .ok_or_else(|| meta.error("expected a key"))?
            .to_string();
        let value = argument(&meta.value()?.parse()?).map_err(|e| meta.error(e))?;
        map.insert(key, value);
        Ok(())
    })
    .map_err(|e| e.to_string())?;
    Ok(Some(map))
}

/// The rustdoc of a declaration: its `///` lines, one leading space off each, joined by newlines.
fn doc(attrs: &[syn::Attribute]) -> Option<String> {
    let lines: Vec<String> = attrs
        .iter()
        .filter_map(|attr| match &attr.meta {
            syn::Meta::NameValue(syn::MetaNameValue {
                path,
                value:
                    syn::Expr::Lit(syn::ExprLit {
                        lit: syn::Lit::Str(text),
                        ..
                    }),
                ..
            }) if path.is_ident("doc") => {
                let text = text.value();
                Some(text.strip_prefix(' ').unwrap_or(&text).to_owned())
            }
            _ => None,
        })
        .collect();
    (!lines.is_empty()).then(|| lines.join("\n"))
}

/// The declarations of the three kinds the files hold, as entry objects in file order.
#[derive(Default)]
struct Declared {
    hooks: Vec<serde_json::Value>,
    helpers: Vec<serde_json::Value>,
    draw_contexts: Vec<serde_json::Value>,
}

impl Declared {
    /// Reads one declaration file: its `Hooks`, `Helpers` and `DrawContexts` traits.
    fn read(&mut self, path: &str, text: &str) -> Result<(), CatalogError> {
        let invalid = |error: String| CatalogError::Declaration {
            path: path.to_owned(),
            error,
        };
        let file = syn::parse_file(text).map_err(|e| invalid(e.to_string()))?;
        for item in file.items {
            let syn::Item::Trait(declarations) = item else {
                continue;
            };
            let kind = declarations.ident.to_string();
            for member in declarations.items {
                let syn::TraitItem::Fn(function) = member else {
                    return Err(invalid(format!(
                        "{kind} holds only declarations: fn name();"
                    )));
                };
                let name = function.sig.ident.to_string();
                let attribute = match kind.as_str() {
                    "Hooks" => "hook",
                    "Helpers" => "helper",
                    "DrawContexts" => "draw_context",
                    _ => continue,
                };
                let mut entry = arguments(&function.attrs, attribute)
                    .map_err(|e| invalid(format!("{name}: {e}")))?
                    .ok_or_else(|| {
                        invalid(format!("{name}: needs a #[{attribute}(...)] attribute"))
                    })?;
                entry
                    .entry("target")
                    .or_insert_with(|| default_target(&name).into());
                match kind.as_str() {
                    "Hooks" => {
                        entry.insert("name".into(), name.clone().into());
                        let call = function
                            .sig
                            .inputs
                            .iter()
                            .map(|input| match input {
                                syn::FnArg::Typed(input) => Ok(serde_json::json!([
                                    input.pat.to_token_stream().to_string(),
                                    input.ty.to_token_stream().to_string()
                                ])),
                                syn::FnArg::Receiver(_) => {
                                    Err(invalid(format!("{name}: a declaration has no receiver")))
                                }
                            })
                            .collect::<Result<Vec<_>, _>>()?;
                        if !call.is_empty() {
                            entry.insert("call".into(), call.into());
                        }
                        if let Some(doc) = doc(&function.attrs) {
                            entry.insert("doc".into(), doc.into());
                        }
                        // The contract's arguments are the entry's `mod_api` object.
                        let mut api = serde_json::Map::new();
                        for key in ["contract", "out", "scope", "key", "answered_by"] {
                            if let Some(value) = entry.remove(key) {
                                api.insert(key.into(), value);
                            }
                        }
                        entry.insert("mod_api".into(), api.into());
                        self.hooks.push(entry.into());
                    }
                    "Helpers" => {
                        entry.insert("name".into(), name.into());
                        self.helpers.push(entry.into());
                    }
                    _ => {
                        entry.remove("target");
                        if let Some(trait_name) = entry.remove("name") {
                            entry.insert("trait".into(), trait_name);
                        }
                        entry.insert(
                            "doc".into(),
                            doc(&function.attrs).unwrap_or_default().into(),
                        );
                        self.draw_contexts.push(entry.into());
                    }
                }
            }
        }
        Ok(())
    }
}

impl Catalog {
    /// The catalog of the checkout at `root`: every file of the declarations directory, `base.rs` first and
    /// then by name, so the order never depends on a list a PR edits. `mod.rs` only lists the files for rustfmt,
    /// and must match the directory. The retired JSON catalog is refused wherever it still exists.
    pub fn load(root: &Path) -> Result<Self, CatalogError> {
        let read_error = |path: &Path, e: std::io::Error| CatalogError::Read {
            path: path.display().to_string(),
            error: e.to_string(),
        };
        for retired in RETIRED {
            let path = root.join(retired);
            if path.is_file() {
                return Err(CatalogError::Retired((*retired).to_string()));
            }
            if path.is_dir() {
                let mut entries = fs::read_dir(&path).map_err(|e| read_error(&path, e))?;
                if let Some(entry) = entries.next() {
                    let entry = entry.map_err(|e| read_error(&path, e))?;
                    return Err(CatalogError::Retired(format!(
                        "{retired}/{}",
                        entry.file_name().to_string_lossy()
                    )));
                }
            }
        }
        let dir = root.join(DECLARATIONS);
        let mut modules = fs::read_dir(&dir)
            .map_err(|e| read_error(&dir, e))?
            .map(|entry| entry.map_err(|e| read_error(&dir, e)))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                name.strip_suffix(".rs")
                    .filter(|module| *module != "mod")
                    .map(str::to_string)
            })
            .collect::<Vec<_>>();
        modules.sort_by(|a, b| (a != "base", a).cmp(&(b != "base", b)));
        let index = dir.join("mod.rs");
        let index_source = fs::read_to_string(&index).map_err(|e| read_error(&index, e))?;
        let listing = syn::parse_file(&index_source).map_err(|e| CatalogError::Declaration {
            path: index.display().to_string(),
            error: e.to_string(),
        })?;
        let listed = listing
            .items
            .iter()
            .filter_map(|item| match item {
                syn::Item::Mod(module) => Some(module.ident.to_string()),
                _ => None,
            })
            .collect::<Vec<_>>();
        if listed != modules {
            return Err(CatalogError::ModLines {
                listed,
                files: modules,
            });
        }
        let files = modules
            .iter()
            .map(|module| {
                let path = dir.join(format!("{module}.rs"));
                let text = fs::read_to_string(&path).map_err(|e| read_error(&path, e))?;
                Ok((path.display().to_string(), text))
            })
            .collect::<Result<Vec<_>, CatalogError>>()?;
        Self::parse(&files)
    }

    /// The catalog of declaration files given as (path, source), in the order given.
    pub fn parse(files: &[(String, String)]) -> Result<Self, CatalogError> {
        let mut declared = Declared::default();
        for (path, text) in files {
            declared.read(path, text)?;
        }
        let mut seen = HashSet::new();
        let hooks = declared
            .hooks
            .into_iter()
            .map(|raw| {
                let hook = Hook::parse(raw)?;
                if seen.insert(hook.name.clone()) {
                    Ok(hook)
                } else {
                    Err(CatalogError::DuplicateHook(hook.name))
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut seen = HashSet::new();
        let helpers = declared
            .helpers
            .into_iter()
            .map(|raw| {
                let helper = Helper::parse(raw)?;
                if seen.insert(helper.name.clone()) {
                    Ok(helper)
                } else {
                    Err(CatalogError::DuplicateHelper(helper.name))
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        let draw_contexts = declared
            .draw_contexts
            .into_iter()
            .map(|raw| {
                serde_json::from_value(raw).map_err(|e| CatalogError::Declaration {
                    path: "draw context".into(),
                    error: e.to_string(),
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            hooks,
            helpers,
            draw_contexts,
        })
    }
}

#[cfg(test)]
impl Hook {
    /// A global listener with no counterparts, for tests of what reads a hook.
    pub(crate) fn sample(target: &str, side: Side) -> Self {
        Self {
            name: "test".into(),
            doc: "Test.".into(),
            side,
            target: target.into(),
            contract: Contract::Listen,
            out: None,
            answered_by_mod: false,
            scope: None,
            host: None,
            context: None,
            counterparts: Counterparts::default(),
            version: 1,
            cancellable: None,
            call: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn hook(name: &str, extra: serde_json::Value) -> serde_json::Value {
        let mut hook = json!({
            "name": name, "doc": "Doc.", "side": "server",
            "target": "horizon_client::game::native_hooks::Game<'_>::native_x",
            "mod_api": {"contract": "listen"}
        });
        hook.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        hook
    }

    #[test]
    fn service_helpers_reject_unknown_services_and_handle_backends() {
        for attrs in [
            r#"service = "unknown""#,
            r#"service = "commands", receiver = "player""#,
            r#"service = "messages", backend = true"#,
        ] {
            let source =
                format!(r#"trait Helpers {{ #[helper(side = "server", {attrs})] fn example(); }}"#);
            assert!(super::Catalog::parse(&[("test.rs".into(), source)]).is_err());
        }
    }

    #[test]
    fn the_shipped_catalog_loads_with_typed_contracts() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let catalog = Catalog::load(&root).unwrap();
        let find = |name: &str| catalog.hooks.iter().find(|h| h.name == name).unwrap();
        let scoped = find("pre_remove_side_effects");
        assert_eq!(scoped.scope.as_ref().unwrap().kind, PartKind::BlockEntity);
        assert_eq!(find("recipes_received").contract, Contract::Listen);
        assert!(
            catalog
                .hooks
                .iter()
                .any(|h| h.contract == Contract::Replace)
        );
        assert!(!catalog.draw_contexts.is_empty());
    }

    fn checkout(files: &[(&str, &str)]) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "catalog-load-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&root);
        let dir = root.join(DECLARATIONS);
        fs::create_dir_all(&dir).unwrap();
        for (name, text) in files {
            let path = root.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        root
    }

    const HOOK: &str = "pub trait Hooks { /// Doc.\n #[hook(side = \"server\", contract = \"listen\")] fn one(); }";

    #[test]
    fn the_retired_json_catalog_is_refused_wherever_it_is() {
        let base = format!("{DECLARATIONS}/base.rs");
        let index = format!("{DECLARATIONS}/mod.rs");
        for retired in ["native-hooks.json", "tools/native-hook-fragments/late.json"] {
            let root = checkout(&[(&base, HOOK), (&index, "mod base;"), (retired, "{}")]);
            let error = Catalog::load(&root).unwrap_err().to_string();
            assert!(error.starts_with(retired), "{error}");
            assert!(error.contains("declared in Rust now"), "{error}");
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn the_order_comes_from_the_directory_and_mod_rs_must_match_it() {
        let second = "pub trait Hooks { /// Doc.\n #[hook(side = \"server\", contract = \"listen\")] fn two(); }";
        let base = format!("{DECLARATIONS}/base.rs");
        let alpha = format!("{DECLARATIONS}/alpha.rs");
        let index = format!("{DECLARATIONS}/mod.rs");
        let root = checkout(&[
            (&alpha, second),
            (&base, HOOK),
            (&index, "mod base;\nmod alpha;"),
        ]);
        let catalog = Catalog::load(&root).unwrap();
        let names = catalog
            .hooks
            .iter()
            .map(|h| h.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, ["one", "two"], "base.rs first, then by name");
        fs::write(root.join(&index), "mod base;").unwrap();
        let error = Catalog::load(&root).unwrap_err().to_string();
        assert!(
            error.contains("mod.rs must list exactly the declaration files"),
            "{error}"
        );
        fs::remove_dir_all(root).unwrap();
    }

    fn file(name: &str, source: &str) -> (String, String) {
        (name.to_owned(), source.to_owned())
    }

    #[test]
    fn declarations_become_entries_with_the_defaults_a_declaration_leaves_out() {
        let catalog = Catalog::parse(&[file(
            "a.rs",
            r#"
            pub trait Hooks {
                /// First line.
                ///
                ///  Indented.
                #[hook(side = "both", contract = "replace", out = "o", answered_by = "mod",
                       neoforge = ["One", "Two"], cancellable = true, version = 2)]
                fn thing();
                /// Scoped.
                #[hook(side = "server", contract = "listen", scope = "item", key = "stack",
                       target = "horizon_world::loot::native_scoped")]
                fn scoped();
            }
            pub trait Helpers {
                #[helper(side = "both", client_target = "c", backend = true)]
                fn help();
            }
            pub trait DrawContexts {
                /// Draws.
                #[draw_context(surface = "crate::s::S", name = "Draw", prefix = "d_")]
                fn draw();
            }
            "#,
        )])
        .unwrap();
        let thing = &catalog.hooks[0];
        assert_eq!(thing.doc, "First line.\n\n Indented.");
        assert_eq!(
            thing.target,
            "horizon_client::game::native_hooks::Game<'_>::native_thing"
        );
        assert!(thing.answered_by_mod && thing.cancellable == Some(true));
        assert_eq!((thing.version, thing.counterparts.neoforge.len()), (2, 2));
        assert_eq!(
            catalog.hooks[1].target,
            "horizon_world::loot::native_scoped"
        );
        assert_eq!(
            catalog.hooks[1].scope.as_ref().unwrap().kind,
            PartKind::Item
        );
        let help = &catalog.helpers[0];
        assert!(help.backend && help.client_target.as_deref() == Some("c"));
        assert_eq!(
            (
                catalog.draw_contexts[0].trait_name.as_str(),
                catalog.draw_contexts[0].prefix.as_str()
            ),
            ("Draw", "d_")
        );
    }

    #[test]
    fn a_declaration_repeating_a_hook_is_refused_even_across_files() {
        let declared = |name: &str| {
            format!(
                "pub trait Hooks {{ /// Doc.\n #[hook(side = \"server\", contract = \"listen\")] fn {name}(); }}"
            )
        };
        assert_eq!(
            Catalog::parse(&[
                file("a.rs", &declared("same")),
                file("b.rs", &declared("same"))
            ])
            .unwrap_err(),
            CatalogError::DuplicateHook("same".into())
        );
        assert_eq!(
            Catalog::parse(&[
                file("a.rs", &declared("one")),
                file("b.rs", &declared("two"))
            ])
            .unwrap()
            .hooks
            .len(),
            2
        );
    }

    #[test]
    fn a_declaration_needs_its_attribute_and_its_doc() {
        let parse = |source: &str| {
            Catalog::parse(&[file("a.rs", source)])
                .unwrap_err()
                .to_string()
        };
        assert!(
            parse("pub trait Hooks { fn bare(); }")
                .contains("bare: needs a #[hook(...)] attribute")
        );
        assert_eq!(
            parse(
                "pub trait Hooks { #[hook(side = \"server\", contract = \"listen\")] fn bare(); }"
            ),
            "hook bare: doc missing: add `///` lines, the rustdoc of its trait method"
        );
        assert!(
            parse("pub trait Hooks { /// Doc.\n #[hook(side = \"server\", contract = \"listen\", unknown = 1)] fn bare(); }")
                .contains("unknown field `unknown`")
        );
    }

    #[test]
    fn a_hook_without_a_doc_is_refused() {
        for doc in [json!(null), json!("  ")] {
            assert_eq!(
                Hook::parse(hook("bare", json!({"doc": doc})))
                    .unwrap_err()
                    .to_string(),
                "hook bare: doc missing: add `///` lines, the rustdoc of its trait method"
            );
        }
    }

    #[test]
    fn counterparts_accept_strings_lists_and_absence() {
        let one = Hook::parse(hook(
            "one",
            json!({"fabric": "Event.ONE", "neoforge": "OneEvent"}),
        ))
        .unwrap();
        assert_eq!(one.counterparts.fabric, ["Event.ONE"]);
        assert_eq!(one.counterparts.neoforge, ["OneEvent"]);
        let two =
            Hook::parse(hook("two", json!({"fabric": ["Event.TWO", "Event.THREE"]}))).unwrap();
        assert_eq!(two.counterparts.fabric.len(), 2);
        assert_eq!(
            Hook::parse(hook("none", json!({}))).unwrap().counterparts,
            Counterparts::default()
        );
        for key in ["fabric", "neoforge", "paper"] {
            assert_eq!(
                Hook::parse(hook("bad", json!({key: ["ok", 4]})))
                    .unwrap_err()
                    .to_string(),
                format!("hook bad: {key} must be a string or list of strings")
            );
        }
    }

    #[test]
    fn a_hook_needs_a_side_a_target_and_a_contract() {
        for side in [json!(null), json!("invalid")] {
            assert!(Hook::parse(hook("bad", json!({"side": side}))).is_err());
        }
        for field in ["side", "target", "mod_api"] {
            let mut value = hook("bad", json!({}));
            value.as_object_mut().unwrap().remove(field);
            assert!(
                Hook::parse(value)
                    .unwrap_err()
                    .to_string()
                    .contains(&format!("missing field `{field}`"))
            );
        }
        assert_eq!(
            Hook::parse(json!({})).unwrap_err(),
            CatalogError::NamelessHook
        );
    }

    #[test]
    fn contracts_and_outputs_go_together() {
        let parse = |contract: &str, out: Option<&str>| {
            Hook::parse(hook(
                "c",
                json!({"mod_api": {"contract": contract, "out": out}}),
            ))
        };
        assert_eq!(parse("listen", None).unwrap().contract, Contract::Listen);
        for contract in ["cancel", "collect", "replace"] {
            assert_eq!(
                parse(contract, Some("o")).unwrap().out.as_deref(),
                Some("o")
            );
            assert_eq!(
                parse(contract, None).unwrap_err(),
                CatalogError::InvalidContract("c".into())
            );
        }
        assert_eq!(
            parse("listen", Some("o")).unwrap_err(),
            CatalogError::InvalidContract("c".into())
        );
        assert!(parse("unknown", None).is_err());
    }

    #[test]
    fn a_scope_and_its_key_go_together() {
        let scoped = |api: serde_json::Value| Hook::parse(hook("s", json!({"mod_api": api})));
        let hook_scope = scoped(json!({"contract": "listen", "scope": "item", "key": "stack"}))
            .unwrap()
            .scope
            .unwrap();
        assert_eq!(
            (hook_scope.kind, hook_scope.key.as_str()),
            (PartKind::Item, "stack")
        );
        for api in [
            json!({"contract": "listen", "scope": "item"}),
            json!({"contract": "listen", "key": "stack"}),
        ] {
            assert_eq!(
                scoped(api).unwrap_err(),
                CatalogError::ScopeNeedsKey("s".into())
            );
        }
        assert!(scoped(json!({"contract": "listen", "scope": "nothing", "key": "k"})).is_err());
    }

    #[test]
    fn answered_by_needs_a_global_replace_hook() {
        let answered = |api: serde_json::Value| Hook::parse(hook("a", json!({"mod_api": api})));
        assert!(
            answered(json!({"contract": "replace", "out": "o", "answered_by": "mod"}))
                .unwrap()
                .answered_by_mod
        );
        for api in [
            json!({"contract": "cancel", "out": "o", "answered_by": "mod"}),
            json!({"contract": "replace", "out": "o", "answered_by": "mod", "scope": "item", "key": "k"}),
        ] {
            assert_eq!(
                answered(api).unwrap_err(),
                CatalogError::AnsweredNeedsGlobalReplace("a".into())
            );
        }
    }

    #[test]
    fn a_client_target_needs_a_both_side_helper() {
        let helper = |side: &str| {
            Helper::parse(json!({"name": "h", "target": "t", "side": side, "client_target": "c"}))
        };
        assert_eq!(helper("both").unwrap().client_target.as_deref(), Some("c"));
        assert_eq!(
            helper("server").unwrap_err(),
            CatalogError::ClientTargetNeedsBoth("h".into())
        );
        assert!(Helper::parse(json!({"name": "h", "target": "t"})).is_err());
    }
}
