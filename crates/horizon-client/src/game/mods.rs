//! Mods in the game (`docs/modding/src/how-to/content-mods.md`): content mods' blocks, items, data and
//! assets (`content_root`), and the Mods page's rows. Code mods are native (`native_hooks.rs`)
//! or wasm (`horizon-mod-wasm`, loaded at startup); any other executable file is refused.
use super::{Game, execution_role::ExecutionRole};
use horizon_client::mod_list::{self, Installed};
use horizon_mods::manifest::{MissingJar, collect_jar_requirements, missing_jars};
use horizon_world::{
    item_defaults::ItemData,
    native_load_context::{LoadContext, LoadInputs},
};
use mod_api::__private::game::*;
use mod_api::RegistrationContext;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

#[derive(Clone, Default)]
pub(super) struct ModsState {
    /// `--dev-mod PATH`: content mods always on.
    pub(super) dev: Vec<PathBuf>,
    pub(super) native: std::sync::Arc<horizon_mods::runtime::Runtime>,
    pub(super) selected: std::sync::Arc<[horizon_mods::selection::SelectedPackage]>,
    /// Structured load refusals, rendered only at the existing Mods-page boundary.
    pub(super) refused: Arc<Mutex<Vec<MissingJar>>>,
}

impl ModsState {
    pub(super) fn select(
        &mut self,
        game_dir: &std::path::Path,
        enabled: &std::collections::BTreeSet<String>,
    ) {
        self.selected = horizon_mods::selection::content_packages(game_dir, enabled, &self.dev)
            .filter(|selected| {
                let id = &selected.package.manifest.id;
                // A refused wasm mod's content stays out with its code.
                !self.native.built().iter().any(|built| built.id == *id)
                    && !horizon_mod_wasm::refused()
                        .iter()
                        .any(|(refused, _)| refused == id)
            })
            .collect();
        self.refused = Default::default();
    }

    fn report_refusals(&self, rows: &mut [Installed]) {
        let refused = self.refused.lock().unwrap();
        for row in rows {
            let problems: Vec<_> = refused
                .iter()
                .filter(|missing| missing.owners.contains(&row.id))
                .map(ToString::to_string)
                .collect();
            if !problems.is_empty() {
                row.problem = Some(problems.join("\n"));
            }
        }
    }
}

/// Capture load inputs at the resource-generation boundary, before workers share them.
pub(super) fn load_item_data(root: &Path, role: ExecutionRole) -> horizon_client::Result<ItemData> {
    let mut registries: BTreeMap<_, _> = builtin_registries(root)?
        .registries
        .into_iter()
        .map(|(name, ids)| (name, ids.names))
        .collect();
    if let Some(types) = registries.get_mut("minecraft:data_component_type") {
        types.extend(horizon_protocol::item::custom::entries().map(|(_, name)| name.to_owned()));
    }
    let inputs = LoadInputs::new(registries, Game::mod_data_root());
    let context = LoadContext::new(
        role.data_load_origin(),
        Arc::new(inputs),
        mod_api::__private::loaded_mods().ids(),
    );
    Ok(ItemData::load(root, context)?)
}

fn builtin_registries(root: &Path) -> horizon_client::Result<horizon_protocol::registry::Builtin> {
    let path = horizon_protocol::registry::Builtin::find(root).ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "registries.json not found")
    })?;
    Ok(horizon_protocol::registry::Builtin::load(&path)?)
}

/// Freeze native component IDs before reading content or opening a connection.
/// With no mod registering any, nothing is installed, which every reader
/// treats as no custom components, and `registries.json` isn't read.
pub(super) fn register_components(root: &std::path::Path) -> horizon_client::Result<()> {
    let mut names = Vec::new();
    Game::native_component_types(&RegistrationContext::new(), &mut names);
    if names.is_empty() {
        return Ok(());
    }
    let builtin = builtin_registries(root)?;
    let types = builtin
        .get("minecraft:data_component_type")
        .ok_or("missing data component registry")?;
    let first = i32::try_from(types.names.len())?;
    horizon_protocol::item::custom::install(first, names)
        .map_err(|error| format!("invalid native component registration: {error}"))?;
    Ok(())
}

struct NativeDeclared {
    blocks: Vec<horizon_mods::content::Block>,
    items: Vec<horizon_mods::content::Item>,
    types: Vec<horizon_mods::content::BlockEntityType>,
}

