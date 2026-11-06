//! Data packs in the game: functions (`/function`, `/return`, tick and load
//! tags, `/schedule`), predicates, `/reload` and `/datapack`.
use super::{
    Game,
    commands::{
        R, Target, err,
        execute::{ExecState, Source},
    },
};
use horizon_client::{
    command::{Arg, Context},
    datapack::{self, DataPack, DataStore, Function},
    save,
    scoreboard::bracketed,
    text::{self, Text},
};
use horizon_world::nbt::Tag;
use mod_api::__private::reload::ReloadStageError;
use std::sync::Arc;

/// A `TimerQueue` event.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Scheduled {
    /// `ns:function`, or `#ns:tag` for a function tag.
    pub id: String,
    pub time: i64,
    pub seq: u64,
}
impl FnState {
    /// The file `path` of the highest enabled pack that has it.
    pub(super) fn pack_file(&self, path: &str) -> Option<Vec<u8>> {
        self.enabled
            .iter()
            .rev()
            .filter_map(|id| self.packs.iter().find(|p| &p.id == id))
            .find_map(|pack| pack.read(path))
    }
}
#[derive(Default)]
pub(super) struct FnState {
    pub store: DataStore,
    packs: Vec<Arc<DataPack>>,
    /// Enabled pack ids, in load order (later overrides earlier).
    enabled: Vec<String>,
    loaded: bool,
    /// Run the `minecraft:load` tag on the next tick.
    load_pending: bool,
    pub schedule: Vec<Scheduled>,
    next_seq: u64,
    /// A dedicated server's `function-permission-level`.
    pub permission: Option<u8>,
}
/// What a function reported through `return`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Outcome {
    NoReturn,
    Value(i32),
    Fail,
}

/// The world's pack stack: the built-in pack (unless disabled), the packs
/// of `<world>/datapacks` and the resources root's feature packs, with the
/// enabled ids from `datapacks.json` (`PackRepository.reload`).
pub(super) struct PackStack {
    pub builtin: Option<DataPack>,
    pub packs: Vec<Arc<DataPack>>,
    pub enabled: Vec<String>,
}
impl PackStack {
    /// The enabled packs in `enabled` (the selected) order, later overriding
    /// earlier: `PackRepository.openAllSelected` keeps the selected order.
    pub(super) fn enabled_packs(&self) -> impl Iterator<Item = &DataPack> {
        self.enabled
            .iter()
            .filter_map(|id| self.packs.iter().find(|p| &p.id == id).map(Arc::as_ref))
    }
}
pub(super) fn pack_stack(
    world_root: &std::path::Path,
    resources: &std::path::Path,
    vanilla_disabled: bool,
    enabled_features: &[String],
) -> PackStack {
    let builtin = (!vanilla_disabled).then(|| DataPack::builtin(resources));
    let packs = datapack::discover_with_features(world_root, resources);
    let enabled = datapack::enabled_ids(world_root, &packs, enabled_features);
    PackStack {
        builtin,
        packs: packs.into_iter().map(Arc::new).collect(),
        enabled,
    }
}

/// The world's enabled packs' content files (`recipe/`, `loot_table/`,
/// `advancement/`, `tags/…`) for the tables to merge over the resource
/// root's own. Empty when the world has no packs.
pub(super) fn content_overlay(stack: &PackStack) -> Vec<(String, Vec<Vec<u8>>)> {
    let packs: Vec<&DataPack> = stack.enabled_packs().collect();
    if packs.is_empty() {
        return Vec::new();
    }
    DataStore::load(&packs).files.into_iter().collect()
}

/// The files the content tables load from: the world's enabled packs'
/// (`content_overlay`) with what native mods contribute for this load
/// (`native_data_pack_files`) below them. `PackOverlay::top` is a path's last
/// file, so a pack's file still overrides a mod's for the same path, and a
/// later contribution overrides an earlier one, as packs do. `Game` builds
/// its tables through here on a world's first open and on `/reload`, so the
/// mods are asked once per load.
pub(super) fn overlay_files(
    stack: &PackStack,
    contributed: Vec<(String, Vec<u8>)>,
) -> Vec<(String, Vec<Vec<u8>>)> {
    let mut files = content_overlay(stack);
    if contributed.is_empty() {
        return files;
    }
    // A path's mod files in contribution order, under the pack files.
    let mut mods: std::collections::BTreeMap<String, Vec<Vec<u8>>> = Default::default();
    for (path, bytes) in contributed {
        // One bad file would fail its whole table and drop the world's pack
        // content with it, so it is skipped alone: every kind is JSON.
        if let Err(e) = serde_json::from_slice::<Box<serde_json::value::RawValue>>(&bytes) {
            log::warn!("Horizon: a native mod's data pack file {path} is not JSON, skipped: {e}");
            continue;
        }
        mods.entry(path).or_default().push(bytes);
    }
    for (path, versions) in files.iter_mut() {
        let Some(under) = mods.remove(path.as_str()) else {
            continue;
        };
        let mut merged = under;
        merged.append(versions);
        *versions = merged;
    }
    // The paths only a mod has.
    files.extend(mods);
    files
}

impl FnState {
    /// Enabled pack ids, in load order (later overrides earlier).
    #[cfg(test)]
    pub(super) fn enabled(&self) -> &[String] {
        &self.enabled
    }
    /// The level function files compile at (`ServerFunctionLibrary`'s
    /// `functionCompilationPermissions`): `function-permission-level` on a
    /// dedicated server, game master (2) otherwise. Whoever runs a function,
    /// its commands need no more than this.
    fn command_level(&self) -> u8 {
        self.permission.unwrap_or(2)
    }
}

