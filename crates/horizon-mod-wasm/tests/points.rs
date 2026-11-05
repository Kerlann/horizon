//! A point's listeners run in the installed order whatever their build target: native mods' first,
//! then wasm mods' in dependency order, as their hooks do. CI builds `tests/points/chat_tail` first
//! and points `HORIZON_WASM_POINTS` at that folder and `HORIZON_WASM_STD` at the std member; the
//! test is ignored elsewhere.
use mod_api::HostSide;
use mod_points::{HandleChatInputArgs, PointId, Points};

struct Server;

impl mod_api::Host for Server {}

impl mod_api::ServerHost for Server {}

/// A native mod appending `N` at the same point.
struct NativeTail;

impl Points for NativeTail {
    fn subscribed(&self) -> &'static [Option<PointId>] {
        &[mod_points::ids::handle_chat_input]
    }
    fn handle_chat_input(&self, _host: HostSide<'_>, args: &mut HandleChatInputArgs<'_>) {
        args.message = format!("{}N", args.message).into();
    }
}

#[test]
#[ignore = "needs tests/points/chat_tail built to mod.wasm (CI's wasm-mods job)"]
fn native_and_wasm_listeners_at_a_point_run_in_the_installed_order() {
    let path = |var: &str| std::path::PathBuf::from(std::env::var(var).expect(var));
    let package = path("HORIZON_WASM_POINTS").join("chat_tail/target/horizon-mod/chat_tail.hmod");
    let loaded = horizon_mod_wasm::load(
        &horizon_mod_wasm::RuntimeConfig::default(),
        &std::fs::read(path("HORIZON_WASM_STD")).unwrap(),
        vec![horizon_mod_wasm::WasmPackage {
            manifest: horizon_mods::Package::read(&package).unwrap().manifest,
            module: horizon_mods::Package::read_wasm(&package).unwrap(),
        }],
    );
    assert!(loaded.refused.is_empty(), "{:?}", loaded.refused);
    // Loading registers nothing: the game registers the native mods' listeners, then these.
    mod_points::register("native_tail", &NativeTail).unwrap();
    for listening in loaded.listening {
        listening.register().unwrap();
    }
    mod_points::__private::seal();
    let mut args = HandleChatInputArgs::__new("x");
    mod_points::__private::fire(
        PointId::HandleChatInput,
        "handle_chat_input",
        &mut |listener| listener.handle_chat_input(HostSide::Server(&mut Server), &mut args),
    );
    assert_eq!(args.message, "xNW");
}
