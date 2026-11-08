mod model {
    pub struct Unknown;
    pub struct Game {
        pub hidden: Unknown,
        pub later: Nested,
        pub other: u32,
    }
    pub struct Nested {
        pub value: u16,
        pub back: Option<Box<Game>>,
    }
}
include!(concat!(env!("OUT_DIR"), "/reader.rs"));
#[test]
fn two_policies() {
    use mod_points::{
        exposure::{Grant, Plan},
        native_read::NativeNode,
    };
    let game = model::Game {
        hidden: model::Unknown,
        later: model::Nested {
            value: 42,
            back: Some(Box::new(model::Game {
                hidden: model::Unknown,
                later: model::Nested {
                    value: 7,
                    back: None,
                },
                other: 11,
            })),
        },
        other: 99,
    };
    let metadata = layouts();
    let a = Plan::build(
        &metadata,
        Some(ROOT),
        &[Grant {
            case: None,
            ordinal: 1,
        }],
        &[],
    )
    .unwrap();
    let b = Plan::build(
        &metadata,
        Some(ROOT),
        &[Grant {
            case: None,
            ordinal: 2,
        }],
        &[],
    )
    .unwrap();
    let first = reader(&game, &a);
    let second = reader(&game, &b);
    assert_eq!(first.key(), a.key(ROOT).unwrap());
    assert_ne!(first.key(), second.key());
    let mut calls = 0;
    first
        .child(0, &mut |_, _, _| {
            calls += 1;
            Ok(())
        })
        .unwrap();
    assert_eq!(calls, 0);
    first
        .child(2, &mut |_, _, _| {
            calls += 1;
            Ok(())
        })
        .unwrap();
    assert_eq!(calls, 0);
    first
        .child(1, &mut |_, ordinal, nested| {
            assert_eq!(ordinal, 1);
            nested.child(0, &mut |_, _, _| {
                calls += 1;
                Ok(())
            })
        })
        .unwrap();
    assert_eq!(calls, 1);
    second
        .child(1, &mut |_, _, _| {
            calls += 1;
            Ok(())
        })
        .unwrap();
    assert_eq!(calls, 1);
    let mut back_calls = 0;
    first
        .child(1, &mut |_, _, nested| {
            nested.child(1, &mut |_, _, option| {
                option.child(0, &mut |_, _, payload| {
                    payload.child(0, &mut |_, _, back| {
                        back.child(0, &mut |_, _, _| {
                            back_calls += 1;
                            Ok(())
                        })?;
                        back.child(2, &mut |_, _, _| {
                            back_calls += 1;
                            Ok(())
                        })?;
                        back.child(1, &mut |_, _, _| {
                            back_calls += 10;
                            Ok(())
                        })
                    })
                })
            })
        })
        .unwrap();
    assert_eq!(back_calls, 10);
    let mut paths = mod_points::locators::Locators::default();
    let root = paths
        .intern(None, mod_points::locators::Step::Root(0))
        .unwrap();
    let hidden = paths
        .intern(Some(root), mod_points::locators::Step::Field(0))
        .unwrap();
    assert_eq!(
        mod_points::native_read::read(
            &first,
            root,
            &mut paths,
            mod_api::RowWindow {
                node: hidden,
                start: 0,
                rows: 1,
                depth: 0
            }
        ),
        Err(mod_api::RowError::Bounds)
    );
    assert!(
        Plan::build(
            &metadata,
            Some(ROOT),
            &[Grant {
                case: None,
                ordinal: 0
            }],
            &[]
        )
        .is_err()
    );
}
