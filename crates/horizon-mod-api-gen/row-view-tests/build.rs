use horizon_mod_api_gen::{
    crossing::{Collection, Rule, Scalar, Shape},
    native_schema::All,
};
struct LastOnly;
impl horizon_mod_api_gen::native_schema::Exposure for LastOnly {
    fn name(&self) -> &str {
        "last"
    }
    fn field(&self, _: &str, case: Option<u32>, flattened: u32) -> bool {
        case == Some(1) && flattened == 2
    }
}
fn main() {
    let shape = Shape::Tuple(vec![
        Shape::Scalar(Scalar::Unsigned("u32")),
        Shape::Option(Box::new(Shape::Scalar(Scalar::String))),
        Shape::Collection(
            Collection::Vec,
            vec![Shape::Scalar(Scalar::Unsigned("u16"))],
        ),
    ]);
    let code =
        horizon_mod_api_gen::row_views::generate(&Rule::new(&[]), &shape, "Fixture", &All).unwrap();
    let internal = horizon_mod_api_gen::internals::Internal {
        path: "horizon_client::Choice".into(),
        kind: "enum",
        signature: "enum Choice { First { first: u32 }, Value { number: u32, last: u16 } }".into(),
        hash: String::new(),
        cfg: String::new(),
        shape_only: false,
        opaque_reason: None,
        refs: Default::default(),
    };
    let hidden = horizon_mod_api_gen::internals::Internal {
        path: "horizon_client::Hidden".into(),
        kind: "struct",
        signature: "struct Hidden { secret: u32 }".into(),
        opaque_reason: Some("private runtime state".into()),
        hash: String::new(),
        cfg: String::new(),
        shape_only: false,
        refs: Default::default(),
    };
    let record = horizon_mod_api_gen::internals::Internal {
        path: "horizon_client::Record".into(),
        kind: "struct",
        signature: "struct Record { hidden: Hidden, visible: u32 }".into(),
        hash: String::new(),
        cfg: String::new(),
        shape_only: false,
        refs: Default::default(),
        opaque_reason: None,
    };
    let opaque_enum = horizon_mod_api_gen::internals::Internal {
        path: "horizon_client::OpaqueChoice".into(),
        kind: "enum",
        signature: "enum OpaqueChoice { A { hidden: Hidden, visible: u32 } }".into(),
        hash: String::new(),
        cfg: String::new(),
        shape_only: false,
        refs: Default::default(),
        opaque_reason: None,
    };
    let rule = Rule::new(&[internal, hidden, record, opaque_enum]);
    let enum_code = horizon_mod_api_gen::row_views::generate(
        &rule,
        &Shape::Named("horizon_client::Choice".into(), 0),
        "ChoiceView",
        &All,
    )
    .unwrap();
    let map = Shape::Collection(
        Collection::BTreeMap,
        vec![
            Shape::Scalar(Scalar::Unsigned("u32")),
            Shape::Scalar(Scalar::String),
        ],
    );
    let map_code =
        horizon_mod_api_gen::row_views::generate(&Rule::new(&[]), &map, "MapView", &All).unwrap();
    let mut graph = horizon_mod_api_gen::native_policy::Graph::default();
    let root = graph.add(&rule, &Shape::Named("horizon_client::Choice".into(), 0));
    let plan =
        horizon_mod_api_gen::exposure::Plan::build(&graph.layouts, None, &[], &[root]).unwrap();
    let key = plan.key(root).unwrap();
    let partial_code = horizon_mod_api_gen::row_views::generate(
        &rule,
        &Shape::Named("horizon_client::Choice".into(), 0),
        "PartialView",
        &LastOnly,
    )
    .unwrap();
    let partial_plan = horizon_mod_api_gen::exposure::Plan::build(
        &graph.layouts,
        Some(root),
        &[horizon_mod_api_gen::exposure::Grant {
            case: Some(1),
            ordinal: 1,
        }],
        &[],
    )
    .unwrap();
    let partial_key = partial_plan.key(root).unwrap();
    let opaque_shape = Shape::Tuple(vec![
        Shape::Opaque(Some("hidden".into())),
        Shape::Collection(Collection::Vec, vec![Shape::Opaque(Some("element".into()))]),
        Shape::Scalar(Scalar::Unsigned("u32")),
    ]);
    let opaque_code =
        horizon_mod_api_gen::row_views::generate(&rule, &opaque_shape, "OpaqueView", &All).unwrap();
    let nominal_code = horizon_mod_api_gen::row_views::generate(
        &rule,
        &Shape::Named("horizon_client::Record".into(), 0),
        "RecordView",
        &All,
    )
    .unwrap();
    let opaque_enum_code = horizon_mod_api_gen::row_views::generate(
        &rule,
        &Shape::Named("horizon_client::OpaqueChoice".into(), 0),
        "OpaqueChoiceView",
        &All,
    )
    .unwrap();
    let code = format!(
        "{code} pub mod choice {{ pub const EXPECTED_ROOT_KEY: u64 = {key}; {enum_code} }} pub mod map {{ {map_code} }} pub mod opaque {{ {opaque_code} }} pub mod nominal {{ {nominal_code} }} pub mod opaque_enum {{ {opaque_enum_code} }} pub mod partial {{ pub const KEY: u64 = {partial_key}; {partial_code} }}"
    );
    std::fs::write(
        std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("views.rs"),
        code.to_string(),
    )
    .unwrap();
}
