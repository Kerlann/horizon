//! How a point's record crosses to a wasm mod and back, through the one generic `listeners`
//! interface (`wit/points.wit`): nothing here is per point. Each record's [`Record`] impl,
//! generated beside it, lowers its fields to [`Value`]s in order; the mod's guest lifts them into
//! the record ([`Lift`]), runs its listener and lowers them again; the game writes them back.
use std::borrow::Cow;

#[cfg(target_arch = "wasm32")]
pub mod bindings {
    wit_bindgen::generate!({
        path: "wit",
        world: "points",
        runtime_path: "mod_points::__private::wire::rt",
        pub_export_macro: true,
        export_macro_name: "export_listeners",
        default_bindings_module: "mod_points::__private::wire::bindings",
    });
}
#[cfg(target_arch = "wasm32")]
pub use bindings::exports::horizon::mod_points::listeners::{Guest, HostKind, Value};
#[cfg(target_arch = "wasm32")]
pub use wit_bindgen::rt;

#[cfg(all(feature = "wasm-host", not(target_arch = "wasm32")))]
mod bindings {
    wasmtime::component::bindgen!({ path: "wit", world: "points" });
}
#[cfg(all(feature = "wasm-host", not(target_arch = "wasm32")))]
pub use bindings::exports::horizon::mod_points::listeners::{HostKind, Value};

/// A record's fields didn't fit its shape: the mod and the game disagree on the point.
#[derive(Debug, PartialEq, Eq)]
pub struct Mismatch;

impl std::fmt::Display for Mismatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the record's fields don't fit the point's shape")
    }
}

/// A record field's type, as it crosses.
pub trait Wire<'v>: Sized {
    fn lower(&self) -> Value;
    fn lift(value: &'v Value) -> Result<Self, Mismatch>;
}

/// A record field the game takes back as the listener left it: its new value, from the field as it
/// was and what the listener answered.
pub trait WriteBack: Sized {
    fn taken(&self, value: Value) -> Result<Self, Mismatch>;
}

macro_rules! scalar {
    ($case:ident as $wire:ty: $($t:ty),*) => {$(
        impl<'v> Wire<'v> for $t {
            fn lower(&self) -> Value {
                Value::$case(*self as $wire)
            }
            fn lift(value: &'v Value) -> Result<Self, Mismatch> {
                match value {
                    Value::$case(v) => <$t>::try_from(*v).map_err(|_| Mismatch),
                    _ => Err(Mismatch),
                }
            }
        }
        impl WriteBack for $t {
            fn taken(&self, value: Value) -> Result<Self, Mismatch> {
                Self::lift(&value)
            }
        }
    )*};
}
scalar!(S as i64: i8, i16, i32, i64, isize);
scalar!(U as u64: u8, u16, u32, u64, usize);

macro_rules! exact {
    ($case:ident: $($t:ty),*) => {$(
        impl<'v> Wire<'v> for $t {
            fn lower(&self) -> Value {
                Value::$case(*self)
            }
            fn lift(value: &'v Value) -> Result<Self, Mismatch> {
                match value {
                    Value::$case(v) => Ok(*v),
                    _ => Err(Mismatch),
                }
            }
        }
        impl WriteBack for $t {
            fn taken(&self, value: Value) -> Result<Self, Mismatch> {
                Self::lift(&value)
            }
        }
    )*};
}
exact!(B: bool);
exact!(C: char);
exact!(F: f64);

impl<'v> Wire<'v> for f32 {
    fn lower(&self) -> Value {
        Value::F(f64::from(*self))
    }
    fn lift(value: &'v Value) -> Result<Self, Mismatch> {
        // Exact: an `f32` widened to `f64` narrows back to itself.
        f64::lift(value).map(|v| v as f32)
    }
}

impl WriteBack for f32 {
    fn taken(&self, value: Value) -> Result<Self, Mismatch> {
        Self::lift(&value)
    }
}

