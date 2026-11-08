use super::*;
use std::{fs, process::Command};

#[test]
fn generated_text_refusals_are_typed_on_both_sides() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let generated = crate::sources(&root, crate::FillMode::Require).unwrap();
    let included = [
        ("mod_api_generated.rs", generated.hooks.as_str()),
        ("mod_api_generated_host.rs", generated.host.as_str()),
    ];
    let mut types = ty::Types::new(index::Index::new(&root, &included)).unwrap();
    let module = types.index.module("mod_api", &[]).unwrap();
    types.index.modules[module].items.extend(syn::parse_file("pub struct TextBatch { pub values: Vec<Option<Text>>, pub shape: [u32; 13] } pub enum TextFormError { Unavailable, TooDeep, Malformed, NonFinite, InvalidString, NestedNull } pub struct TextCarrier; pub struct TextEvent { pub first: u32, pub text: Text, pub last: u32 }").unwrap().items);
    let text = types
        .ty(module, &syn::parse_str("Text").unwrap(), &[])
        .unwrap();
    let batch = types
        .ty(module, &syn::parse_str("TextBatch").unwrap(), &[])
        .unwrap();
    let error = types
        .ty(module, &syn::parse_str("TextFormError").unwrap(), &[])
        .unwrap();
    let Ty::Named(text_id) = text else { panic!() };
    let Ty::Named(error_id) = error else { panic!() };
    // Isolate the adapter contract from TextForm's own tree validation. The real Text
    // name and Via rule are used; the fixture supplies independent refusal values.
    // A second fallible Via models TextForm's Tag carrier, independently of the
    // legacy Tag codec and its limits. Failures at this stage are Text refusals too.
    let carrier = types
        .ty(module, &syn::parse_str("TextCarrier").unwrap(), &[])
        .unwrap();
    let Ty::Named(carrier_id) = carrier else {
        panic!()
    };
    types.defs[carrier_id].kind = ty::Kind::Via(Ty::Prim("u32", "u32"));
    types.defs[text_id].kind = ty::Kind::Via(carrier);
    types.defs[error_id].kind = ty::Kind::Enum(
        [
            "Unavailable",
            "TooDeep",
            "Malformed",
            "NonFinite",
            "InvalidString",
            "NestedNull",
        ]
        .map(String::from)
        .to_vec(),
    );
    let methods: Vec<_> = [
        (
            "direct",
            vec![("input".into(), text.clone())],
            Ty::Prim("u32", "u32"),
        ),
        (
            "batch",
            vec![("input".into(), batch)],
            Ty::Prim("u32", "u32"),
        ),
        ("output", vec![], text),
    ]
    .into_iter()
    .map(|(name, params, ok)| Method {
        via: None,
        owner: "TextProbe",
        name: name.into(),
        receiver: Some(true),
        params,
        ret: Ty::Result(Box::new(ok), Box::new(error.clone())),
    })
    .collect();
    // GuiDraw carries Text through a variant payload, not a direct method argument.
    let gui = types
        .ty(module, &syn::parse_str("GuiDraw").unwrap(), &[])
        .unwrap();
    let gui_method = Method {
        via: None,
        owner: "TextProbe",
        name: "gui_paint".into(),
        receiver: Some(true),
        params: vec![(
            "draws".into(),
            Ty::List(Box::new(gui.clone()), ty::List::Vec),
        )],
        ret: methods[0].ret.clone(),
    };
    wit::refuse(&mut types);
    assert!(
        sides::guest_call(&types, &gui_method, "paint", None)
            .unwrap()
            .contains("Err(Refused::Text(why))")
    );
    let Ty::Named(gui_id) = gui else { panic!() };
    types.defs[gui_id].kind = ty::Kind::Variant(vec![ty::Case {
        rust: "Label".into(),
        wit: "label".into(),
        payload: ty::Payload::Tuple(vec![Ty::Named(text_id)]),
        link: None,
    }]);
    let event = types
        .ty(module, &syn::parse_str("TextEvent").unwrap(), &[])
        .unwrap();
    let health = Method {
        via: None,
        owner: "Hooks",
        name: "health_hud".into(),
        receiver: Some(false),
        params: vec![],
        ret: Ty::Option(Box::new(Ty::List(Box::new(gui), ty::List::Vec))),
    };
    let multi = Method {
        via: None,
        owner: "Hooks",
        name: "multi".into(),
        receiver: Some(false),
        params: vec![("event".into(), Ty::Ref(Box::new(event.clone()), true))],
        ret: Ty::Unit,
    };
    wit::refuse(&mut types);
    let (defs, outs) = wit::reached(
        &types,
        &methods.iter().chain([&health, &multi]).collect::<Vec<_>>(),
    );
    let conversions = glue::conversions(&types, &defs, &outs);
    let health_sig = wit::signature(&types, &health).unwrap();
    assert_eq!(
        wit::wire_results(&types, &health, &health_sig),
        ["result<option<list<gui-draw>>, text-form-error>"]
    );
    let health_value = sides::results(&types, &health, &health_sig, "r", &[]);
    let health_answer = sides::export_answer(
        &types,
        &health,
        &format!(
            "(|| -> Result<_, Refused> {{ let r = Some(vec![GuiDraw::Label(Text(depth))]); Ok({health_value}) }})()"
        ),
    );
    let health_payload =
        sides::export_payload(&types, &health, "guest_exports::health(depth)", "|| None");
    let health_lift = glue::lift(&types, &health.ret, "r0");
    let multi_sig = wit::signature(&types, &multi).unwrap();
    let mut before = String::new();
    sides::before(&types, &multi_sig, &event, "m0", &mut before);
    let multi_value = sides::results(
        &types,
        &multi,
        &multi_sig,
        "r",
        &[(0, "m0".into(), event.clone())],
    );
    let multi_lift = glue::lift(&types, &event, "input");
    let multi_answer = sides::export_answer(
        &types,
        &multi,
        &format!(
            "(|| -> Result<_, Refused> {{ let mut m0 = {multi_lift}; {before} m0.first = 10; m0.text = Text(depth); m0.last = 20; Ok({multi_value}) }})()"
        ),
    );
    let multi_lower = glue::lower(&types, &event, "p0");
    let multi_payload = sides::export_payload(
        &types,
        &multi,
        &format!("guest_exports::multi({multi_lower}, depth)"),
        "|| ()",
    );
    let (multi_pattern, names) = glue::unpack(&multi_sig);
    let sides::Writeback { prepare, apply } = sides::writeback(&types, &multi, &multi_sig, &names);
    let host = methods
        .iter()
        .map(|m| sides::host_import(&types, m, "self", false).unwrap())
        .collect::<String>();
    types.guest.set(true);
    let guest = methods
        .iter()
        .map(|m| {
            sides::guest_call(&types, m, &format!("super::imports::{}", m.name), None).unwrap()
        })
        .collect::<String>();
    let refused = fs::read_to_string(root.join("crates/horizon-mod-api/src/wasm_refused.rs"))
        .unwrap()
        .lines()
        .filter(|line| !line.starts_with("//!"))
        .collect::<Vec<_>>()
        .join("\n");
    let source = include_str!("text_refusal_fixture.rs")
        .replace("// REFUSED", &refused)
        .replace(
            "// GENERATED",
            &format!(
                r#"
mod host {{ use super::*; {conversions}
    pub trait ProbeCalls {{
        fn direct(&mut self, input: u32) -> Result<Result<u32, wire::TextFormError>>;
        fn batch(&mut self, input: wire::TextBatch) -> Result<Result<u32, wire::TextFormError>>;
        fn output(&mut self) -> Result<Result<u32, wire::TextFormError>>;
    }}
    impl ProbeCalls for Probe {{ {host} }}
}}
mod guest {{ use super::*; {conversions} pub struct Guest; impl TextProbe for Guest {{ {guest} }} }}
mod guest_exports {{ use super::*; {conversions}
    pub fn health(depth: u32) -> Result<Option<Vec<wire::GuiDraw>>, wire::TextFormError> {{ {health_answer} }}
    pub fn multi(input: wire::TextEvent, depth: u32) -> Result<wire::TextEventOut, wire::TextFormError> {{ {multi_answer} }}
}}
mod proxy {{ use super::*; {conversions}
    pub struct Proxy {{ pub module: std::cell::RefCell<Option<Refused>> }}
    impl Proxy {{
        pub fn health(&self, depth: u32) -> Result<Option<Vec<GuiDraw>>, Refused> {{ let r0 = {health_payload}; Ok({health_lift}) }}
        pub fn multi(&self, p0: &mut TextEvent, depth: u32) -> Result<(), Refused> {{ let {multi_pattern} = {multi_payload}; {prepare} {apply} Ok(()) }}
    }}
}}
"#
            ),
        );
    let dir = root
        .join("target")
        .join(format!("text-refusal-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let src = dir.join("fixture.rs");
    let binary = dir.join("fixture");
    fs::write(&src, source).unwrap();
    let built = Command::new("rustc")
        .args(["--edition=2024", "--test", "-Awarnings"])
        .arg(&src)
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let ran = Command::new(binary).arg("--nocapture").output().unwrap();
    assert!(
        ran.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&ran.stdout),
        String::from_utf8_lossy(&ran.stderr)
    );
    println!("{}", String::from_utf8_lossy(&ran.stdout));
    fs::remove_dir_all(dir).unwrap();
}