/// The same native declarations serve startup and every pack reload.
fn native_declared() -> &'static NativeDeclared {
    static DECLARED: std::sync::OnceLock<NativeDeclared> = std::sync::OnceLock::new();
    DECLARED.get_or_init(|| {
        let (mut blocks, mut items) = (Vec::new(), Vec::new());
        Game::native_blocks(&RegistrationContext::new(), &mut blocks);
        Game::native_items(&RegistrationContext::new(), &mut items);
        let types = mod_api::__private::block_entity_types().to_vec();
        NativeDeclared {
            blocks,
            items,
            types,
        }
    })
}
/// The native mods' standalone models (`native_standalone_models`), set once
/// before the resources first load.
pub(super) fn register_standalone_models(role: ExecutionRole) {
    if let Some(names) = collect_standalone_models(role) {
        horizon_client::models::register_standalone(names);
    }
}

fn collect_standalone_models(role: ExecutionRole) -> Option<Vec<String>> {
    if !role.is_client() {
        return None;
    }
    let mut names = Vec::new();
    Game::native_standalone_models(&RegistrationContext::new(), &mut names);
    Some(names)
}

/// `root` with the content mods that are on layered over it
/// (docs/modpacks.md): their blocks, items, data and assets. Read once at
/// start, before the resources load; a mod whose content is refused is left
/// out here and its row on the Mods page says why.
/// The merged content root and the accepted content mods behind it, before
/// any process-wide registration. A caller that only reads the overlay (the
/// reload tests) must not freeze the mod registries for the rest of the
/// process.
pub(super) struct ContentOverlay {
    /// The root with the mods' content merged in.
    pub(super) root: std::path::PathBuf,
    /// The accepted content mods, in overlay order.
    pub(super) mods: Vec<horizon_client::mod_content::ContentMod>,
    /// The protocol entries of the accepted mods.
    pub(super) announced: Vec<horizon_protocol::mod_list::Entry>,
    /// Whether the protocol's local mod list is still unset.
    pub(super) announce: bool,
}

/// Builds the content overlay: every enabled or dev content mod's files
/// merged over `root`. Registration of what the mods declare is
/// [`content_root`]'s step.
pub(super) fn content_overlay(
    root: &std::path::Path,
    sources: &ModsState,
    jar_store: &std::path::Path,
) -> horizon_client::Result<ContentOverlay> {
    use horizon_client::mod_content::{self, ContentMod};
    use horizon_mods::Content;
    let here = std::env::current_dir().unwrap_or_default();

    let requirements = collect_jar_requirements(
        sources
            .native
            .packages()
            .iter()
            .map(|p| &p.manifest)
            .chain(sources.selected.iter().map(|p| &p.package.manifest))
            .map(|m| (m.id.as_str(), m.jar.as_slice())),
    )?;
    let refused = missing_jars(jar_store, &requirements)?;
    *sources.refused.lock().unwrap() = refused.clone();
    if let Some(missing) = refused.iter().find(|missing| {
        missing
            .owners
            .iter()
            .any(|id| sources.native.built().iter().any(|built| built.id == id))
    }) {
        return Err(missing.clone().into());
    }

    let mut known = None;
    let mut mods: Vec<ContentMod> = Vec::new();
    let announce = !horizon_protocol::mod_list::is_set();
    let mut announced: Vec<_> = if announce {
        sources
            .native
            .built()
            .iter()
            .map(|m| announced_entry(m.id, m.version, m.side))
            .collect()
    } else {
        Vec::new()
    };
    struct PackageSource<'a> {
        path: &'a std::path::Path,
        manifest: &'a horizon_mods::Manifest,
        content_only: bool,
    }
    let native = sources.native.packages().iter().map(|p| PackageSource {
        path: &p.path,
        manifest: &p.manifest,
        content_only: false,
    });
    let content = sources.selected.iter().map(|p| PackageSource {
        path: &p.path,
        manifest: &p.package.manifest,
        content_only: true,
    });
    for source in native.chain(content) {
        let m = source.manifest;
        // Only content packages can be omitted independently: none has code.
        if refused.iter().any(|missing| missing.owners.contains(&m.id)) {
            continue;
        }
        if mods.iter().any(|c| c.id == m.id) {
            continue;
        }
        // The items this mod's native code declares may be a file item's crafting remainder.
        let declared: Vec<String> = native_declared()
            .items
            .iter()
            .filter(|i| i.id.split_once(':').is_some_and(|(ns, _)| ns == m.id))
            .map(|i| i.id.clone())
            .collect();
        let content = match Content::read_declaring(source.path, m, &declared) {
            Ok(content) => content,
            Err(e) => {
                log::warn!("Horizon: mod {} content not loaded: {e}", m.id);
                continue;
            }
        };
        let known = match &mut known {
            Some(types) => types,
            None => match mod_content::known_components(root) {
                Ok(types) => known.insert(types),
                Err(e) => {
                    log::warn!("Horizon: content mods not loaded: {e}");
                    return Err(e);
                }
            },
        };
        let candidate = ContentMod {
            id: m.id.clone(),
            content,
            jars: m.jar.clone(),
        };
        match mod_content::accept_mod(&mut mods, candidate, known) {
            Ok(_) if announce && source.content_only => {
                announced.push(announced_entry(&m.id, &m.version, m.side))
            }
            Ok(_) => {}
            Err(e) => log::warn!("Horizon: mod {} content not loaded: {e}", m.id),
        }
    }
    let NativeDeclared {
        blocks,
        items,
        types,
    } = native_declared();
    if !blocks.is_empty() || !items.is_empty() || !types.is_empty() {
        let known = match &mut known {
            Some(types) => types,
            None => known.insert(mod_content::known_components(root)?),
        };
        let errors = mod_content::accept_declared(&mut mods, blocks, items, types, known);
        if !errors.is_empty() {
            return Err(errors.join("\n").into());
        }
        // A file item's remainder named a code-declared item that was refused: that mod
        // alone is left out, not every content mod through the overlay's check.
        for e in mod_content::drop_dangling_remainders(&mut mods) {
            log::warn!("Horizon: {e}");
        }
    }
    let root = mod_content::resource_root(root, jar_store, &mods, &here.join(".horizon-cache"))?;
    Ok(ContentOverlay {
        root,
        mods,
        announced,
        announce,
    })
}

