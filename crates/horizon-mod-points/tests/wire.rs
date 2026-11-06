//! How records cross to a wasm mod (`mod_points::wire`), on the real points: an integration test, so
//! the generated points compile once for the tests, not again as a unit-test copy of the crate.
#![cfg(feature = "wasm-host")]
use mod_points::__private::wire::{Forward, Mismatch, PointHost, Record, Value, Wire, forwarded};
use mod_points::places::SHAPES;
use mod_points::{PointId, ids};
use std::borrow::Cow;

struct Host;
impl mod_api::Host for Host {}
impl mod_api::ServerHost for Host {}

struct Listener(fn(PointId, PointHost<'_>, &mut dyn Record));
impl Forward for Listener {
    fn subscribed(&self) -> &'static [Option<PointId>] {
        &[
            ids::handle_chat_input,
            ids::reach,
            ids::reach_return,
            ids::turn_player,
            ids::slot_candidates,
        ]
    }

    fn forward(&self, place: PointId, host: PointHost<'_>, record: &mut dyn Record) {
        self.0(place, host, record);
    }
}

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
    let listener = forwarded(Box::new(Listener(|place, host, record| match place {
        PointId::HandleChatInput => {
            assert!(matches!(
                host,
                PointHost::Side(mod_api::HostSide::Server(_))
            ));
            let fields = record.lower();
            assert!(matches!(&fields[..], [Value::Str(text), Value::B(false)] if text == "hi"));
            record
                .write_back(vec![Value::Str("hello".into()), Value::B(true)])
                .unwrap();
        }
        PointId::Reach => {
            assert!(matches!(host, PointHost::Read(_)));
            assert!(matches!(&record.lower()[..], [Value::None]));
            record.write_back(vec![Value::F(6.0)]).unwrap();
        }
        PointId::ReachReturn => {
            assert!(matches!(&record.lower()[..], [Value::F(6.0)]));
            record.write_back(vec![Value::F(7.0)]).unwrap();
        }
        PointId::TurnPlayer => {
            // A mod answering another shape is refused, and the record is left as it was.
            assert_eq!(record.write_back(vec![Value::F(3.0)]), Err(Mismatch));
            assert_eq!(
                record.write_back(vec![
                    Value::F(3.0),
                    Value::B(true),
                    Value::B(true),
                    Value::B(true)
                ]),
                Err(Mismatch)
            );
            assert_eq!(
                record.write_back(vec![Value::F(3.0), Value::Str("4".into()), Value::B(true)]),
                Err(Mismatch)
            );
        }
        _ => panic!("unexpected place {place:?}"),
    })));
    assert_eq!(
        listener.subscribed(),
        &[
            Some(PointId::HandleChatInput),
            Some(PointId::Reach),
            Some(PointId::ReachReturn),
            Some(PointId::TurnPlayer),
            Some(PointId::SlotCandidates),
        ]
    );
    let mut record = mod_points::HandleChatInputArgs::__new("hi");
    listener.handle_chat_input(mod_api::HostSide::Server(&mut Host), &mut record);
    assert_eq!((&*record.message, record.cancel), ("hello", true));

    // Unchanged text stays the game's own borrow.
    let unchanged = forwarded(Box::new(Listener(|_, _, record| {
        record.write_back(record.lower()).unwrap();
    })));
    let mut kept = mod_points::HandleChatInputArgs::__new("hi");
    unchanged.handle_chat_input(mod_api::HostSide::Server(&mut Host), &mut kept);
    assert!(matches!(kept.message, Cow::Borrowed("hi")));

    let mut reach = mod_points::ReachArgs::__new();
    listener.reach(&Host, &mut reach);
    assert_eq!(reach.result, Some(6.0));
    listener.reach_return(&Host, &mut reach);
    assert_eq!(reach.result, Some(7.0));

    let mut turn = mod_points::TurnPlayerArgs::__new(1.0, 2.0);
    listener.turn_player(mod_api::HostSide::Server(&mut Host), &mut turn);
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
    let listener = forwarded(Box::new(Listener(|place, _, record| {
        assert_eq!(place, PointId::SlotCandidates);
        let fields = record.lower();
        assert!(matches!(&fields[0], Value::U(u64::MAX)));
        // As the mod lifts it, on any target.
        assert_eq!(u64::lift(&fields[0]), Ok(u64::MAX));
        record.write_back(fields).unwrap();
    })));
    let mut record = mod_points::SlotCandidatesArgs::__new(usize::MAX);
    listener.slot_candidates(mod_api::HostSide::Server(&mut Host), &mut record);
    assert_eq!((record.limit, record.cancel), (u64::MAX, false));
}