impl Game<'_> {
    // ---- loading -----------------------------------------------------------

    fn datapack_settings_path(&self) -> std::path::PathBuf {
        self.world_root.join("datapacks.json")
    }
    /// Discover and load the data packs of the world (once, or on `/reload`).
    pub(super) fn ensure_datapacks(&mut self) {
        if self.fns.loaded || self.world_root.as_os_str().is_empty() {
            return;
        }
        let _ = self.reload_datapacks(false, &self.level.enabled_features.clone());
    }
    pub(super) fn reload_datapacks(
        &mut self,
        run_load: bool,
        enabled_features: &[String],
    ) -> Result<PackStack, ReloadStageError> {
        let stack = pack_stack(
            &self.world_root,
            &self.options.resources,
            self.cmds.vanilla_disabled,
            enabled_features,
        );
        let prepared = self.prepare_server_reload(&stack)?;
        if !run_load {
            self.fns = Default::default();
        }
        self.fns.packs = stack.packs.clone();
        self.fns.enabled = stack.enabled.clone();
        self.apply_server_reload(prepared, &stack);
        self.fns.loaded = true;
        self.fns.load_pending = true;
        // An explicit reload (`/reload`, `/datapack`) also rebuilds the
        // content tables, as `ReloadableServerResources` does; on world open
        // `finish_open_world` builds them right after this.
        if run_load {
            self.reload_world_tables(&stack);
        }
        if self.fns.schedule.is_empty() {
            self.load_schedule();
        }
        Ok(stack)
    }
    fn save_datapack_settings(&self) {
        let disabled: Vec<&String> = self
            .fns
            .packs
            .iter()
            .filter(|p| !self.fns.enabled.contains(&p.id))
            .map(|p| &p.id)
            .collect();
        let json = serde_json::json!({"enabled": self.fns.enabled, "disabled": disabled});
        let _ = std::fs::write(self.datapack_settings_path(), json.to_string());
    }
    pub(super) fn sync_function_registries(&mut self, stack: &PackStack) {
        self.sync_schedule_registry();
        let reg = &mut self.ui.registries;
        reg.functions = self.fns.store.functions.keys().cloned().collect();
        reg.function_tags = self.fns.store.function_tags.keys().cloned().collect();
        reg.predicates = self.fns.store.predicates.keys().cloned().collect();
        reg.item_modifiers = self.fns.store.item_modifiers.keys().cloned().collect();
        reg.datapacks_enabled = std::iter::once("vanilla".to_owned())
            .chain(self.fns.enabled.iter().cloned())
            .collect();
        reg.datapacks_disabled = self
            .fns
            .packs
            .iter()
            .filter(|p| !self.fns.enabled.contains(&p.id))
            .map(|p| p.id.clone())
            .collect();
        // A pack's worldgen ids join the completion registries
        // (`command-registries.json` lists only the game's own), and so do
        // its worldgen tag ids for the `#tag` completions. The packs are the
        // ones the world's worldgen is built from.
        let enabled: Vec<_> = stack.enabled_packs().collect();
        let (overrides, _) = horizon_client::datapack::worldgen_overrides(&enabled, |_| true);
        for (bundle, registry) in [
            ("biomes", "minecraft:worldgen/biome"),
            ("structures", "minecraft:worldgen/structure"),
            ("structure_sets", "minecraft:worldgen/structure_set"),
            ("template_pools", "minecraft:worldgen/template_pool"),
            ("processor_lists", "minecraft:worldgen/processor_list"),
        ] {
            let entry = reg.by_registry.entry(registry.to_owned()).or_default();
            for (id, _) in overrides.entries(bundle) {
                if !entry.ids.contains(&id.to_owned()) {
                    entry.ids.push(id.to_owned());
                }
            }
            entry.ids.sort();
        }
        let entry = reg
            .by_registry
            .entry("minecraft:worldgen/biome".to_owned())
            .or_default();
        for (id, _, _) in overrides.tags("biome_tags") {
            if !entry.tags.contains(&id.to_owned()) {
                entry.tags.push(id.to_owned());
            }
        }
        entry.tags.sort();
        let entry = reg
            .by_registry
            .entry("minecraft:worldgen/structure".to_owned())
            .or_default();
        for (id, _, _) in overrides.tags("structure_tags") {
            if !entry.tags.contains(&id.to_owned()) {
                entry.tags.push(id.to_owned());
            }
        }
        entry.tags.sort();
    }

    fn sync_schedule_registry(&mut self) {
        self.ui.registries.schedules = self.fns.schedule.iter().map(|s| s.id.clone()).collect();
    }

    // ---- scheduled events ------------------------------------------------------

    fn schedule_path(&self) -> std::path::PathBuf {
        self.world_root.join("data/minecraft/scheduled_events.dat")
    }
    fn load_schedule(&mut self) {
        let Some(root) = save::read_dat(&self.schedule_path()) else {
            return;
        };
        let Some(Tag::List(events)) = root.get("data").and_then(|d| d.get("events")) else {
            return;
        };
        for e in events {
            let (Some(Tag::Long(time)), Some(Tag::String(id)), Some(cb)) =
                (e.get("trigger_time"), e.get("id"), e.get("callback"))
            else {
                continue;
            };
            let _ = cb;
            self.fns.next_seq += 1;
            self.fns.schedule.push(Scheduled {
                id: id.to_string_lossy(),
                time: *time,
                seq: self.fns.next_seq,
            });
        }
    }
    pub(super) fn save_schedule(&self) {
        if !self.fns.loaded || self.remote.is_some() {
            return;
        }
        let mut events = self.fns.schedule.clone();
        events.sort_by_key(|e| (e.time, e.seq));
        let list = events
            .iter()
            .map(|e| {
                let (kind, name) = match e.id.strip_prefix('#') {
                    Some(tag) => ("minecraft:function_tag", tag),
                    None => ("minecraft:function", e.id.as_str()),
                };
                Tag::compound([
                    ("trigger_time", Tag::Long(e.time)),
                    ("id", Tag::String(e.id.as_str().into())),
                    (
                        "callback",
                        Tag::compound([
                            ("type", Tag::String(kind.into())),
                            ("id", Tag::String(name.into())),
                        ]),
                    ),
                ])
            })
            .collect();
        if events.is_empty() && !self.schedule_path().exists() {
            return;
        }
        let _ = save::write_dat(
            &self.schedule_path(),
            Tag::compound([
                ("data", Tag::compound([("events", Tag::List(list))])),
                ("DataVersion", Tag::Int(horizon_world::DATA_VERSION)),
            ]),
        );
    }

    // ---- ticking -----------------------------------------------------------------

    /// `ServerFunctionManager.tick` and the scheduled events.
    pub(super) fn tick_functions(&mut self) {
        if self.remote.is_some() {
            return;
        }
        self.exec = ExecState::default();
        let source = self.server_source();
        if self.fns.load_pending {
            self.fns.load_pending = false;
            for f in self.fns.store.tag_functions("minecraft:load") {
                self.run_function_by_id(&f, None, source);
            }
        }
        let now = self.world.game_time;
        loop {
            let due = self
                .fns
                .schedule
                .iter()
                .enumerate()
                .filter(|(_, e)| e.time <= now)
                .min_by_key(|(_, e)| (e.time, e.seq))
                .map(|(i, _)| i);
            let Some(i) = due else { break };
            let event = self.fns.schedule.remove(i);
            match event.id.strip_prefix('#') {
                Some(tag) => {
                    for f in self.fns.store.tag_functions(tag) {
                        self.run_function_by_id(&f, None, source);
                    }
                }
                None => self.run_function_by_id(&event.id, None, source),
            }
        }
        for f in self.fns.store.tag_functions("minecraft:tick") {
            self.run_function_by_id(&f, None, source);
        }
    }
    fn run_function_by_id(&mut self, id: &str, args: Option<&Tag>, source: Source) {
        self.exec.commands = 0;
        if let Some(f) = self.fns.store.functions.get(id).cloned() {
            // `ServerFunctionManager.execute` logs a function that failed to
            // instantiate or run; its lines' own failures stay silent.
            if let Err(e) = self.run_function(&f, args, source) {
                log::warn!(
                    "Horizon: failed to run function {id}: {}",
                    e.message.plain()
                );
            }
        }
    }

    /// The function branch of advancement rewards: resolve the optional function
    /// and execute it as the rewarded player, with suppressed output and GAMEMASTER permission.
    pub(super) fn run_advancement_reward_function(&mut self, id: &str) {
        self.ensure_datapacks();
        let Some(function) = self.fns.store.functions.get(id).cloned() else {
            return;
        };
        let Some(pos) = self.target_position(Target::Player) else {
            return;
        };
        // createCommandSourceStack belongs to the player, not an enclosing
        // execute's position, rotation, output sink or store callback.
        let source = Source {
            pos,
            rot: self.target_rotation(Target::Player),
            silent: true,
            ..self.player_source()
        };
        // The existing runner applies this level to nested function calls too.
        let permission = self
            .fns
            .permission
            .replace(horizon_client::command::vanilla::GAMEMASTERS);
        let callback = self.exec.callback.take();
        let result = self.run_function(&function, None, source);
        self.exec.callback = callback;
        self.fns.permission = permission;
        if let Err(error) = result {
            log::warn!(
                "Horizon: failed to run advancement reward function {id}: {}",
                error.message.plain()
            );
        }
    }

    // ---- running -----------------------------------------------------------------

    /// Run one function with `source`: its commands in order, until a `return`.
    pub(in crate::game) fn run_function(
        &mut self,
        f: &Function,
        args: Option<&Tag>,
        source: Source,
    ) -> R<Outcome> {
        let lines = f.instantiate(args).map_err(|(key, a)| {
            let args: Vec<&dyn std::fmt::Display> =
                a.iter().map(|s| s as &dyn std::fmt::Display).collect();
            err(
                "commands.function.instantiationFailure",
                &[&f.id, &horizon_client::text::Text::tr(&key, &args).plain()],
            )
        })?;
        if self.exec.function_depth >= 128 {
            return Ok(Outcome::NoReturn);
        }
        self.exec.function_depth += 1;
        let outer = self.exec.returned.take();
        let level = std::mem::replace(&mut self.ui.registries.permission, self.fns.command_level());
        let inner = Source {
            silent: true,
            store: None,
            ..source
        };
        for line in lines {
            let _ = self.execute_line(&line, inner);
            if self.exec.returned.is_some() {
                break;
            }
            if self.exec.commands
                > self
                    .ui
                    .rules
                    .int_at(horizon_client::gamerules::vanilla::MAX_COMMAND_SEQUENCE_LENGTH)
                    .max(1) as u32
            {
                break;
            }
        }
        self.ui.registries.permission = level;
        let returned = std::mem::replace(&mut self.exec.returned, outer);
        self.exec.function_depth -= 1;
        Ok(match returned {
            None => Outcome::NoReturn,
            Some(Some(v)) => Outcome::Value(v),
            Some(None) => Outcome::Fail,
        })
    }
    /// The functions a `FunctionArgument` names: a function or a tag.
    pub(in crate::game) fn functions_named(&mut self, name: &str) -> R<Vec<Function>> {
        match name.strip_prefix('#') {
            // ServerFunctionLibrary.getTag: an unknown tag is empty.
            Some(tag) => {
                if !self.fns.store.function_tags.contains_key(tag) {
                    return Ok(Vec::new());
                }
                Ok(self
                    .fns
                    .store
                    .tag_functions(tag)
                    .iter()
                    .filter_map(|id| self.fns.store.functions.get(id).cloned())
                    .collect())
            }
            None => self
                .fns
                .store
                .functions
                .get(name)
                .cloned()
                .map(|f| vec![f])
                .ok_or_else(|| err("arguments.function.unknown", &[&name])),
        }
    }
    /// `if function`: what each function returns (None if it did not).
    pub(in crate::game) fn call_function_ref(
        &mut self,
        name: &str,
        args: Option<&Tag>,
    ) -> R<Vec<Option<i32>>> {
        let functions = self.functions_named(name)?;
        let source = self.command_source.unwrap_or_else(|| self.player_source());
        let mut out = Vec::new();
        for f in functions {
            out.push(match self.run_function(&f, args, source)? {
                Outcome::NoReturn => None,
                Outcome::Value(v) => Some(v),
                Outcome::Fail => Some(0),
            });
        }
        Ok(out)
    }

    // ---- commands ------------------------------------------------------------------

    /// `FunctionCommand.decorateOutputIfNeeded`: report returned values even for failure.
    pub(super) fn cmd_function(&mut self, ctx: &Context) -> R<i32> {
        let Some(Arg::Resource(name)) = ctx.get("name").cloned() else {
            return Ok(0);
        };
        let functions = self.functions_named(&name)?;
        if functions.is_empty() {
            let id = name.trim_start_matches('#');
            return Err(err("commands.function.scheduled.no_functions", &[&id]));
        }
        let args = if ctx.has("with") {
            let r = self.data_ref_named(ctx, 0, "source")?;
            let data = self.data_of(&r)?;
            let tag = match ctx.get("path") {
                Some(Arg::Str(p)) => {
                    let path = horizon_client::nbt_path::NbtPath::parse(p)
                        .map_err(horizon_client::data_cmd::path_error)?;
                    horizon_client::data_cmd::single_tag(&path, &data)?
                }
                _ => data,
            };
            if !matches!(tag, Tag::Compound(_)) {
                return Err(err(
                    "commands.function.error.argument_not_compound",
                    &[&"tag"],
                ));
            }
            Some(tag)
        } else if let Some(Arg::Str(text)) = ctx.get("arguments") {
            Some(
                horizon_world::nbt::parse_snbt(text)
                    .map_err(|e| err("argument.nbt.invalid", &[&e]))?
                    .0,
            )
        } else {
            None
        };
        let message = if functions.len() == 1 {
            Text::translate(
                "commands.function.scheduled.single",
                &[Text::literal(functions[0].id.clone())],
            )
        } else {
            Text::translate(
                "commands.function.scheduled.multiple",
                &[horizon_client::scoreboard_cmd::format_list(
                    functions
                        .iter()
                        .map(|f| Text::literal(f.id.clone()))
                        .collect(),
                )],
            )
        };
        self.feedback(message);
        let source = self.command_source.unwrap_or_else(|| self.player_source());
        let mut sum: i32 = 0;
        let mut any = false;
        let mut failed = false;
        for f in functions {
            match self.run_function(&f, args.as_ref(), source)? {
                Outcome::NoReturn => {}
                Outcome::Value(v) => {
                    any = true;
                    sum = sum.wrapping_add(v);
                    let message = Text::translate(
                        "commands.function.result",
                        &[Text::literal(f.id.clone()), Text::literal(v.to_string())],
                    );
                    self.feedback(message);
                }
                Outcome::Fail => {
                    any = true;
                    failed = true;
                    self.feedback(Text::translate(
                        "commands.function.result",
                        &[Text::literal(f.id.clone()), Text::literal("0")],
                    ));
                }
            }
        }
        self.exec.callback = Some(if failed && sum == 0 {
            Some((false, 0))
        } else if any {
            Some((true, sum))
        } else {
            None
        });
        Ok(sum)
    }
    /// `return <value>` and `return fail` (`return run` is a chain).
    pub(super) fn cmd_return(&mut self, id: &str, ctx: &Context) -> R<i32> {
        if id == "return.fail" {
            self.finish_return(None);
            self.exec.callback = Some(Some((false, 0)));
            return Ok(0);
        }
        let value = match ctx.get("value") {
            Some(Arg::Int(v)) => *v,
            _ => 0,
        };
        self.finish_return(Some(value));
        Ok(value)
    }
    pub(super) fn cmd_schedule(&mut self, id: &str, ctx: &Context) -> R<i32> {
        if id == "schedule.clear" {
            let Some(Arg::Resource(name)) = ctx.get("function").cloned() else {
                return Ok(0);
            };
            let before = self.fns.schedule.len();
            self.fns.schedule.retain(|e| e.id != name);
            let count = before - self.fns.schedule.len();
            if count == 0 {
                return Err(err("commands.schedule.cleared.failure", &[&name]));
            }
            self.feedback(Text::translate(
                "commands.schedule.cleared.success",
                &[Text::literal(count.to_string()), Text::literal(name)],
            ));
            self.sync_schedule_registry();
            return Ok(count as i32);
        }
        let Some(Arg::Resource(name)) = ctx.get("function").cloned() else {
            return Ok(0);
        };
        let time = match ctx.get("time") {
            Some(Arg::Int(t)) => *t,
            _ => 0,
        };
        if time == 0 {
            return Err(err("commands.schedule.same_tick", &[]));
        }
        let replace = !ctx.has("append");
        let tick_time = self.world.game_time + i64::from(time);
        let schedule_id = match name.strip_prefix('#') {
            // An unknown tag is empty, and scheduling it is allowed.
            Some(_) => name.clone(),
            None => {
                let Some(f) = self.fns.store.functions.get(&name) else {
                    return Err(err("arguments.function.unknown", &[&name]));
                };
                if f.is_macro() {
                    return Err(err("commands.schedule.macro", &[]));
                }
                name.clone()
            }
        };
        if replace {
            self.fns.schedule.retain(|e| e.id != schedule_id);
        }
        if !self
            .fns
            .schedule
            .iter()
            .any(|e| e.id == schedule_id && e.time == tick_time)
        {
            self.fns.next_seq += 1;
            let seq = self.fns.next_seq;
            self.fns.schedule.push(Scheduled {
                id: schedule_id,
                time: tick_time,
                seq,
            });
        }
        let key = if name.starts_with('#') {
            "commands.schedule.created.tag"
        } else {
            "commands.schedule.created.function"
        };
        self.feedback(Text::translate(
            key,
            &[
                Text::literal(name.trim_start_matches('#').to_owned()),
                Text::literal(time.to_string()),
                Text::literal(tick_time.to_string()),
            ],
        ));
        self.sync_schedule_registry();
        Ok((tick_time.rem_euclid(i64::from(i32::MAX))) as i32)
    }
    /// ReloadCommand: the message, then the (asynchronous) reload; returns 0.
    pub(super) fn cmd_reload(&mut self) -> R<i32> {
        self.feedback(Text::translate("commands.reload.success", &[]));
        self.reload_datapacks(true, &self.level.enabled_features.clone())
            .map_err(Self::reload_failure)?;
        Ok(0)
    }
    /// `PackLocationInfo.createChatLink`: the id decorated with its source
    /// (`pack.nameAndSource`, grey) in brackets, green when enabled.
    fn pack_link(&self, id: &str, enabled: bool) -> Text {
        let hint = if enabled { "disable" } else { "enable" };
        let style = text::Style {
            color: Some(if enabled { text::GREEN } else { text::RED }),
            click: Some(text::ClickEvent::SuggestCommand(format!(
                "/datapack {hint} \"{id}\""
            ))),
            ..text::Style::default()
        };
        let description = self
            .fns
            .packs
            .iter()
            .find(|p| p.id == id)
            .map_or_else(|| "Default data pack".to_owned(), |p| p.description());
        let style = text::Style {
            hover: Some(Box::new(Text::literal(format!("{id}\n{description}")))),
            ..style
        };
        let source = if id == "vanilla" {
            "pack.source.builtin"
        } else if FEATURE_PACKS.iter().any(|(p, _)| *p == id) {
            "pack.source.feature"
        } else {
            "pack.source.world"
        };
        let name = Text::translate(
            "pack.nameAndSource",
            &[Text::literal(id), Text::translate(source, &[])],
        )
        .with_color(text::GRAY);
        bracketed(name).with_style(&style)
    }
    /// The selected packs, lowest priority first.
    fn selected_packs(&self) -> Vec<String> {
        let vanilla = (!self.cmds.vanilla_disabled).then(|| "vanilla".to_owned());
        vanilla
            .into_iter()
            .chain(self.fns.enabled.iter().cloned())
            .collect()
    }
    /// `FileUtil.isPathPartPortable`: no trailing dot and no Windows device
    /// name (CON, PRN, AUX, NUL, COM and LPT with a digit from 1 to 9).
    fn is_portable_name(name: &str) -> bool {
        let stem = name.split('.').next().unwrap_or_default();
        let device = match stem.len() {
            3 => matches!(stem, "com" | "con" | "prn" | "aux" | "nul"),
            4 => matches!(&stem[..3], "com" | "lpt") && matches!(stem.as_bytes()[3], b'1'..=b'9'),
            _ => false,
        };
        !device && !name.ends_with('.')
    }
    /// `DataPackCommand.createPack`: a pack folder with its `data` folder and a
    /// `pack.mcmeta` in the world's datapacks folder.
    fn create_pack(&mut self, ctx: &Context) -> R<i32> {
        let id = match ctx.get("id") {
            Some(Arg::Str(s)) => s.clone(),
            _ => String::new(),
        };
        let description = self.component_arg(ctx, "description")?;
        // FileUtil.isValidPathSegment: lowercase letters, digits, '-', '.' and '_'.
        let valid = !id.is_empty()
            && id != "."
            && id != ".."
            && id
                .chars()
                .all(|c| matches!(c, 'a'..='z' | '0'..='9' | '-' | '.' | '_'));
        if !valid {
            return Err(err("commands.datapack.create.invalid_name", &[&id]));
        }
        if !Self::is_portable_name(&id) {
            return Err(err("commands.datapack.create.invalid_full_name", &[&id]));
        }
        // Without a world there is no datapacks folder to write to.
        if self.world_root.as_os_str().is_empty() {
            return Err(err("commands.datapack.create.io_failure", &[&id]));
        }
        let datapacks = self.world_root.join("datapacks");
        let pack = datapacks.join(&id);
        if pack.exists() {
            return Err(err("commands.datapack.create.already_exists", &[&id]));
        }
        // The data pack version (SharedConstants), written as a bare major
        // number for both bounds, as vanilla does.
        let pack_format = horizon_core::version::DATA_PACK_FORMAT.0;
        let mcmeta = serde_json::json!({
            "pack": {
                "description": description.component(),
                "min_format": pack_format,
                "max_format": pack_format,
            }
        });
        let written = std::fs::create_dir_all(&datapacks)
            .and_then(|()| std::fs::create_dir(&pack))
            .and_then(|()| std::fs::create_dir(pack.join("data")))
            .and_then(|()| {
                std::fs::write(
                    pack.join("pack.mcmeta"),
                    serde_json::to_string_pretty(&mcmeta).unwrap_or_default(),
                )
            });
        if written.is_err() {
            return Err(err("commands.datapack.create.io_failure", &[&id]));
        }
        self.feedback(Text::translate(
            "commands.datapack.create.success",
            &[Text::literal(id)],
        ));
        Ok(1)
    }
    pub(super) fn cmd_datapack(&mut self, id: &str, ctx: &Context) -> R<i32> {
        let name = match ctx.get("name") {
            Some(Arg::Str(n)) => Some(n.clone()),
            _ => None,
        };
        let selected = self.selected_packs();
        let previous_enabled = self.fns.enabled.clone();
        let previous_vanilla_disabled = self.cmds.vanilla_disabled;
        // `DataPackCommand.listAvailablePacks`: a feature pack the world's
        // features do not cover is not available.
        let available: Vec<String> = std::iter::once("vanilla".to_owned())
            .chain(self.fns.packs.iter().map(|p| p.id.clone()))
            .filter(|p| !selected.contains(p))
            .filter(|p| {
                FEATURE_PACKS
                    .iter()
                    .find(|(f, _)| *f == p.as_str())
                    .is_none_or(|(_, flag)| self.level.enabled_features.iter().any(|f| f == flag))
            })
            .collect();
        let known = |g: &Self, n: &str| {
            n == "vanilla"
                || FEATURE_PACKS.iter().any(|(p, _)| *p == n)
                || g.fns.packs.iter().any(|p| p.id == n)
        };
        match id {
            "datapack.create" => self.create_pack(ctx),
            "datapack.list" => {
                let want_enabled = ctx.has("enabled") || (!ctx.has("available"));
                let want_available = ctx.has("available") || (!ctx.has("enabled"));
                let mut total = 0;
                if want_enabled {
                    if selected.is_empty() {
                        self.feedback(Text::translate("commands.datapack.list.enabled.none", &[]));
                    } else {
                        let links: Vec<Text> =
                            selected.iter().map(|p| self.pack_link(p, true)).collect();
                        total += links.len();
                        self.feedback(Text::translate(
                            "commands.datapack.list.enabled.success",
                            &[
                                Text::literal(links.len().to_string()),
                                horizon_client::scoreboard_cmd::format_list(links),
                            ],
                        ));
                    }
                }
                if want_available {
                    if available.is_empty() {
                        self.feedback(Text::translate(
                            "commands.datapack.list.available.none",
                            &[],
                        ));
                    } else {
                        let links: Vec<Text> =
                            available.iter().map(|p| self.pack_link(p, false)).collect();
                        total += links.len();
                        self.feedback(Text::translate(
                            "commands.datapack.list.available.success",
                            &[
                                Text::literal(links.len().to_string()),
                                horizon_client::scoreboard_cmd::format_list(links),
                            ],
                        ));
                    }
                }
                Ok(total as i32)
            }
            "datapack.enable" => {
                let name = name.unwrap_or_default();
                if !known(self, &name) {
                    return Err(err("commands.datapack.unknown", &[&name]));
                }
                if selected.contains(&name) {
                    return Err(err("commands.datapack.enable.failed", &[&name]));
                }
                if let Some((_, flag)) = FEATURE_PACKS.iter().find(|(p, _)| *p == name)
                    && !self.level.enabled_features.iter().any(|f| f == flag)
                {
                    // `DataPackCommand`: the pack's requested features must
                    // be a subset of the world's enabled ones.
                    return Err(err(
                        "commands.datapack.enable.failed.no_flags",
                        &[&name, flag],
                    ));
                }
                if name == "vanilla" {
                    self.cmds.vanilla_disabled = false;
                } else {
                    let existing = match ctx.get("existing") {
                        Some(Arg::Str(e)) => Some(e.clone()),
                        _ => None,
                    };
                    let position = if ctx.has("first") {
                        0
                    } else if let Some(e) = &existing {
                        let Some(i) = self.fns.enabled.iter().position(|p| p == e) else {
                            return Err(err("commands.datapack.unknown", &[e]));
                        };
                        if ctx.has("before") { i } else { i + 1 }
                    } else {
                        self.fns.enabled.len()
                    };
                    self.fns.enabled.insert(position, name.clone());
                }
                self.feedback(Text::translate(
                    "commands.datapack.modify.enable",
                    &[self.pack_link(&name, true)],
                ));
                self.apply_pack_change(previous_enabled, previous_vanilla_disabled)?;
                Ok(self.selected_packs().len() as i32)
            }
            _ => {
                let name = name.unwrap_or_default();
                if !known(self, &name) {
                    return Err(err("commands.datapack.unknown", &[&name]));
                }
                if !selected.contains(&name) {
                    return Err(err("commands.datapack.disable.failed", &[&name]));
                }
                if FEATURE_PACKS.iter().any(|(p, _)| *p == name) {
                    // `DataPackCommand`'s ERROR_CANNOT_DISABLE_FEATURE: the
                    // flag stays on for the world's life.
                    return Err(err("commands.datapack.disable.failed.feature", &[&name]));
                }
                if name == "vanilla" {
                    self.cmds.vanilla_disabled = true;
                } else {
                    self.fns.enabled.retain(|p| p != &name);
                }
                self.feedback(Text::translate(
                    "commands.datapack.modify.disable",
                    &[self.pack_link(&name, true)],
                ));
                self.apply_pack_change(previous_enabled, previous_vanilla_disabled)?;
                Ok(self.selected_packs().len() as i32)
            }
        }
    }
    /// `ReloadCommand.reloadPacks`: no message of its own. It reloads the
    /// server's data as `/reload` does (`reload_datapacks`), the content
    /// tables with the functions.
    fn apply_pack_change(
        &mut self,
        previous_enabled: Vec<String>,
        previous_vanilla_disabled: bool,
    ) -> R<()> {
        // One resolve for the store and the tables, as `/reload`'s is: the
        // timers already in memory stay, and none come back from disk
        // (`ServerFunctionManager` keeps its queue over a pack change).
        let mut stack = pack_stack(
            &self.world_root,
            &self.options.resources,
            self.cmds.vanilla_disabled,
            &self.level.enabled_features,
        );
        stack.enabled = self.fns.enabled.clone();
        let prepared = match self.prepare_server_reload(&stack) {
            Ok(prepared) => prepared,
            Err(error) => {
                self.fns.enabled = previous_enabled;
                self.cmds.vanilla_disabled = previous_vanilla_disabled;
                return Err(Self::reload_failure(error));
            }
        };
        self.save_datapack_settings();
        self.apply_server_reload(prepared, &stack);
        self.fns.load_pending = true;
        self.reload_world_tables(&stack);
        Ok(())
    }

    fn reload_failure(error: ReloadStageError) -> horizon_client::command::CommandError {
        log::warn!("Horizon: {error}");
        horizon_client::command::CommandError::new(
            Text::translate("commands.reload.failure", &[])
                .append(Text::literal(format!("\n{error}"))),
        )
    }

    // ---- predicates -------------------------------------------------------------------

    /// `ResourceOrIdArgument.getLootPredicate`: accept the inline or named
    /// predicate, then test it with the existing source entity and origin.
    pub(in crate::game) fn check_predicate(&mut self, id: &str) -> R<bool> {
        self.ensure_datapacks();
        // ResourceOrIdArgument retains either an inline value or a named id.
        // Both use the same evaluator and command-source context below.
        let json = if id.starts_with('{') {
            let (tag, _) = horizon_world::nbt::parse_snbt(id)
                .map_err(|e| err("argument.nbt.invalid", &[&e]))?;
            horizon_client::scoreboard::tag_to_json(&tag)
        } else {
            self.fns
                .store
                .predicates
                .get(id)
                .cloned()
                .ok_or_else(|| err("argument.predicate.unknown", &[&id]))?
        };
        let source = self.command_source.unwrap_or_else(|| self.player_source());
        let view = source.entity.map(|t| self.entity_view_asked(t, &json));
        Ok(self.conditions_hold(&json, view.as_ref(), source.pos))
    }
}