impl<'v, T: Wire<'v>> Wire<'v> for Option<T> {
    fn lower(&self) -> Value {
        self.as_ref().map_or(Value::None, T::lower)
    }
    fn lift(value: &'v Value) -> Result<Self, Mismatch> {
        match value {
            Value::None => Ok(None),
            value => T::lift(value).map(Some),
        }
    }
}

impl<T: WriteBack + for<'x> Wire<'x>> WriteBack for Option<T> {
    fn taken(&self, value: Value) -> Result<Self, Mismatch> {
        Self::lift(&value)
    }
}

impl<'v> Wire<'v> for Cow<'v, str> {
    fn lower(&self) -> Value {
        Value::Str(self.to_string())
    }
    fn lift(value: &'v Value) -> Result<Self, Mismatch> {
        match value {
            Value::Str(text) => Ok(Cow::Borrowed(text)),
            _ => Err(Mismatch),
        }
    }
}

impl WriteBack for Cow<'_, str> {
    fn taken(&self, value: Value) -> Result<Self, Mismatch> {
        match value {
            // Unchanged text stays the game's own borrow.
            Value::Str(text) if text == **self => Ok(self.clone()),
            Value::Str(text) => Ok(Cow::Owned(text)),
            _ => Err(Mismatch),
        }
    }
}

/// The next field of a record being lifted.
pub fn field<'v, T: Wire<'v>>(fields: &mut std::slice::Iter<'v, Value>) -> Result<T, Mismatch> {
    T::lift(fields.next().ok_or(Mismatch)?)
}

/// `field`'s new value, from the next of the fields a listener left.
pub fn taken<T: WriteBack>(
    field: &T,
    fields: &mut std::vec::IntoIter<Value>,
) -> Result<T, Mismatch> {
    field.taken(fields.next().ok_or(Mismatch)?)
}

/// The end of a record's fields: none may be left over.
pub fn end<T>(fields: &mut impl Iterator<Item = T>) -> Result<(), Mismatch> {
    fields.next().map_or(Ok(()), |_| Err(Mismatch))
}

/// A point's record, as it crosses: its fields in order, and back.
pub trait Record {
    fn lower(&self) -> Vec<Value>;
    /// Takes every field back as the listener left it, or none when they don't fit its shape.
    fn write_back(&mut self, fields: Vec<Value>) -> Result<(), Mismatch>;
}

/// A record from its fields, in the mod: its shape, the field names and types the game checks at
/// load, and the record.
#[cfg(target_arch = "wasm32")]
pub trait Lift<'v>: Sized {
    const SHAPE: &'static str;
    fn lift(fields: &'v [Value]) -> Result<Self, Mismatch>;
}

/// The mod's side: its listeners, the hosts a call gives them, and one place's run.
#[cfg(target_arch = "wasm32")]
mod guest {
    use super::{HostKind, Lift, Record, Value};
    use mod_api::__private::wasm_guest;

    /// The mod's `impl Points`: its hooks, the same value.
    pub fn listener<T: 'static>() -> &'static T {
        let hooks: &'static dyn std::any::Any = wasm_guest::hooks();
        hooks
            .downcast_ref::<T>()
            .expect("a mod's Points and Hooks are one type")
    }

    /// A listener's host, from the one the call names.
    pub trait FromHost {
        fn from_host(host: HostKind) -> Self;
    }

