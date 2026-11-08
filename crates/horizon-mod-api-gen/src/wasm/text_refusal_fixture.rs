// Runtime fixture for production-generated adapters; the wire shape is deliberately small.
extern crate self as wasmtime;
type Result<T, E = Error> = std::result::Result<T, E>;
#[derive(Debug)]
struct Error(Box<dyn std::any::Any>);
impl Error {
    fn downcast<T: 'static>(self) -> Result<T, Self> {
        self.0.downcast::<T>().map(|x| *x).map_err(Self)
    }
}
impl From<Refused> for Error {
    fn from(v: Refused) -> Self {
        Self(Box::new(v))
    }
}
#[derive(Debug, PartialEq)]
pub enum TextFormError {
    TooDeep,
    Malformed,
    NonFinite,
    Unavailable,
    InvalidString,
    NestedNull,
}
impl std::fmt::Display for TextFormError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for TextFormError {}
// REFUSED
#[derive(Clone, Debug, PartialEq)]
pub struct Text(u32);
#[derive(Clone, Debug, PartialEq)]
pub enum GuiDraw {
    Label(Text),
}
#[derive(Debug, PartialEq)]
pub struct TextEvent {
    pub first: u32,
    pub text: Text,
    pub last: u32,
}
fn unsent<T>(
    module: &std::cell::RefCell<Option<Refused>>,
    why: Refused,
    no_answer: impl FnOnce() -> T,
) -> T {
    *module.borrow_mut() = Some(why);
    no_answer()
}
pub struct TextCarrier(u32);
impl TryFrom<&TextCarrier> for u32 {
    type Error = &'static str;
    fn try_from(t: &TextCarrier) -> Result<Self, Self::Error> {
        if t.0 == 800 {
            Err("carrier cannot encode")
        } else {
            Ok(t.0)
        }
    }
}
impl TryFrom<u32> for TextCarrier {
    type Error = &'static str;
    fn try_from(n: u32) -> Result<Self, Self::Error> {
        if n == 900 {
            Err("carrier cannot decode")
        } else {
            Ok(Self(n))
        }
    }
}
impl TryFrom<&Text> for TextCarrier {
    type Error = TextFormError;
    fn try_from(t: &Text) -> Result<Self, Self::Error> {
        match t.0 {
            513 => Err(TextFormError::TooDeep),
            600 => Err(TextFormError::Malformed),
            700 => Err(TextFormError::NonFinite),
            701 => Err(TextFormError::Unavailable),
            702 => Err(TextFormError::InvalidString),
            703 => Err(TextFormError::NestedNull),
            n => Ok(TextCarrier(n)),
        }
    }
}
impl TryFrom<TextCarrier> for Text {
    type Error = TextFormError;
    fn try_from(n: TextCarrier) -> Result<Self, Self::Error> {
        TextCarrier::try_from(&Text(n.0)).map(|c| Text(c.0))
    }
}
pub struct TextBatch {
    pub values: Vec<Option<Text>>,
    pub shape: [u32; 13],
}
trait TextProbe {
    fn direct(&mut self, input: Text) -> Result<u32, TextFormError>;
    fn batch(&mut self, input: TextBatch) -> Result<u32, TextFormError>;
    fn output(&mut self) -> Result<Text, TextFormError>;
}
#[derive(Default)]
struct Probe {
    calls: u32,
    output: u32,
}
impl TextProbe for Probe {
    fn direct(&mut self, input: Text) -> Result<u32, TextFormError> {
        self.calls += 1;
        Ok(input.0)
    }
    fn batch(&mut self, input: TextBatch) -> Result<u32, TextFormError> {
        self.calls += 1;
        Ok(input.values.len() as u32)
    }
    fn output(&mut self) -> Result<Text, TextFormError> {
        self.calls += 1;
        Ok(Text(self.output))
    }
}
fn expect<T>(value: Result<T, Refused>) -> T {
    value.unwrap()
}
fn array<T, const N: usize>(items: Vec<T>) -> Result<[T; N], Refused> {
    items.try_into().map_err(|_| Refused::SHAPE)
}
#[derive(Default)]
struct Keep;
mod horizon {
    pub mod mod_api {
        pub mod types {
            pub type Text = u32;
            pub type TextCarrier = u32;
            #[derive(Debug)]
            pub enum GuiDraw {
                Label(Text),
            }
            pub struct TextEvent {
                pub first: u32,
                pub text: Text,
                pub last: u32,
            }
            pub struct TextEventOut {
                pub first: Option<u32>,
                pub text: Option<Text>,
                pub last: Option<u32>,
            }
            #[derive(Debug, PartialEq)]
            pub enum TextFormError {
                TooDeep,
                Malformed,
                NonFinite,
                Unavailable,
                InvalidString,
                NestedNull,
            }
            pub struct TextBatch {
                pub values: Vec<Option<Text>>,
                pub shape: Vec<u32>,
            }
        }
    }
}
use horizon::mod_api::types as wire;
mod imports {
    use super::*;
    pub fn direct(input: u32) -> Result<u32, wire::TextFormError> {
        assert!(input <= 512);
        Ok(input)
    }
    pub fn batch(input: &wire::TextBatch) -> Result<u32, wire::TextFormError> {
        assert!(input.values.iter().flatten().all(|n| *n <= 512));
        Ok(input.values.len() as u32)
    }
    pub fn output() -> Result<u32, wire::TextFormError> {
        Ok(OUTPUT.with(|v| v.get()))
    }
    thread_local! { pub static OUTPUT: std::cell::Cell<u32> = const { std::cell::Cell::new(513) }; }
}
// GENERATED
#[test]
fn health_hud_refuses_as_typed_data_instead_of_trapping() {
    let proxy = proxy::Proxy {
        module: Default::default(),
    };
    assert_eq!(
        proxy.health(512).unwrap(),
        Some(vec![GuiDraw::Label(Text(512))])
    );
    for (depth, error) in [
        (513, TextFormError::TooDeep),
        (600, TextFormError::Malformed),
        (700, TextFormError::NonFinite),
        (701, TextFormError::Unavailable),
        (702, TextFormError::InvalidString),
        (703, TextFormError::NestedNull),
        (800, TextFormError::Malformed),
    ] {
        assert_eq!(proxy.health(depth).unwrap(), None);
        assert!(matches!(proxy.module.take(), Some(Refused::Text(why)) if why == error));
    }
}
#[test]
fn refused_event_writeback_keeps_every_field_unchanged() {
    let proxy = proxy::Proxy {
        module: Default::default(),
    };
    let mut event = TextEvent {
        first: 1,
        text: Text(2),
        last: 3,
    };
    for n in [513, 800, 900] {
        if n == 900 {
            assert!(matches!(
                proxy.multi(&mut event, n),
                Err(Refused::Text(TextFormError::Malformed))
            ));
        } else {
            proxy.multi(&mut event, n).unwrap();
        }
        assert_eq!(
            event,
            TextEvent {
                first: 1,
                text: Text(2),
                last: 3
            }
        );
        if n == 900 {
            assert!(proxy.module.take().is_none());
        } else {
            assert!(
                matches!(proxy.module.take(), Some(Refused::Text(why)) if why == if n == 513 { TextFormError::TooDeep } else { TextFormError::Malformed })
            );
        }
    }
    proxy.multi(&mut event, 512).unwrap();
    assert_eq!(
        event,
        TextEvent {
            first: 10,
            text: Text(512),
            last: 20
        }
    );
    assert!(proxy.module.take().is_none());
}
#[test]
fn guest_refuses_before_import_and_preserves_variants() {
    let mut g = guest::Guest;
    assert_eq!(g.direct(Text(512)), Ok(512));
    for (n, why) in [
        (513, TextFormError::TooDeep),
        (600, TextFormError::Malformed),
        (700, TextFormError::NonFinite),
        (701, TextFormError::Unavailable),
        (702, TextFormError::InvalidString),
        (703, TextFormError::NestedNull),
        (800, TextFormError::Malformed),
    ] {
        assert_eq!(g.direct(Text(n)), Err(why));
    }
    assert_eq!(
        g.batch(TextBatch {
            values: vec![None, Some(Text(513))],
            shape: [0; 13],
        }),
        Err(TextFormError::TooDeep)
    );
    assert!(matches!(g.output(), Err(TextFormError::TooDeep)));
    imports::OUTPUT.with(|v| v.set(900));
    assert_eq!(g.output(), Err(TextFormError::Malformed));
}
#[test]
fn host_refuses_before_write_and_preserves_variants() {
    let mut p = Probe::default();
    assert_eq!(host::ProbeCalls::direct(&mut p, 512).unwrap(), Ok(512));
    for (n, why) in [
        (513, wire::TextFormError::TooDeep),
        (600, wire::TextFormError::Malformed),
        (700, wire::TextFormError::NonFinite),
        (701, wire::TextFormError::Unavailable),
        (702, wire::TextFormError::InvalidString),
        (703, wire::TextFormError::NestedNull),
        (900, wire::TextFormError::Malformed),
    ] {
        assert_eq!(host::ProbeCalls::direct(&mut p, n).unwrap(), Err(why));
    }
    assert_eq!(
        host::ProbeCalls::batch(
            &mut p,
            wire::TextBatch {
                values: vec![Some(513)],
                shape: vec![0; 13],
            }
        )
        .unwrap(),
        Err(wire::TextFormError::TooDeep)
    );
    assert_eq!(p.calls, 1);
    for (n, why) in [
        (513, wire::TextFormError::TooDeep),
        (800, wire::TextFormError::Malformed),
    ] {
        p.output = n;
        assert_eq!(host::ProbeCalls::output(&mut p).unwrap(), Err(why));
    }
}
#[test]
fn unrelated_transport_errors_stay_transport_errors() {
    let mut p = Probe::default();
    let why = host::ProbeCalls::batch(
        &mut p,
        wire::TextBatch {
            values: vec![Some(512)],
            shape: vec![0; 12],
        },
    )
    .unwrap_err();
    assert!(matches!(
        why.downcast::<Refused>().unwrap(),
        Refused::Transport(_)
    ));
    assert_eq!(p.calls, 0);
    assert!(matches!(Refused::because("wire"), Refused::Transport(_)));
}
