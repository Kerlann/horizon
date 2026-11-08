use horizon_mod_api_gen::{
    crossing::{Rule, Scalar, Shape},
    native_callable::codecs,
};

#[test]
fn callable_codecs_use_argument_tuple_and_refuse_non_owned_shapes() {
    let rule = Rule::new(&[]);
    let generated = codecs(
        &rule,
        &[
            Shape::Scalar(Scalar::Unsigned("u32")),
            Shape::Scalar(Scalar::Bool),
        ],
        &Shape::Scalar(Scalar::Unsigned("u32")),
    )
    .unwrap();
    let _: syn::Expr = syn::parse2(generated.decode).unwrap();
    let _: syn::Expr = syn::parse2(generated.encode).unwrap();
    assert!(
        codecs(
            &rule,
            &[Shape::Ref {
                mutable: false,
                to: Box::new(Shape::Unit)
            }],
            &Shape::Unit
        )
        .is_err()
    );
}