/// The content root with everything the mods declare registered against the
/// process: block entities, mob effects, entity and menu types, fluid looks,
/// and the protocol's local mod list, each set once.
pub(super) fn content_root(
    root: &std::path::Path,
    sources: &ModsState,
) -> horizon_client::Result<std::path::PathBuf> {
    let jar_store = horizon_client::logging::game_home().join("jars");
    let overlay = content_overlay(root, sources, &jar_store)?;
    register_block_entity_types(&overlay.root, &overlay.mods)?;
    if overlay.announce {
        horizon_protocol::mod_list::set_local(overlay.announced);
    }
    if overlay.root != root {
        log::info!(
            "Horizon: content from {} mod(s) in {}",
            overlay.mods.len(),
            overlay.root.display()
        );
    }
    register_mob_effects(&overlay.mods);
    register_entity_types(&overlay.mods);
    register_menu_types(&overlay.mods);
    horizon_client::mod_content::set_fluid_looks(&overlay.mods);
    Ok(overlay.root)
}

fn register_block_entity_types(
    root: &std::path::Path,
    mods: &[horizon_client::mod_content::ContentMod],
) -> horizon_client::Result<()> {
    use horizon_client::block_entity_types::{self, BlockEntityTypes};
    use horizon_protocol::registry::Builtin;
    let path = Builtin::find(root).ok_or("registries.json not found")?;
    let builtin = Builtin::load(&path)?;
    let names = &builtin
        .get("minecraft:block_entity_type")
        .ok_or("missing block entity type registry")?
        .names;
    let membership = mods
        .iter()
        .flat_map(|m| &m.content.block_entity_types)
        .flat_map(|kind| {
            kind.blocks
                .iter()
                .map(|block| (block.clone(), kind.id.clone()))
        })
        .collect();
    let types = BlockEntityTypes::new(membership, names.clone());
    mod_api::__private::validate_block_entity_type_keys(&types)?;
    block_entity_types::install(types)?;
    Ok(())
}

/// Manifest identity for the configuration-phase mod check.
fn announced_entry(
    id: &str,
    version: &str,
    side: horizon_mods::Side,
) -> horizon_protocol::mod_list::Entry {
    use horizon_mods::Side;
    use horizon_protocol::mod_list;
    mod_list::Entry {
        id: id.to_owned(),
        version: version.to_owned(),
        side: match side {
            Side::Both => mod_list::Side::Both,
            Side::Client => mod_list::Side::Client,
        },
    }
}

/// The mods' mob effects join vanilla's (`brewing::effect`), once the root
/// holding their network ids is built. The first root that declares effects
/// sets them, until restart: a pack reload after a Mods switch builds a root
/// with others. A root declaring none freezes nothing, and a game without
/// mob effects reads the same empty list either way.
fn register_mob_effects(mods: &[horizon_client::mod_content::ContentMod]) {
    use horizon_client::brewing::{self, Category};
    use horizon_mods::content::EffectCategory;
    let effects = horizon_client::mod_content::mob_effects(mods);
    if effects.is_empty() {
        return;
    }
    let ids: Vec<String> = effects.iter().map(|e| e.id.clone()).collect();
    let set = brewing::register_mod_effects(effects.into_iter().map(|e| {
        let category = match e.category {
            EffectCategory::Beneficial => Category::Beneficial,
            EffectCategory::Harmful => Category::Harmful,
            EffectCategory::Neutral => Category::Neutral,
        };
        (e.id.clone(), e.color, category)
    }));
    if !set
        && brewing::mod_effects()
            .iter()
            .map(|e| e.id)
            .ne(ids.iter().map(String::as_str))
    {
        log::warn!("Horizon: the mods' mob effects changed; restart the game to use them");
    }
}

