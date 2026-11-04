//! Startup shared by the gameplay library and its executable.
use crate::{crash, game, logging, ram_limit};
use horizon_mods::selection::{Argument, LaunchArguments};
use mod_api::__private::game::*;

/// Start the game. The executable supplies the allocator and retains the mod API marker.
pub fn main() {
    main_with(Vec::new(), &[]);
}

/// Start the game with named trait mods in dispatch order.
pub fn main_with(
    mut mods: Vec<mod_api::ModEntry>,
    built: &'static [horizon_mods::runtime::BuiltMod],
) {
    let exe = std::env::current_exe();
    let native: crate::Result<_> = exe
        .as_ref()
        .map_err(|e| e.to_string().into())
        .and_then(|exe| horizon_mods::runtime::Runtime::load(exe, built).map_err(Into::into));
    let native = match native {
        Ok(native) => native,
        Err(error) => {
            eprintln!("Horizon: {error}");
            std::process::exit(game::exit_code::ERROR);
        }
    };
    crate::gfx::native_render::set_mods_built_in(!native.built().is_empty());
    horizon_threads::configure_allocator();
    horizon_client::telemetry::mark_start();
    horizon_client::spans::startup::init(std::time::Instant::now());
    let launch: crate::Result<_> = std::env::current_dir().map_err(Into::into).and_then(|cwd| {
        Ok(LaunchArguments::parse(
            std::env::args().skip(1),
            &cwd,
            &logging::game_home(),
            std::env::var_os("HORIZON_HOME").as_deref(),
        )?)
    });
    let launch = match launch {
        Ok(launch) => launch,
        Err(error) => {
            eprintln!("Horizon: {error}");
            std::process::exit(game::exit_code::USAGE);
        }
    };
    // Wasm mods join the native ones after them, in dependency order, from the same selection
    // as content mods. Then the mods are installed, before anything below (logging, the crash
    // handler) that a trait injection may call into.
    if let Ok(exe) = &exe {
        let switch = |name: &str| {
            launch
                .rest
                .iter()
                .any(|arg| matches!(arg, Argument::Switch(s) if s == name))
        };
        let mut budgets = mod_api::__private::wasm_host::Budgets::default();
        if switch("--wasm-fuel") {
            budgets.fuel = Some(Default::default());
        }
        let config = horizon_mod_wasm::RuntimeConfig {
            pulley: switch("--wasm-interpreter"),
            cache: Some(launch.context.home.join("wasm-cache")),
            budgets,
        };
        let game_dir = &launch.context.game_dir;
        mods.extend(horizon_mod_wasm::load_installed(
            exe,
            game_dir,
            &launch.context.dev_mods,
            &config,
        ));
    }
    mod_api::__private::install(mods.into());
    crate::game::points::attach_installed();
    let help = launch.is_information_request();
    logging::set_game_home(launch.context.home.clone());
    // The dedicated server logs in its own folder, as vanilla's does; the game in its home.
    let (home, side, thread) = if launch
        .rest
        .iter()
        .any(|arg| matches!(arg, Argument::Dedicated(_)))
    {
        (launch.context.game_dir.clone(), "server", "Server thread")
    } else {
        // A player's own log stays separate; crash telemetry follows the shared home.
        if let Some(dir) = launch.rest.iter().find_map(|arg| match arg {
            Argument::Value(name, value) if name == "--player-dir" => Some(value),
            _ => None,
        }) {
            logging::set_player_dir(std::path::absolute(dir).unwrap_or_else(|_| dir.into()));
        }
        (launch.context.home.clone(), "client", "Render thread")
    };
    if !help {
        logging::init(
            logging::player_dir_override().unwrap_or(home.as_path()),
            thread,
        );
        // The log's first line names the build, so a bug report that carries the log says which one it was.
        log::info!("Horizon {}", crash::version());
        crash::install(home.clone(), side);
        ram_limit::start(&home, side);
    }
    let renderers = mod_api::__private::mods()
        .iter()
        .filter_map(|m| m.fields().hooks.render())
        .collect();
    if crate::gfx::native_render::install(renderers).is_err() {
        log::warn!("Trait renderers already installed; keeping the first list");
    }
    if horizon_world::native_load_hooks::install(
        horizon_world::native_load_hooks::NativeLoadHooks {
            loot_table_load: |context, id, table| {
                let context = mod_api::OwningLoad::new(context);
                mod_api::dispatch_loot_table_load(&context, id, table);
            },
            modify_default_components: |context, item, components| {
                let context = mod_api::OwningLoad::new(context);
                mod_api::dispatch_modify_default_components(&context, item, components);
            },
            loot_function: |kind, data, ctx, stack| {
                mod_api::dispatch_loot_function(kind, data, ctx, stack);
            },
            loot_condition: |kind, data, ctx, holds| {
                mod_api::dispatch_loot_condition(kind, data, ctx, holds);
            },
        },
    )
    .is_err()
    {
        log::warn!("Native data-load hooks already installed; keeping the first callbacks");
    }
    if horizon_items::native_enchantment_hooks::install(
        crate::enchantment_levels::get_enchantment_level,
    )
    .is_err()
    {
        log::warn!("The enchantment level hook is already installed; keeping the first");
    }
    if !crate::native_client_hooks::install(crate::native_client_hooks::NativeClientHooks {
        menu_slot_rules: |context, menu, slot, stack, may_place, max_stack| {
            let context = mod_api::OwningLoop::new(context);
            mod_api::dispatch_menu_slot_rules(&context, menu, slot, stack, may_place, max_stack);
        },
        item_stacked_on_other: |context, event, canceled| {
            let context = mod_api::OwningLoop::new(context);
            mod_api::dispatch_item_stacked_on_other(&context, event, canceled);
        },
        living_heal: |context, entity, amount, cancel| {
            let context = mod_api::OwningLoop::new(context);
            mod_api::dispatch_living_heal(&context, entity, amount, cancel);
        },
        living_breathe: |context, entity, kind, can_breathe, consume, refill| {
            let context = mod_api::OwningLoop::new(context);
            mod_api::dispatch_living_breathe(&context, entity, kind, can_breathe, consume, refill);
        },
        farmland_trample: |context, entity, pos, state, distance, cancel| {
            let context = mod_api::OwningLoop::new(context);
            mod_api::dispatch_farmland_trample(&context, entity, pos, state, distance, cancel);
        },
        has_mods: || !mod_api::__private::mods().is_empty(),
        player_name_format: |context, username, displayname| {
            let context = mod_api::OwningLoop::new(context);
            mod_api::dispatch_player_name_format(&context, username, displayname);
        },
        player_tab_list_name_format: |context, player, name, display_name| {
            let context = mod_api::OwningLoop::new(context);
            mod_api::dispatch_player_tab_list_name_format(&context, player, name, display_name);
        },
        living_drown: |context, entity, kind, is_drowning, damage_amount, bubble_count, cancel| {
            let context = mod_api::OwningLoop::new(context);
            mod_api::dispatch_living_drown(
                &context,
                entity,
                kind,
                is_drowning,
                damage_amount,
                bubble_count,
                cancel,
            );
        },
        living_fall: |context, entity, kind, distance, multiplier, cancel| {
            let context = mod_api::OwningLoop::new(context);
            mod_api::dispatch_living_fall(&context, entity, kind, distance, multiplier, cancel);
        },
        living_jump: |context, entity, kind| {
            let context = mod_api::OwningLoop::new(context);
            mod_api::dispatch_living_jump(&context, entity, kind);
        },
        living_jump_power: |context, entity, kind, power| {
            let context = mod_api::OwningLoop::new(context);
            mod_api::dispatch_living_jump_power(&context, entity, kind, power);
        },
        living_speed: |context, entity, kind, speed| {
            let context = mod_api::OwningLoop::new(context);
            mod_api::dispatch_living_speed(&context, entity, kind, speed);
        },
        living_flying_speed: |context, entity, kind, speed| {
            let context = mod_api::OwningLoop::new(context);
            mod_api::dispatch_living_flying_speed(&context, entity, kind, speed);
        },
        block_jump_factor: |context, state, factor| {
            let context = mod_api::OwningLoop::new(context);
            mod_api::dispatch_block_jump_factor(&context, state, factor);
        },
        block_speed_factor: |context, state, factor| {
            let context = mod_api::OwningLoop::new(context);
            mod_api::dispatch_block_speed_factor(&context, state, factor);
        },
        player_stays_on_ground_surface: |context, entity, staying| {
            let context = mod_api::OwningLoop::new(context);
            mod_api::dispatch_player_stays_on_ground_surface(&context, entity, staying);
        },
        ender_entity_teleport: |context, entity, kind, from, target, cancel| {
            let context = mod_api::OwningLoop::new(context);
            mod_api::dispatch_ender_entity_teleport(&context, entity, kind, from, target, cancel);
        },
        fluid_flow: |context, fluid, pos, flow| {
            let context = mod_api::OwningLoop::new(context);
            mod_api::dispatch_fluid_flow(&context, fluid, pos, flow);
        },
    }) {
        log::warn!("Native client hooks already installed; keeping the first callbacks");
    }
    raise_open_file_limit();
    if let Err(error) = game::run_with(launch, native) {
        // A bad command line is the caller's mistake, not a crash: no report.
        let usage = error.is::<game::UsageError>();
        log::error!("Horizon: {error}");
        logging::flush();
        // The report's path is the last line on stderr (crash::SAVED_LINE).
        if !help && !usage {
            crash::report_error(&home, side, &*error);
        }
        // Started from Explorer, the console window closes with the game:
        // keep the message on screen until it is read.
        #[cfg(windows)]
        if std::io::IsTerminal::is_terminal(&std::io::stdin()) {
            eprintln!("Press Enter to close.");
            let _ = std::io::stdin().read_line(&mut String::new());
        }
        std::process::exit(if usage {
            game::exit_code::USAGE
        } else {
            game::exit_code::ERROR
        });
    }
    logging::flush();
}

/// The JVM raises its open-file limit to the hard limit as it starts
/// (`-XX:+MaxFDLimit`, on by default), so a vanilla server may hold as many
/// sockets and region files as the host allows, not the soft default (often
/// 1024 in a container). The same here; macOS caps it at `OPEN_MAX`, as the
/// JVM does there.
fn raise_open_file_limit() {
    #[cfg(unix)]
    {
        use rustix::process::{Resource, Rlimit, getrlimit, setrlimit};
        let limit = getrlimit(Resource::Nofile);
        let hard = if cfg!(target_os = "macos") {
            Some(limit.maximum.map_or(10_240, |h| h.min(10_240)))
        } else {
            limit.maximum
        };
        let raised = match (limit.current, hard) {
            (Some(soft), Some(hard)) => hard > soft,
            (Some(_), None) => true,
            (None, _) => false,
        };
        if raised {
            let to = Rlimit {
                current: hard,
                maximum: limit.maximum,
            };
            match setrlimit(Resource::Nofile, to) {
                Ok(()) => log::info!(
                    "Open file limit raised from {:?} to {hard:?}",
                    limit.current
                ),
                Err(e) => log::warn!("Open file limit stays at {:?}: {e}", limit.current),
            }
        }
    }
}
