use horizon_mod_api_gen::{
    crossing::{Collection, Rule, Scalar, Shape},
    native_schema::All,
};
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
        signature: "enum Choice { Empty, Value { number: u32 } }".into(),
        hash: String::new(),
        cfg: String::new(),
        shape_only: false,
        opaque_reason: None,
        refs: Default::default(),
    };
    let enum_code = horizon_mod_api_gen::row_views::generate(
        &Rule::new(&[internal]),
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
    let code = format!("{code} pub mod choice {{ {enum_code} }} pub mod map {{ {map_code} }}");
    std::fs::write(
        std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("views.rs"),
        code.to_string(),
    )
    .unwrap();
}
