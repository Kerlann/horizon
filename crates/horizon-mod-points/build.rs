fn main() {
    let root =
        std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("../..");
    let out_dir = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    horizon_mod_api_gen::generate_points(&root, &out_dir).expect("generating the points");
    receiver_fixture(&out_dir);
    // Exercise the actual catalog signature and Rust body, not a handwritten nominal DTO.
    let source = root.join("crates/horizon-client/src/game/network/server_links.rs");
    println!("cargo:rerun-if-changed={}", source.display());
    let parsed = syn::parse_file(&std::fs::read_to_string(source).unwrap()).unwrap();
    let mut function = parsed
        .items
        .iter()
        .find_map(|item| match item {
            syn::Item::Fn(function) if function.sig.ident == "link_buttons" => {
                Some(function.clone())
            }
            _ => None,
        })
        .unwrap();
    let attribute = quote::quote!(name = "game_server_links_link_buttons", raw, free);
    let shape = point_shape::shape(attribute.clone(), &function.sig).unwrap();
    let rule = horizon_mod_api_gen::crossing::Rule::new(&[]);
    let plan = horizon_mod_api_gen::native_raw::generate(
        &rule,
        "horizon_client::game::server_links",
        &function.sig,
        &shape,
    )
    .unwrap()
    .attribute();
    let raw_types = shape.raw_items(&function.sig, None, None);
    function.attrs.clear();
    let constants = parsed.items.iter().filter(|item| matches!(item, syn::Item::Const(item) if ["LINK_WIDTH", "ROW", "SCROLL_RESERVE"].iter().any(|name| item.ident == *name)));
    std::fs::write(
        out_dir.join("native_raw_link_buttons.rs"),
        quote::quote! {
            #(#constants)*
            #raw_types
            #[horizon_mod_api_macros::point(#attribute)]
            #plan
            #function
        }
        .to_string(),
    )
    .unwrap();
    let fixture = horizon_mod_api_gen::internals::Internal {
        path: "horizon_client::Choice".into(),
        kind: "enum",
        cfg: String::new(),
        signature: "pub enum Choice { Empty, One(u32), Named { value: String } }".into(),
        hash: String::new(),
        shape_only: false,
        opaque_reason: None,
        refs: Default::default(),
    };
    let pair = horizon_mod_api_gen::internals::Internal {
        path: "horizon_client::Pair".into(),
        kind: "struct",
        cfg: String::new(),
        signature: "pub struct Pair { pub a: u32, pub b: u32 }".into(),
        hash: String::new(),
        shape_only: false,
        opaque_reason: None,
        refs: Default::default(),
    };
    let later = horizon_mod_api_gen::internals::Internal {
        path: "horizon_client::Later".into(),
        kind: "enum",
        cfg: String::new(),
        signature: "pub enum Later { Both { a: u32, b: u32 } }".into(),
        hash: String::new(),
        shape_only: false,
        opaque_reason: None,
        refs: Default::default(),
    };
    let leaf = horizon_mod_api_gen::internals::Internal {
        path: "horizon_client::Leaf".into(),
        kind: "struct",
        cfg: String::new(),
        signature: "pub struct Leaf { pub n: u32 }".into(),
        hash: String::new(),
        shape_only: false,
        opaque_reason: None,
        refs: Default::default(),
    };
    let holder = horizon_mod_api_gen::internals::Internal {
        path: "horizon_client::Holder".into(),
        kind: "struct",
        cfg: String::new(),
        signature: "pub struct Holder { pub child: Box<Leaf>, pub items: &'static [u32], pub shared: &'static Leaf }".into(),
        hash: String::new(),
        shape_only: false,
        opaque_reason: None,
        refs: Default::default(),
    };
    let rule = horizon_mod_api_gen::crossing::Rule::new(&[fixture, pair, later, leaf, holder]);
    for ty in [
        "horizon_client::Pair",
        "horizon_client::Later",
        "horizon_client::Holder",
        "Box<Vec<u32>>",
        "&'static [u32]",
        "Vec<u32>",
        "Option<String>",
        "(u32, String)",
        "std::collections::BTreeMap<u32, String>",
        "horizon_client::Choice",
    ] {
        let exposure: &dyn horizon_mod_api_gen::native_schema::Exposure =
            if ty.ends_with("Pair") || ty.ends_with("Later") {
                &OnlyB
            } else {
                &horizon_mod_api_gen::native_schema::All
            };
        let generated = horizon_mod_api_gen::native_window::generate(
            &rule,
            "horizon_client",
            None,
            &syn::parse_str(ty).unwrap(),
            quote::quote!(value),
            exposure,
        )
        .unwrap();
        let name = match ty {
            "Vec<u32>" => "vec",
            "Option<String>" => "option",
            "(u32, String)" => "tuple",
            "horizon_client::Choice" => "choice",
            "horizon_client::Pair" => "pair",
            "horizon_client::Holder" => "holder",
            "Box<Vec<u32>>" => "boxed",
            "&'static [u32]" => "slice",
            "horizon_client::Later" => "later",
            _ => "map",
        };
        std::fs::write(
            out_dir.join(format!("native_window_{name}.rs")),
            generated.to_string(),
        )
        .unwrap();
    }
}