    impl FromHost for &'static dyn mod_api::Host {
        fn from_host(_: HostKind) -> Self {
            wasm_guest::host()
        }
    }

    impl FromHost for mod_api::HostSide<'static> {
        fn from_host(host: HostKind) -> Self {
            match host {
                HostKind::Server => mod_api::HostSide::Server(wasm_guest::server_host()),
                HostKind::Client => mod_api::HostSide::Client(wasm_guest::client_host()),
                HostKind::Read | HostKind::None => {
                    panic!("the game gave a writing point no writing host")
                }
            }
        }
    }

    impl FromHost for &'static mut dyn mod_api::ServerHost {
        fn from_host(_: HostKind) -> Self {
            wasm_guest::server_host()
        }
    }

    impl FromHost for &'static mut dyn mod_api::ClientHost {
        fn from_host(_: HostKind) -> Self {
            wasm_guest::client_host()
        }
    }

    /// A point with no host: its listener takes only its record.
    impl FromHost for () {
        fn from_host(_: HostKind) -> Self {}
    }

    /// Runs one place's listener on the record the game sent, answering its fields after.
    pub fn fire<'v, H: FromHost, R: Lift<'v> + Record>(
        host: HostKind,
        fields: &'v [Value],
        listen: impl FnOnce(H, &mut R),
    ) -> Vec<Value> {
        let mut record =
            R::lift(fields).unwrap_or_else(|_| panic!("the game sent a record of another shape"));
        listen(H::from_host(host), &mut record);
        record.lower()
    }
}
#[cfg(target_arch = "wasm32")]
pub use guest::{FromHost, fire, listener};

/// The game's side: a wasm mod's listeners as one [`Points`](crate::Points), each place
/// forwarded with its record.
#[cfg(all(feature = "wasm-host", not(target_arch = "wasm32")))]
mod host {
    use super::Record;
    use crate::PointId;

    /// The host a point hands its listeners.
    pub enum PointHost<'a> {
        Read(&'a dyn mod_api::Host),
        Side(mod_api::HostSide<'a>),
        Server(&'a mut dyn mod_api::ServerHost),
        Client(&'a mut dyn mod_api::ClientHost),
        /// A point below the game: no `Game`, so no host.
        None,
    }

    /// One mod's listeners, every place forwarded to it with its record.
    pub trait Forward: Send + Sync {
        fn subscribed(&self) -> &'static [Option<PointId>];
        fn forward(&self, place: PointId, host: PointHost<'_>, record: &mut dyn Record);
    }

    /// A [`Forward`] as the mod's `Points`.
    pub struct Forwarded(pub Box<dyn Forward>);
}
#[cfg(all(feature = "wasm-host", not(target_arch = "wasm32")))]
pub use host::{Forward, Forwarded, PointHost};

#[cfg(all(test, feature = "wasm-host", not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::places::SHAPES;

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
        let mut record = crate::HandleChatInputArgs::__new("hi");
        let fields = record.lower();
        assert!(matches!(&fields[..], [Value::Str(text), Value::B(false)] if text == "hi"));
        record
            .write_back(vec![Value::Str("hello".into()), Value::B(true)])
            .unwrap();
        assert_eq!((&*record.message, record.cancel), ("hello", true));
        // Unchanged text stays the game's own borrow.
        let mut kept = crate::HandleChatInputArgs::__new("hi");
        kept.write_back(vec![Value::Str("hi".into()), Value::B(false)])
            .unwrap();
        assert!(matches!(kept.message, Cow::Borrowed("hi")));

        let mut reach = crate::ReachArgs::__new();
        assert!(matches!(&reach.lower()[..], [Value::None]));
        reach.write_back(vec![Value::F(6.0)]).unwrap();
        assert_eq!(reach.result, Some(6.0));
        // A mod answering another shape is refused, and the record is left as it was.
        let mut turn = crate::TurnPlayerArgs::__new(1.0, 2.0);
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
        let shape = crate::places::NAMES
            .iter()
            .position(|name| *name == "slot_candidates")
            .map(|place| crate::places::SHAPES[place])
            .expect("slot_candidates is a point");
        assert!(shape.contains("limit:u64"), "{shape}");
        let record = crate::SlotCandidatesArgs::__new(usize::MAX);
        let fields = record.lower();
        assert!(matches!(&fields[0], Value::U(u64::MAX)));
        // As the mod lifts it, on any target.
        assert_eq!(u64::lift(&fields[0]), Ok(u64::MAX));
    }
}