/// The built-in feature packs and the feature flag each needs (none of them
/// enabled in a world created without experiments).
const FEATURE_PACKS: &[(&str, &str)] = &[
    ("minecart_improvements", "minecraft:minecart_improvements"),
    ("redstone_experiments", "minecraft:redstone_experiments"),
    ("trade_rebalance", "minecraft:trade_rebalance"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs prepared resources"]
    fn world_open_loads_functions_before_first_tick() {
        use crate::game::{native_entity_refs::tests::with_game, world_loading::WorldRequest};

        let root = std::env::current_dir()
            .unwrap()
            .join("target")
            .join(format!("world-open-datapacks-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let pack = root.join("datapacks/test");
        for path in [
            "data/test/function",
            "data/minecraft/tags/function",
            "data/test/tags/item",
            "data/test/tags/worldgen/biome",
            "data/test/recipe",
        ] {
            std::fs::create_dir_all(pack.join(path)).unwrap();
        }
        std::fs::write(pack.join("pack.mcmeta"), "{}").unwrap();
        let function = pack.join("data/test/function/start.mcfunction");
        std::fs::write(
            &function,
            "scoreboard objectives add loaded dummy\nscoreboard players add times loaded 1\n",
        )
        .unwrap();
        std::fs::write(
            pack.join("data/minecraft/tags/function/load.json"),
            r#"{"values":["test:start"]}"#,
        )
        .unwrap();
        std::fs::write(
            pack.join("data/test/tags/item/sample.json"),
            r#"{"values":["minecraft:stone"]}"#,
        )
        .unwrap();
        std::fs::write(pack.join("data/test/recipe/sample.json"), r#"{"type":"minecraft:crafting_shapeless","ingredients":["minecraft:stone"],"result":{"id":"minecraft:diamond","count":1}}"#).unwrap();
        std::fs::write(
            pack.join("data/test/tags/worldgen/biome/feature_marker.json"),
            r#"{"values":[]}"#,
        )
        .unwrap();
        std::fs::write(
            root.join("datapacks.json"),
            r#"{"enabled":["file/test"],"disabled":[]}"#,
        )
        .unwrap();
        with_game(|game| {
            game.load_world(WorldRequest {
                rule_registry: std::sync::Arc::new(
                    horizon_client::gamerules::RuleRegistry::vanilla(),
                ),
                save_root: root.clone(),
                folder: "test".into(),
                name: Some("test".into()),
                seed: Some(1),
                creative: true,
                create: None,
            })
            .unwrap();
            assert!(game.fns.store.functions.contains_key("test:start"));
            assert_eq!(
                game.fns.store.tag_functions("minecraft:load"),
                ["test:start"]
            );
            assert!(game.items().item_tags["test:sample"].contains("minecraft:stone"));
            assert!(game.stations().recipe_index.contains("test:sample"));
            // Changes on disk must not make the first tick reload any table.
            std::fs::write(&function, "scoreboard objectives add changed dummy\n").unwrap();
            std::fs::write(
                pack.join("data/test/tags/item/sample.json"),
                r#"{"values":["minecraft:diamond"]}"#,
            )
            .unwrap();
            std::fs::remove_file(pack.join("data/test/recipe/sample.json")).unwrap();
            game.tick_functions();
            assert!(game.score.board.objective("loaded").is_some());
            assert!(game.score.board.objective("changed").is_none());
            game.tick_functions();
            assert_eq!(game.score.board.score("times", "loaded").unwrap().value, 1);
            assert!(game.items().item_tags["test:sample"].contains("minecraft:stone"));
            assert!(game.stations().recipe_index.contains("test:sample"));

            // Changing packs must not reload an old timer from disk.
            game.fns.schedule.push(Scheduled {
                id: "test:start".into(),
                time: 1000,
                seq: 1,
            });
            game.save_schedule();
            game.fns.schedule.clear();
            assert!(game.run_command("datapack disable \"file/test\""));
            assert!(game.fns.schedule.is_empty());
            assert!(game.run_command("datapack enable \"file/test\""));
            assert!(game.fns.schedule.is_empty());

            // World-open selection and completions must both use the incoming
            // level's flags, even while `self.level` is still the old level.
            game.ui.registries.by_registry.clear();
            game.level.enabled_features = Default::default();
            std::fs::write(
                pack.join("pack.mcmeta"),
                r#"{"features":{"enabled":["test:feature"]}}"#,
            )
            .unwrap();
            let stack = game
                .reload_datapacks(false, &["test:feature".into()])
                .unwrap();
            assert!(
                game.ui.registries.by_registry["minecraft:worldgen/biome"]
                    .tags
                    .contains(&"test:feature_marker".to_owned())
            );
            (game.world_tables, game.health_resources) =
                game.build_world_tables(&stack, false).unwrap();
            assert!(game.items().item_tags["test:sample"].contains("minecraft:diamond"));
        });
        std::fs::remove_dir_all(root).unwrap();
    }

    /// Two packs override the same recipe: the later one in the selected
    /// (`datapacks.json`) order wins, not the later one alphabetically.
    #[test]
    fn content_overlay_follows_enabled_order() {
        for order in [["file/a", "file/b"], ["file/b", "file/a"]] {
            let dir = std::env::temp_dir().join(format!(
                "horizon-overlay-{}-{}",
                std::process::id(),
                order[0].replace('/', "_")
            ));
            let _ = std::fs::remove_dir_all(&dir);
            for name in ["a", "b"] {
                let pack = dir.join("datapacks").join(name);
                std::fs::create_dir_all(pack.join("data/x/recipe")).unwrap();
                std::fs::write(pack.join("pack.mcmeta"), "{}").unwrap();
                std::fs::write(pack.join("data/x/recipe/r.json"), name).unwrap();
            }
            let json = serde_json::json!({"enabled": order, "disabled": []});
            std::fs::write(dir.join("datapacks.json"), json.to_string()).unwrap();
            let stack = pack_stack(&dir, std::path::Path::new(""), true, &[]);
            let files = content_overlay(&stack);
            let (_, versions) = files.iter().find(|(id, _)| id == "recipe/x:r").unwrap();
            let last = versions.last().unwrap();
            assert_eq!(
                last.as_slice(),
                order[1].trim_start_matches("file/").as_bytes()
            );
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    /// What a native mod contributes (`native_data_pack_files`) joins the load
    /// the tables are built from, a world's first open and `/reload` alike
    /// (`Game::build_world_tables` is both), and sits below the world's own
    /// packs. Each load asks again, so it is the current contribution that
    /// counts.
    #[test]
    fn contributed_files_join_the_load_below_the_worlds_packs() {
        let dir = std::env::temp_dir().join(format!("horizon-contributed-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let pack = dir.join("datapacks").join("pack");
        std::fs::create_dir_all(pack.join("data/pkg/tags/item")).unwrap();
        std::fs::write(pack.join("pack.mcmeta"), "{}").unwrap();
        std::fs::write(
            pack.join("data/pkg/tags/item/both.json"),
            r#"{"replace": true, "values": ["pkg:from_pack"]}"#,
        )
        .unwrap();
        let settings = serde_json::json!({"enabled": ["file/pack"], "disabled": []});
        std::fs::write(dir.join("datapacks.json"), settings.to_string()).unwrap();
        let tag = |entry: &str| format!(r#"{{"values": ["{entry}"]}}"#).into_bytes();
        let stack = pack_stack(&dir, std::path::Path::new(""), true, &[]);
        let files = overlay_files(
            &stack,
            vec![
                ("tags/pkg:item/both".to_owned(), tag("pkg:from_mod")),
                ("tags/pkg:item/mod_only".to_owned(), tag("pkg:only_mod")),
            ],
        );
        // The pack's file is the path's last (`PackOverlay::top`), and its
        // `replace` drops what the mod put below it.
        let (_, versions) = files
            .iter()
            .find(|(id, _)| id == "tags/pkg:item/both")
            .unwrap();
        assert!(horizon_world::loot::PackOverlay::top(versions).starts_with(b"{\"replace\""));
        let overlay = horizon_world::loot::PackOverlay { files: &files };
        let tags = horizon_items::items::load_tags(&dir, "item", &overlay).unwrap();
        assert!(tags["pkg:both"].contains("pkg:from_pack"));
        assert!(!tags["pkg:both"].contains("pkg:from_mod"));
        // A path only the mod has reaches the tables too.
        assert_eq!(
            tags["pkg:mod_only"].iter().collect::<Vec<_>>(),
            ["pkg:only_mod"]
        );
        // The next load asks again: the new contribution, not the first's.
        let files = overlay_files(
            &stack,
            vec![("tags/pkg:item/mod_only".to_owned(), tag("pkg:second_load"))],
        );
        let overlay = horizon_world::loot::PackOverlay { files: &files };
        let tags = horizon_items::items::load_tags(&dir, "item", &overlay).unwrap();
        assert_eq!(
            tags["pkg:mod_only"].iter().collect::<Vec<_>>(),
            ["pkg:second_load"]
        );
        // A file that isn't JSON is skipped alone, not the whole load.
        let files = overlay_files(
            &stack,
            vec![
                ("tags/pkg:item/mod_only".to_owned(), b"{not json".to_vec()),
                ("tags/pkg:item/good".to_owned(), tag("pkg:good")),
            ],
        );
        let overlay = horizon_world::loot::PackOverlay { files: &files };
        let tags = horizon_items::items::load_tags(&dir, "item", &overlay).unwrap();
        assert!(!tags.contains_key("pkg:mod_only"));
        assert!(tags["pkg:good"].contains("pkg:good"));
        // With nothing contributed the overlay is the packs' own.
        assert_eq!(overlay_files(&stack, Vec::new()), content_overlay(&stack));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