/// Exposes only the second declared field of the sparse fixtures.
struct OnlyB;
impl horizon_mod_api_gen::native_schema::Exposure for OnlyB {
    fn name(&self) -> &str {
        "only-b"
    }
    fn field(&self, _: &str, _: Option<u32>, field: u32) -> bool {
        field == 1
    }
}

/// One stand-in layout of a policy fixture, written as its declaration.
fn entry(path: &str, signature: &str) -> horizon_mod_api_gen::internals::Internal {
    horizon_mod_api_gen::internals::Internal {
        path: path.into(),
        kind: if signature.contains("trait ") {
            "trait"
        } else if signature.contains("enum ") {
            "enum"
        } else {
            "struct"
        },
        cfg: String::new(),
        signature: signature.into(),
        hash: String::new(),
        shape_only: false,
        opaque_reason: None,
        refs: Default::default(),
    }
}

/// A real catalog point, on stand-in layouts, generated through the whole-catalog graph,
/// registration policy and once-per-source helpers. `granted` names the receiver fields the
/// fixture's manifest declares; the test file includes the output beside its stand-in types.
struct PolicyFixture<'a> {
    file: &'a str,
    entries: Vec<horizon_mod_api_gen::internals::Internal>,
    module: &'a str,
    /// The receiver's type, as written in the signature; none for a free function.
    owner: Option<&'a str>,
    /// The path the point's `impl` names in the test file.
    impl_path: &'a str,
    signature: &'a str,
    name: &'a str,
    /// Every field of the receiver, in declaration order, and the declared subset.
    fields: &'a [&'a str],
    body: &'a str,
}

