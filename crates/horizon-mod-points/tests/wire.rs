//! How records cross to a wasm mod (`mod_points::wire`), on the real points: an integration test, so
//! the generated points compile once for the tests, not again as a unit-test copy of the crate.
#![cfg(feature = "wasm-host")]
use mod_points::__private::wire::{Mismatch, Record, Value, Wire};
use mod_points::places::SHAPES;
use std::borrow::Cow;

/// The field types a record's shape may name: each one [`Wire`] carries with the same range in
/// the game and in a wasm32 mod. Native-width integers cross as `u64`/`i64`, and `i128` isn't
/// an API scalar, so neither may appear.
const CARRIED: &[&str] = &[
    "bool", "char", "str", "f32", "f64", "i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64",
];

#[test]
fn every_point_of_the_game_crosses_to_a_wasm_mod() {
    for shape in SHAPES.iter() {
        let (host, fields) = shape.split_once('|').expect("a shape names its host");
        assert!(
            ["read", "side", "server", "client", "none"].contains(&host),
            "a point's listener gets no known host in ({shape})"
        );
        for field in fields.split(',').filter(|field| !field.is_empty()) {
            let (name, ty) = field.split_once(':').expect("a field is name:type");
            let ty = ty
                .strip_prefix("Option<")
                .and_then(|ty| ty.strip_suffix('>'))
                .unwrap_or(ty);
            assert!(
                !name.is_empty() && CARRIED.contains(&ty),
                "a point's record field {field} in ({shape}) doesn't cross"
            );
        }
    }
}

#[test]
fn a_record_crosses_and_takes_back_what_a_listener_changed() {
    let mut record = mod_points::HandleChatInputArgs::__new("hi");
    let fields = record.lower();
    assert!(matches!(&fields[..], [Value::Str(text), Value::B(false)] if text == "hi"));
    record
        .write_back(vec![Value::Str("hello".into()), Value::B(true)])
        .unwrap();
    assert_eq!((&*record.message, record.cancel), ("hello", true));
    // Unchanged text stays the game's own borrow.
    let mut kept = mod_points::HandleChatInputArgs::__new("hi");
    kept.write_back(vec![Value::Str("hi".into()), Value::B(false)])
        .unwrap();
    assert!(matches!(kept.message, Cow::Borrowed("hi")));

    let mut reach = mod_points::ReachArgs::__new();
    assert!(matches!(&reach.lower()[..], [Value::None]));
    reach.write_back(vec![Value::F(6.0)]).unwrap();
    assert_eq!(reach.result, Some(6.0));
    // A mod answering another shape is refused, and the record is left as it was.
    let mut turn = mod_points::TurnPlayerArgs::__new(1.0, 2.0);
    assert_eq!(turn.write_back(vec![Value::F(3.0)]), Err(Mismatch));
    assert_eq!(
        turn.write_back(vec![
            Value::F(3.0),
            Value::B(true),
            Value::B(true),
            Value::B(true)
        ]),
        Err(Mismatch)
    );
    assert_eq!(
        turn.write_back(vec![Value::F(3.0), Value::Str("4".into()), Value::B(true)]),
        Err(Mismatch)
    );
    assert_eq!((turn.xo, turn.yo, turn.cancel), (1.0, 2.0, false));
}

#[test]
fn a_native_width_sentinel_survives_a_listener_that_leaves_it() {
    // `slot_candidates` is called with `usize::MAX` for an unbounded slot source.
    let shape = mod_points::places::NAMES
        .iter()
        .position(|name| *name == "slot_candidates")
        .map(|place| mod_points::places::SHAPES[place])
        .expect("slot_candidates is a point");
    assert!(shape.contains("limit:u64"), "{shape}");
    let record = mod_points::SlotCandidatesArgs::__new(usize::MAX);
    let fields = record.lower();
    assert!(matches!(&fields[0], Value::U(u64::MAX)));
    // As the mod lifts it, on any target.
    assert_eq!(u64::lift(&fields[0]), Ok(u64::MAX));
}
