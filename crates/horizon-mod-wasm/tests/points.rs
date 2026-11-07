//! Native and wasm functions at one point preserve registration/dependency order.
//! CI builds the chat guest; the test is ignored without that real artifact.
use mod_api::HostSide;
use mod_points::HandleChatInputArgs;

use mod_api::testing::FakeHost;

fn native_tail<const T: char>(_: HostSide<'_>, args: &mut HandleChatInputArgs<'_>) {
    args.message = format!("{}{T}", args.message).into();
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
            manifest: horizon_mod_package::Package::read(&package)
                .unwrap()
                .manifest,
            module: horizon_mod_package::Package::read_wasm(&package).unwrap(),
        }],
    );
    assert!(loaded.refused.is_empty(), "{:?}", loaded.refused);
    // The game installs each owner's native/wasm slots in dependency order, not alphabetically.
    mod_points::register(&mod_points::listeners::handle_chat_input(
        "z_native",
        5,
        "tail",
        native_tail::<'N'>,
    ))
    .unwrap();
    for listening in loaded.listening {
        listening.register().unwrap();
    }
    mod_points::register(&mod_points::listeners::handle_chat_input(
        "a_native",
        0,
        "tail",
        native_tail::<'M'>,
    ))
    .unwrap();
    mod_points::__private::seal();
    let mut args = HandleChatInputArgs::__new("x");
    mod_points::dispatch::handle_chat_input(HostSide::Server(&mut FakeHost::new()), &mut args);
    assert_eq!(args.message, "xVNWM");
}
