//! The modding book's wasm sample, built by `cargo horizon-mod build --wasm`, loaded as the game
//! loads it. CI builds the sample first and points `HORIZON_WASM_SAMPLE` at its `.hmod` and
//! `HORIZON_WASM_STD` at the std member; the test is ignored elsewhere.
use std::sync::Mutex;

use horizon_mod_wasm::{Refusal, WasmPackage};
use horizon_mods::manifest::Manifest;
use mod_api::Text;

/// The built sample: its manifest and its `mod.wasm`, and the std member.
fn sample() -> (Manifest, Vec<u8>, Vec<u8>) {
    let path = |var: &str| std::path::PathBuf::from(std::env::var(var).expect(var));
    let package = path("HORIZON_WASM_SAMPLE");
    let std_member = std::fs::read(path("HORIZON_WASM_STD")).unwrap();
    let manifest = horizon_mods::Package::read(&package).unwrap().manifest;
    let module = horizon_mods::Package::read_wasm(&package).unwrap();
    (manifest, module, std_member)
}

fn load(manifest: Manifest, module: Vec<u8>, std_member: &[u8]) -> horizon_mod_wasm::Loaded {
    horizon_mod_wasm::load(
        &horizon_mod_wasm::RuntimeConfig::default(),
        std_member,
        vec![WasmPackage { manifest, module }],
    )
}

/// The sample's manifest with `injection` as its one injection.
fn listing(manifest: &Manifest, injection: &str) -> Manifest {
    let wasm = manifest.wasm.as_ref().unwrap();
    Manifest::parse(&format!(
        "id = \"hello_wasm\"\nname = \"Hello Wasm\"\nversion = \"0.1.0\"\napi = \"0.2\"\n\
         capabilities = [\"registries\"]\n\n[wasm]\nsha512 = \"{}\"\ntoolchain = \"{}\"\n\n\
         [[native.injection]]\nid = \"listen\"\nkind = \"HEAD\"\n{injection}\n",
        wasm.sha512(),
        wasm.toolchain(),
    ))
    .unwrap()
}

/// The chat lines a point's listeners broadcast.
#[derive(Default)]
struct Server(Mutex<Vec<String>>);

impl mod_api::Host for Server {}

impl mod_api::ServerHost for Server {
    fn messages(&self) -> &dyn mod_api::Messages {
        self
    }
}

impl mod_api::Messages for Server {
    fn broadcast(&self, line: Text) {
        self.0.lock().unwrap().push(line.plain());
    }
}

#[test]
#[ignore = "needs the sample built to mod.wasm (CI's wasm-mods job)"]
fn the_sample_mod_loads_subscribes_and_registers_its_part() {
    let (manifest, module, std_member) = sample();
    // A manifest that doesn't list where the mod listens, or lists a point this game lacks, refuses
    // it; neither registers anything.
    let tick = "target = \"horizon_client::game::Game<'_>::tick\"";
    let undeclared = load(
        listing(&manifest, &format!("{tick}\ntrait_method = \"tick_world\"")),
        module.clone(),
        &std_member,
    );
    assert!(
        matches!(&undeclared.refused[..], [(_, Refusal::NoSuchPoint { .. })]),
        "a target that isn't the method's function: {:?}",
        undeclared.refused
    );
    let elsewhere = "target = \"horizon_client::game::Game<'_>::tick_world\"\n\
                     trait_method = \"tick_world\"";
    let undeclared = load(listing(&manifest, elsewhere), module.clone(), &std_member);
    assert!(
        matches!(
            &undeclared.refused[..],
            [(_, Refusal::UndeclaredPoint { point })] if point == "tick"
        ),
        "the mod listens at tick, which its manifest doesn't list: {:?}",
        undeclared.refused
    );

    let mut loaded = load(manifest, module, &std_member);
    assert!(loaded.refused.is_empty(), "{:?}", loaded.refused);
    let [entry] = loaded.mods.as_slice() else {
        panic!("one mod");
    };
    use mod_api::__private::game::Fields;
    let subscribed = entry.fields().hooks.subscribed();
    assert!(
        subscribed
            .iter()
            .any(|hook| matches!(hook, Some(mod_api::HookId::ServerChat))),
        "the sample listens to server_chat"
    );
    // Its `register` runs in the mod and attaches a part to sticks, which listens to use_item.
    mod_api::__private::install_parts(&loaded.mods).unwrap();
    assert!(mod_api::__private::has_part_hook(mod_api::HookId::UseItem));

    // Its mod.wasm fills the tick point: every 200 server ticks it says so in chat.
    for listening in std::mem::take(&mut loaded.listening) {
        listening.register().unwrap();
    }
    mod_points::__private::seal();
    assert!(
        mod_points::__private::POINTS[mod_points::PointId::Tick as usize]
            .load(std::sync::atomic::Ordering::Acquire)
    );
    let mut server = Server::default();
    for _ in 0..400 {
        let mut args = mod_points::TickArgs::__new();
        mod_points::__private::fire(mod_points::PointId::Tick, "tick", &mut |listener| {
            listener.tick(mod_api::HostSide::Server(&mut server), &mut args)
        });
        assert!(!args.cancel, "the sample lets the tick run");
    }
    assert_eq!(
        *server.0.lock().unwrap(),
        [
            "hello_wasm: 200 server ticks",
            "hello_wasm: 400 server ticks"
        ]
    );
}