fn policy_fixture(out_dir: &std::path::Path, fixture: PolicyFixture<'_>) {
    use horizon_mod_api_gen::{
        crossing::Rule, native_patch, native_policy::Graph, native_raw, native_window,
    };
    let rule = Rule::new(&fixture.entries);
    let module = fixture.module;
    let owner: Option<syn::Type> = fixture.owner.map(|owner| syn::parse_str(owner).unwrap());
    let owner = owner.as_ref();
    let signature: syn::Signature = syn::parse_str(fixture.signature).unwrap();
    let name = fixture.name;
    let attribute = if owner.is_some() {
        quote::quote!(name = #name, raw)
    } else {
        quote::quote!(name = #name, raw, free)
    };
    let shape = point_shape::shape(attribute.clone(), &signature).unwrap();
    let mut graph = Graph::default();
    let call = native_raw::register(&rule, module, owner, &signature, &shape, &mut graph).unwrap();
    let plan = native_raw::generate_runtime(&rule, module, owner, &signature, &shape, &graph, call)
        .unwrap()
        .attribute();
    let receiver = match owner {
        Some(owner) => {
            let id = graph.id(&rule.shape(module, None, owner)).unwrap();
            quote::quote!(Some(#id))
        }
        None => quote::quote!(None),
    };
    let layouts = graph.tokens();
    let every: Vec<bool> = graph
        .layouts
        .iter()
        .map(|layout| layout.opaque.is_none() && rule.named(&layout.name).is_some())
        .collect();
    let windows = native_window::helpers(&rule, &graph, &every).unwrap();
    let patches = native_patch::helpers(&rule, &graph, &every).unwrap();
    let (head, tail) = (
        call.head,
        call.tail
            .map_or(quote::quote!(None), |t| quote::quote!(Some(#t))),
    );
    let raw_types = shape.raw_items(&signature, owner, None);
    let fields = fixture.fields;
    let prefix = format!("{}::{}", module, fixture.owner.unwrap_or_default());
    let impl_path: syn::Type = syn::parse_str(if owner.is_some() {
        fixture.impl_path
    } else {
        "()"
    })
    .unwrap();
    let body: syn::Block = syn::parse_str(fixture.body).unwrap();
    let method = &signature;
    let wrapped = if owner.is_some() {
        quote::quote! {
            impl #impl_path {
                #[horizon_mod_api_macros::point(#attribute)]
                #plan
                #method #body
            }
        }
    } else {
        quote::quote! {
            #[horizon_mod_api_macros::point(#attribute)]
            #plan
            #method #body
        }
    };
    std::fs::write(
        out_dir.join(fixture.file),
        quote::quote! {
            pub mod __horizon_points { #windows #patches }
            fn catalog() -> mod_points::endpoint_policy::SchemaCatalog {
                use mod_points::endpoint_policy::{InternalField, PointSpec, SchemaCatalog};
                SchemaCatalog {
                    layouts: #layouts,
                    points: vec![PointSpec { head_call: #head, tail_call: #tail, receiver: #receiver }],
                    fields: <[&str]>::iter(&[#(#fields),*]).map(|&field| InternalField {
                        path: format!("{}::{field}", #prefix),
                        kind: "field".into(),
                        hash: field.into(),
                        receiver: #receiver.unwrap(),
                        field: field.into(),
                    }).collect(),
                }
            }
            const HEAD_CALL: usize = #head;
            #raw_types
            #wrapped
        }
        .to_string(),
    )
    .unwrap();
}

fn receiver_fixture(out_dir: &std::path::Path) {
    let loading = "pub struct Loading { pub hidden: u32, pub size: [u32; 2] }";
    policy_fixture(
        out_dir,
        PolicyFixture {
            file: "native_raw_receiver.rs",
            entries: vec![entry("horizon_client::model::Loading", loading)],
            module: "horizon_client::model",
            owner: Some("Loading"),
            impl_path: "model::Loading",
            signature: "fn menu_size(&self) -> [u32; 2]",
            name: "game_startup_menu_loading_menu_size",
            fields: &["hidden", "size"],
            body: "{ self.size }",
        },
    );
    // UnitResult: the head carries the bool cancel, the return phase the `Result` slot.
    policy_fixture(
        out_dir,
        PolicyFixture {
            file: "native_raw_unit_result.rs",
            entries: vec![entry(
                "horizon_client::split::Split",
                "pub struct Split { pub seats: u32 }",
            )],
            module: "horizon_client::split",
            owner: Some("Split"),
            impl_path: "split::Split",
            signature: "fn can_join(&self, name: &str) -> Result<(), String>",
            name: "game_split_players_split_can_join",
            fields: &["seats"],
            body: "{ if name == \"ok\" { Ok(()) } else { Err(\"full\".to_owned()) } }",
        },
    );
    // A plain `Result` return with a projectable Ok payload and a `Box<dyn Error>` Err.
    policy_fixture(
        out_dir,
        PolicyFixture {
            file: "native_raw_result.rs",
            entries: vec![entry(
                "horizon_client::split::Split",
                "pub struct Split { pub seats: u32 }",
            )],
            module: "horizon_client::split",
            owner: Some("Split"),
            impl_path: "split::Split",
            signature: "fn profile_names(&self) -> Result<u32, Box<dyn std::error::Error>>",
            name: "game_split_players_split_profile_names",
            fields: &["seats"],
            body: "{ if self.seats > 0 { Ok(self.seats) } else { Err(\"no seats\".into()) } }",
        },
    );
    // Borrowed returns: a shared `&str` (read-only) and a `&mut` struct (pointee writable).
    policy_fixture(
        out_dir,
        PolicyFixture {
            file: "native_raw_borrowed_shared.rs",
            entries: vec![entry(
                "horizon_client::jigsaw::JigsawEditor",
                "pub struct JigsawEditor { pub count: u32 }",
            )],
            module: "horizon_client::jigsaw",
            owner: Some("JigsawEditor"),
            impl_path: "jigsaw::JigsawEditor",
            signature: "fn value(&self, i: usize) -> &str",
            name: "game_jigsaw_edit_jigsaw_editor_value",
            fields: &[],
            body: "{ [\"zero\", \"one\"][i] }",
        },
    );
    policy_fixture(
        out_dir,
        PolicyFixture {
            file: "native_raw_borrowed_mut.rs",
            entries: vec![
                entry(
                    "horizon_client::maps::MapData",
                    "pub struct MapData { pub holders: Vec<Holder> }",
                ),
                entry(
                    "horizon_client::maps::Holder",
                    "pub struct Holder { pub n: u32, pub label: String }",
                ),
            ],
            module: "horizon_client::maps",
            owner: Some("MapData"),
            impl_path: "maps::MapData",
            signature: "fn holder(&mut self, who: u32) -> &mut Holder",
            name: "game_maps_map_data_holder",
            fields: &[],
            body: "{ &mut self.holders[who as usize] }",
        },
    );
    // Argument classes the shape marks `unclassified` and `callback`: opaque leaves.
    policy_fixture(
        out_dir,
        PolicyFixture {
            file: "native_raw_unclassified.rs",
            entries: vec![
                entry(
                    "horizon_client::startup_menu::Loading",
                    "pub struct Loading { pub actions: u32 }",
                ),
                entry(
                    "horizon_client::startup_menu::Action",
                    "pub enum Action { Quit, Options }",
                ),
                // The real catalog's trait and the shape of winit's `ControlFlow`.
                entry(
                    "horizon_client::startup_menu::EventLoopCtl",
                    "trait EventLoopCtl { fn exit(&self); fn set_control_flow(&self, flow: ControlFlow); fn bump(&self, n: &mut u32) -> bool; }",
                ),
                entry(
                    "horizon_client::startup_menu::ControlFlow",
                    "pub enum ControlFlow { Poll, Wait, WaitUntil(std::time::Instant) }",
                ),
            ],
            module: "horizon_client::startup_menu",
            owner: Some("Loading"),
            impl_path: "startup_menu::Loading",
            signature: "fn menu_action(&mut self, event_loop: &dyn EventLoopCtl, action: Action)",
            name: "game_startup_menu_loading_menu_action",
            fields: &["actions"],
            body: "{ match action { Action::Quit => event_loop.exit(), Action::Options => self.actions += 1 } }",
        },
    );
    // Callback arguments: five that project (`&dyn Fn`, `&mut dyn FnMut`, scalar, string and
    // struct arguments and results) and one whose `&Path` argument has no Wire codec.
    policy_fixture(
        out_dir,
        PolicyFixture {
            file: "native_raw_callback.rs",
            entries: vec![entry(
                "horizon_client::test_environments::BlockPos",
                "pub struct BlockPos { pub x: i32, pub y: i32, pub z: i32 }",
            )],
            module: "horizon_client::test_environments",
            owner: None,
            impl_path: "",
            signature: "fn parse(depth: u32, load: &dyn Fn(&str) -> Option<u32>, wanted: &dyn Fn(&str) -> bool, rain: &dyn Fn(BlockPos) -> bool, emit: &mut dyn FnMut(u32) -> u32, publish: &mut dyn FnMut(u32, &std::path::Path)) -> Option<u32>",
            name: "game_test_environments_parse",
            fields: &[],
            body: "{ let n = load(\"minecraft:overworld\")?; let w = wanted(\"w\") as u32; let r = rain(BlockPos { x: depth as i32, y: 64, z: -3 }) as u32; let e = emit(n); publish(1, std::path::Path::new(\"p\")); Some(n + w * 1000 + r * 10000 + e * 100000) }",
        },
    );
    // A callback taking the call's own receiver (`fn(&mut Self, ..)`) and one returning JSON.
    policy_fixture(
        out_dir,
        PolicyFixture {
            file: "native_raw_bound.rs",
            entries: vec![
                entry(
                    "horizon_client::startup_menu::Loading",
                    "pub struct Loading { pub actions: u32 }",
                ),
                entry(
                    "horizon_client::startup_menu::Action",
                    "pub enum Action { Quit, Options }",
                ),
            ],
            module: "horizon_client::startup_menu",
            owner: Some("Loading"),
            impl_path: "startup_menu::Loading",
            signature: "fn menu_action(&mut self, fire: fn(&mut Self, u32), data: &dyn Fn(&str) -> Option<serde_json::Value>, action: Action)",
            name: "game_startup_menu_loading_menu_action",
            fields: &["actions"],
            body: "{ let _ = action; fire(self, 7); let _ = data(\"k\"); }",
        },
    );
}