/// the mods' thrown entity types, as their mob effects: set once, from the
/// startup root. Living types are mobs, loaded with vanilla's from the
/// content root's `entity-types.json`.
fn register_entity_types(mods: &[horizon_client::mod_content::ContentMod]) {
    use horizon_client::projectiles;
    let mut types = horizon_client::mod_content::entity_types(mods);
    types.retain(|t| t.item.is_some());
    let set = projectiles::register_thrown_types(
        types
            .iter()
            .filter_map(|t| Some((t.id.clone(), t.item.clone()?, t.damage))),
    );
    if !set
        && projectiles::thrown_types()
            .iter()
            .map(|t| t.id)
            .ne(types.iter().map(|t| t.id.as_str()))
    {
        log::warn!("Horizon: the mods' entity types changed; restart the game to use them");
    }
}

/// the mods' menu types, as their entity types: set once, from the startup
/// root.
fn register_menu_types(mods: &[horizon_client::mod_content::ContentMod]) {
    use horizon_client::containers;
    let menus = horizon_client::mod_content::menu_types(mods);
    let set = containers::register_mod_menu_types(menus.iter().map(|m| {
        let results = m.results.iter().map(|&r| usize::from(r)).collect();
        (m.id.clone(), m.slots.into(), m.data_slots.into(), results)
    }));
    if !set
        && containers::mod_menu_types()
            .iter()
            .map(|m| m.id)
            .ne(menus.iter().map(|m| m.id.as_str()))
    {
        log::warn!("Horizon: the mods' menu types changed; restart the game to use them");
    }
}

/// The embedded inventory, formatted for the crash report.
fn native_mods_detail(built: &[horizon_mods::runtime::BuiltMod]) -> Option<String> {
    let list = built
        .iter()
        .map(|m| format!("{} {}", m.id, m.version))
        .collect::<Vec<_>>()
        .join(", ");
    (!list.is_empty()).then_some(list)
}

/// Installed native mods not compiled into this executable get an explanatory row.
fn not_built(
    dir: &std::path::Path,
    built: &[horizon_mods::runtime::BuiltMod],
    rows: &[Installed],
) -> Vec<Installed> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|d| d.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    paths.retain(|p| p.extension().is_some_and(|e| e == "hmod"));
    paths.sort();
    let mut out: Vec<Installed> = Vec::new();
    for path in paths {
        let Ok(bundle) = horizon_mods::index::native::Bundle::read(&path) else {
            continue;
        };
        let m = bundle.manifest;
        let listed = rows.iter().chain(&out).any(|row| row.id == m.id);
        if !listed && !built.iter().any(|b| b.id == m.id) {
            out.push(Installed {
                title: format!("{} {}", m.id, m.version),
                id: m.id,
                description: String::new(),
                problem: Some(mod_list::NOT_BUILT.to_owned()),
                dev: false,
                native: None,
            });
        }
    }
    out
}

impl Game<'_> {
    /// Build this game's loop context over the loaded registries and the mods'
    /// current settings. Called where a resource generation starts or reloads: a
    /// client at window start and after a resource reload, a server when it starts
    /// its mods and on `/reload`. A client hands a second one to its audio.
    pub(super) fn refresh_loop_context(&mut self) {
        let side = self.options.execution_role.loop_side();
        let item_data = self.res.core.item_data.context();
        self.loop_context = std::sync::Arc::new(
            item_data.loop_context(side, mod_api::__private::loaded_mods().ids()),
        );
        if let Some(host) = self.lan.as_mut() {
            host.hub.loop_context = self.loop_context.clone();
        }
        if self.options.execution_role.is_client() {
            self.audio.set_loop_context(
                item_data.loop_context(side, mod_api::__private::loaded_mods().ids()),
            );
        }
    }

    /// Lists the installed mods on the Mods page. Content mods load with the resources
    /// (`content_root`); native ones are built into the game (`native_hooks.rs`). Unsupported
    /// files get the same unreadable-file row as any other invalid package.
    pub(super) fn start_mods(&mut self) {
        let dir = Self::game_dir().join(mod_list::DIR);
        let mut paths = horizon_mods::find(&dir);
        paths.retain(|p| !self.mods.dev.contains(p));
        paths.extend(self.mods.dev.iter().cloned());
        self.settings.mods.installed = paths
            .iter()
            .map(|path| {
                let file = path.file_name().unwrap_or_default().to_string_lossy();
                let package = match horizon_mods::Package::read(path) {
                    Ok(package) => package,
                    Err(e) => {
                        return Installed {
                            id: String::new(),
                            title: file.into_owned(),
                            description: String::new(),
                            problem: Some(format!("Can't be read: {e}")),
                            dev: false,
                            native: None,
                        };
                    }
                };
                let m = &package.manifest;
                let refused = horizon_mod_wasm::refused()
                    .iter()
                    .find(|(id, _)| *id == m.id);
                Installed {
                    id: m.id.clone(),
                    title: format!("{} {}", m.name, m.version),
                    description: m.description.clone(),
                    problem: refused.map(|(_, why)| format!("Not loaded: {why}")),
                    dev: self.mods.dev.contains(path),
                    native: None,
                }
            })
            .collect();
        // A mod with a native version built in says so instead.
        self.list_native_mods();
        let built = self.mods.native.built();
        let built_mods = native_mods_detail(built);
        if let Some(list) = built_mods {
            mod_api::crash::set_detail("Native Mods", list);
        }

        let native_dir = Self::game_dir().join(mod_list::NATIVE_DIR);
        let unbuilt = not_built(&native_dir, built, &self.settings.mods.installed);
        self.settings.mods.installed.extend(unbuilt);
        self.mods.report_refusals(&mut self.settings.mods.installed);
    }
}

/// Mob types loaded from a scratch root whose `entity-types.json` has a
/// zombie, a villager, and the rows a content root writes for a mod's types
/// (`mod_content`): `ruby:ghoul` (a zombie with 40 health), `ruby:settler` (a
/// villager as it is) and `ruby:bolt` (a thrown snowball, not a mob).
#[cfg(test)]
pub(super) fn living_mod_types()
-> std::collections::HashMap<String, std::sync::Arc<horizon_client::mobs::MobType>> {
    let zombie =
        r#""width":0.6,"height":1.95,"eye_height":1.74,"category":"monster","fire_immune":false"#;
    let villager =
        r#""width":0.6,"height":1.95,"eye_height":1.62,"category":"misc","fire_immune":false"#;
    let table = format!(
        r#"{{"minecraft:zombie":{{{zombie},"attributes":{{"minecraft:max_health":20.0,"minecraft:movement_speed":0.23000000417232513}}}},
        "minecraft:villager":{{{villager},"attributes":{{"minecraft:max_health":20.0,"minecraft:movement_speed":0.5}}}},
        "minecraft:snowball":{{"width":0.25,"height":0.25,"eye_height":0.0,"category":"misc","fire_immune":false}},
        "ruby:ghoul":{{{zombie},"attributes":{{"minecraft:max_health":40.0,"minecraft:movement_speed":0.23000000417232513}},"template":"minecraft:zombie"}},
        "ruby:settler":{{{villager},"attributes":{{"minecraft:max_health":20.0,"minecraft:movement_speed":0.5}},"template":"minecraft:villager"}},
        "ruby:bolt":{{"width":0.25,"height":0.25,"eye_height":0.0,"category":"misc","fire_immune":false,"template":"minecraft:snowball"}}}}"#
    );
    let root =
        std::env::temp_dir().join(format!("horizon-living-mod-types-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("entity-types.json"), table).unwrap();
    let types = horizon_client::mobs::MobTypes::load_for_server(&root).unwrap();
    let _ = std::fs::remove_dir_all(&root);
    types
        .types
        .into_iter()
        .map(|(id, t)| (id, std::sync::Arc::new(t)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_mods::runtime::{BuiltMod, Runtime};

    const BUILT: &[BuiltMod] = &[BuiltMod {
        id: "machine_sample",
        version: "0.3.0",
        side: horizon_mods::Side::Both,
    }];

    struct Fixture(PathBuf);
    impl Fixture {
        fn new(label: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("horizon-game-mods-{}-{label}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
        fn runtime(&self) -> Runtime {
            let dir = self.0.join("compiled");
            let package = dir.join(mod_list::NATIVE_DIR).join("machine_sample");
            let file = std::fs::File::open(self.0.join("machine_sample.hmod")).unwrap();
            zip::ZipArchive::new(file)
                .unwrap()
                .extract(&package)
                .unwrap();
            Runtime::load(&dir.join("horizon-engine"), BUILT).unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }

    /// A content mod's living type is a mob of its template's species under
    /// its own id, with its own attributes: it saves its id with every field
    /// its template's class saves, loads back as itself, and rolls its own
    /// loot table. A thrown type stays a projectile.
    #[test]
    fn a_mod_living_type_is_its_templates_mob_with_its_own_attributes() {
        use horizon_client::mobs::{Mob, Species, drops::death_loot_table};
        use horizon_core::geometry::Vec3;
        let types = living_mod_types();
        let (zombie, ghoul) = (&types["minecraft:zombie"], &types["ruby:ghoul"]);
        assert_eq!(ghoul.species, Species::Zombie);
        assert_eq!(ghoul.class_id(), "minecraft:zombie");
        assert_eq!(types["ruby:settler"].species, Species::Villager);
        assert!(!types.contains_key("ruby:bolt"), "a thrown type is no mob");
        assert_eq!((zombie.max_health, ghoul.max_health), (20.0, 40.0));
        assert_eq!(ghoul.speed.to_bits(), zombie.speed.to_bits());
        assert_eq!(
            (ghoul.sounds, ghoul.texture.as_str()),
            (zombie.sounds, zombie.texture.as_str())
        );

        let at = Vec3::new(0.5, 64.0, 0.5);
        let (zombie, ghoul) = (
            Mob::new(1, zombie.clone(), at, 0.0),
            Mob::new(2, ghoul.clone(), at, 0.0),
        );
        assert_eq!(ghoul.health, 40.0);
        let saved = |m: &Mob| horizon_client::entity_nbt::mob_to_nbt(m, [0, 0, 0, 2], None, None);
        let keys = |tag: &horizon_world::nbt::Tag| -> Vec<String> {
            let horizon_world::nbt::Tag::Compound(map) = tag else {
                panic!("a mob saves a compound");
            };
            map.keys().map(|k| k.to_string_lossy()).collect()
        };
        let (zombie_tag, ghoul_tag) = (saved(&zombie), saved(&ghoul));
        let Some(horizon_world::nbt::Tag::String(id)) = ghoul_tag.get("id") else {
            panic!("a saved mob has its id");
        };
        assert_eq!(id.to_string_lossy(), "ruby:ghoul");
        assert_eq!(
            keys(&ghoul_tag),
            keys(&zombie_tag),
            "it saves what a zombie saves"
        );
        for key in ["IsBaby", "CanBreakDoors", "DrownedConversionTime"] {
            assert!(keys(&ghoul_tag).iter().any(|k| k == key), "{key}");
        }
        let loaded = horizon_client::entity_nbt::mob_from_nbt(&ghoul_tag, &types, 3, None)
            .expect("it loads back");
        assert_eq!(
            (loaded.kind.id.as_str(), loaded.health),
            ("ruby:ghoul", 40.0)
        );
        assert_eq!(
            death_loot_table(&ghoul, true).as_deref(),
            Some("ruby:entities/ghoul")
        );
        assert_eq!(
            death_loot_table(&zombie, true).as_deref(),
            Some("minecraft:entities/zombie")
        );
    }

    #[test]
    fn standalone_models_collect_only_on_the_client() {
        use mod_api::{self, Hooks, ModEntry, StandaloneModelsEvent, testing::with_mods};
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        #[derive(Default)]
        struct Models(Arc<AtomicUsize>);
        #[mod_api::horizon_mod]
        impl Hooks for Models {
            fn standalone_models(
                &self,
                context: &RegistrationContext,
                event: &mut StandaloneModelsEvent<'_>,
            ) {
                assert_eq!(
                    context.mods().ids().collect::<Vec<_>>(),
                    ["registration_test"]
                );
                self.0.fetch_add(1, Ordering::Relaxed);
                event.models.push("registration_test:block/shaft".into());
            }
        }
        let calls = Arc::new(AtomicUsize::new(0));
        with_mods(
            vec![ModEntry::new(
                "registration_test",
                Box::new(Models(calls.clone())),
            )]
            .into(),
            || {
                mod_api::__private::with_test_packages(&["registration_test"], || {
                    assert_eq!(
                        collect_standalone_models(ExecutionRole::IntegratedServer),
                        None
                    );
                    assert_eq!(
                        collect_standalone_models(ExecutionRole::DedicatedServer),
                        None
                    );
                    assert_eq!(calls.load(Ordering::Relaxed), 0);
                    let models = collect_standalone_models(ExecutionRole::Client).unwrap();
                    assert_eq!(calls.load(Ordering::Relaxed), 1);
                    assert_eq!(models, ["registration_test:block/shaft"]);
                })
            },
        );
    }

    fn hmod(dir: &std::path::Path, id: &str) {
        let manifest = format!(
            "id = '{id}'\nname = '{id}'\nversion = '0.3.0'\napi = '0.2'\nside = 'both'\ncapabilities = ['registries']\n[native]\ngame = '26.3'\nhorizon = '{}'\nmixins = '{}'\n",
            "a".repeat(40),
            "b".repeat(40)
        );
        let files: &[(&str, &[u8])] = &[
            (
                "Cargo.toml",
                b"[package]\nname='different_crate'\nversion='9.9.9'\n",
            ),
            ("src/lib.rs", b"pub fn hooks() {}"),
        ];
        let bytes = horizon_mods::Package::write_files(&manifest, files).unwrap();
        std::fs::write(dir.join(format!("{id}.hmod")), bytes).unwrap();
    }

    #[test]
    fn native_content_follows_the_build_and_misplaced_content_follows_its_selection() {
        const MODE: &str = "HORIZON_MODS_SELECTION_TEST";
        let mode = std::env::var(MODE).ok();
        if mode.is_none() {
            // Inventory and content registries are process-wide, as in a real startup.
            for mode in ["compiled", "vanilla"] {
                let status = std::process::Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", "game::mods::tests::native_content_follows_the_build_and_misplaced_content_follows_its_selection", "--nocapture"])
                    .env(MODE, mode).status().unwrap();
                assert!(status.success());
            }
            return;
        }
        let f = Fixture::new(mode.as_deref().unwrap());
        hmod(&f.0, "machine_sample");
        let native = if mode.as_deref() == Some("compiled") {
            f.runtime()
        } else {
            Runtime::default()
        };
        let here = f.0.join("unrelated-game");
        let misplaced = here.join(mod_list::NATIVE_DIR);
        std::fs::create_dir_all(&misplaced).unwrap();
        hmod(&misplaced, "machine_sample");
        let manifest = "id='plain'\nname='Plain'\nversion='1.0.0'\napi='0.2'\nside='both'\ncapabilities=['registries']\n";
        let plain = horizon_mods::Package::write_files(manifest, &[]).unwrap();
        std::fs::write(misplaced.join("plain.hmod"), plain).unwrap();
        let base = f.0.join("resources");
        std::fs::create_dir_all(&base).unwrap();
        // A prepared root's registry report: block entity registration reads it (B1).
        std::fs::write(
            base.join("registries.json"),
            "{\"minecraft:block_entity_type\":{\"entries\":{}}}",
        )
        .unwrap();
        std::env::set_current_dir(&here).unwrap();
        let mut sources = ModsState {
            native: std::sync::Arc::new(native),
            ..Default::default()
        };
        let enabled = std::collections::BTreeSet::from(["plain".to_owned()]);
        sources.select(&here, &enabled);
        assert_eq!(content_root(&base, &sources).unwrap(), base);
        let local = horizon_protocol::mod_list::local();
        let ids: Vec<_> = local.iter().map(|m| m.id.as_str()).collect();
        if mode.as_deref() == Some("compiled") {
            assert_eq!(ids, ["machine_sample", "plain"]);
            assert_eq!(local[0].version, "0.3.0");
        } else {
            assert_eq!(ids, ["plain"]);
        }
        std::env::set_current_dir(&f.0).unwrap();
    }

    #[test]
    fn a_shared_missing_jar_stops_native_load_and_refuses_each_content_mod_row() {
        const MODE: &str = "HORIZON_JAR_REFUSAL_TEST";
        if std::env::var_os(MODE).is_none() {
            // Native declarations, like the real startup, belong to one process.
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "game::mods::tests::a_shared_missing_jar_stops_native_load_and_refuses_each_content_mod_row", "--nocapture"])
                .env(MODE, "1").status().unwrap();
            assert!(status.success());
            return;
        }
        let f = Fixture::new("shared-jar-refusal");
        let jar =
            horizon_mods::manifest::JarRef::new("original".into(), "1".into(), "9".repeat(128))
                .unwrap();
        for id in ["machine_sample", "alpha", "beta", "gamma"] {
            let native = id == "machine_sample";
            let mut text = format!(
                "id='{id}'\nname='{id}'\nversion='0.3.0'\napi='0.2'\nside='both'\ncapabilities=['registries']\n"
            );
            if native {
                text.push_str(&format!(
                    "[native]\ngame='26.3'\nhorizon='{}'\nmixins='{}'\n",
                    "a".repeat(40),
                    "b".repeat(40)
                ));
            }
            if id != "gamma" {
                text.push_str(&format!(
                    "[[jar]]\nid='original'\nversion='1'\nsha512='{}'\n",
                    jar.sha512()
                ));
            }
            let code: &[(&str, &[u8])] = if native {
                &[
                    ("Cargo.toml", b"[package]\nname='example'\nversion='1'\n"),
                    ("src/lib.rs", b"pub fn hooks() {}"),
                ]
            } else {
                &[]
            };
            std::fs::write(
                f.0.join(format!("{id}.hmod")),
                horizon_mods::Package::write_files(&text, code).unwrap(),
            )
            .unwrap();
        }
        let selected: Arc<[horizon_mods::selection::SelectedPackage]> = ["alpha", "beta", "gamma"]
            .into_iter()
            .map(|id| {
                let path = f.0.join(format!("{id}.hmod"));
                horizon_mods::selection::SelectedPackage {
                    package: horizon_mods::Package::read(&path).unwrap(),
                    path,
                }
            })
            .collect();
        let native = ModsState {
            native: Arc::new(f.runtime()),
            selected: selected.clone(),
            ..Default::default()
        };
        let base = f.0.join("vanilla");
        let store = f.0.join("jars");
        std::fs::create_dir_all(&base).unwrap();
        let error = content_overlay(&base, &native, &store).err().unwrap();
        let missing = error.downcast_ref::<MissingJar>().unwrap();
        assert_eq!(missing.owners, ["machine_sample", "alpha", "beta"]);
        assert_eq!(missing.jar, jar);
        assert_eq!(missing.path, jar.cache_path(&store));
        assert!(!store.exists());

        let content = ModsState {
            selected,
            ..Default::default()
        };
        let overlay = content_overlay(&base, &content, &store).unwrap();
        assert_eq!(overlay.root, base);
        assert_eq!(
            overlay
                .mods
                .iter()
                .map(|m| m.id.as_str())
                .collect::<Vec<_>>(),
            ["gamma"]
        );
        let mut rows: Vec<_> = ["alpha", "beta", "gamma"]
            .into_iter()
            .map(|id| Installed {
                id: id.into(),
                title: id.into(),
                description: String::new(),
                problem: None,
                dev: false,
                native: None,
            })
            .collect();
        content.report_refusals(&mut rows);
        for row in &rows[..2] {
            let problem = row.problem.as_deref().unwrap();
            assert!(problem.contains("alpha, beta: original 1:"));
            assert!(problem.contains(&jar.cache_path(&store).display().to_string()));
        }
        assert!(rows[2].problem.is_none());
    }

    #[test]
    fn the_crash_report_lists_the_embedded_native_mods() {
        let built = [
            BUILT[0],
            BuiltMod {
                id: "pipes",
                version: "2.0",
                side: horizon_mods::Side::Client,
            },
        ];
        assert_eq!(
            native_mods_detail(&built).as_deref(),
            Some("machine_sample 0.3.0, pipes 2.0")
        );
        assert_eq!(native_mods_detail(&[]), None);
    }

    #[test]
    fn native_mods_installed_but_not_built_get_a_row_saying_so() {
        let f = Fixture::new("not-built");
        for id in ["veinminer", "zoom", "minimap", "machine_sample"] {
            hmod(&f.0, id);
        }
        std::fs::write(f.0.join("notes.txt"), "not a mod").unwrap();
        std::fs::write(f.0.join("broken.hmod"), "not a zip").unwrap();
        let rows = not_built(&f.0, &[], &[]);
        let ids: Vec<_> = rows.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, ["machine_sample", "minimap", "veinminer", "zoom"]);
        assert_eq!(rows[2].title, "veinminer 0.3.0");
        assert_eq!(rows[2].problem.as_deref(), Some(mod_list::NOT_BUILT));
        assert!(rows.iter().all(|r| r.native.is_none() && !r.dev));
        let listed = Installed {
            id: "minimap".into(),
            ..rows[0].clone()
        };
        let rows = not_built(&f.0, BUILT, &[listed]);
        let ids: Vec<_> = rows.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, ["veinminer", "zoom"]);
        assert!(not_built(&f.0.join("missing"), &[], &[]).is_empty());
    }

    /// A root with mods that declare no mob effects leaves the registry open for the first one
    /// that does.
    #[test]
    fn a_root_without_mob_effects_does_not_freeze_the_registry() {
        use horizon_client::{brewing, mod_content::ContentMod};
        // Another test may have set the process-wide list already.
        if !brewing::mod_effects().is_empty() {
            return;
        }
        let f = Fixture::new("no-effects");
        std::fs::write(
            f.0.join("horizon-mod.toml"),
            "id = 'plain'\nname = 'plain'\nversion = '1.0'\napi = '0.2'\ncapabilities = ['registries']\n",
        )
        .unwrap();
        let package = horizon_mods::Package::read(&f.0).unwrap();
        let plain = ContentMod {
            id: "plain".into(),
            content: horizon_mods::content::Content::read(&f.0, &package.manifest).unwrap(),
            jars: Vec::new(),
        };
        register_mob_effects(&[plain]);
        // The list `potions::tests` registers, so it holds whichever test runs first.
        assert!(brewing::register_mod_effects([(
            "test:effect".to_owned(),
            0,
            brewing::Category::Neutral
        )]));
    }
}
