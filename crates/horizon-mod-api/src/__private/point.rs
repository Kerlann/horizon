//! Injection points (#589): slots compiled into the game at the head of a `#[point]` function,
//! filled once at load with the listeners mods attach. An empty slot costs one atomic load and a
//! branch; the listeners run out of line. The game declares the slots (`#[point]`) and the
//! launcher's generated `main` fills them by name; a mod never names this module.
use std::any::Any;
use std::sync::OnceLock;

/// Where in its function a slot's listeners run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Place {
    /// Before the body: a listener may change the arguments, or stop the function.
    Head,
    /// After the body: a listener sees the arguments and may change the value it returns.
    Return,
}

/// One compiled-in slot. `F` is its listener's function type, fixed by the function's signature.
// exhaustive: built only by `Point::new`, its fields private.
pub struct Point<F: 'static> {
    target: &'static str,
    place: Place,
    listeners: OnceLock<Box<[F]>>,
}

impl<F: 'static> Point<F> {
    /// An empty slot at `place` in the function `target` (an injection's `target` for it).
    pub const fn new(target: &'static str, place: Place) -> Self {
        Self {
            target,
            place,
            listeners: OnceLock::new(),
        }
    }

    /// The attached listeners, or `None` while nothing listens (the hot path).
    #[inline(always)]
    pub fn get(&self) -> Option<&[F]> {
        self.listeners.get().map(|listeners| &**listeners)
    }
}

/// A point the launcher's generated `main` fills by its target, whatever its listener type.
pub trait Attach {
    /// The point's function, as an injection's `target` names it.
    fn target(&self) -> &'static str;
    /// Where in the function its listeners run.
    fn place(&self) -> Place;
    /// Fills the point once; `listeners` is a `Vec<F>` of this point's `F`, in dispatch order.
    fn attach(&self, listeners: Box<dyn Any>) -> Result<(), AttachError>;
}

/// Why a listener list was refused.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum AttachError {
    /// The listeners' type is not this point's: the function's signature changed.
    Signature,
    /// The point was already filled.
    Filled,
}

impl<F: Send + Sync + 'static> Attach for Point<F> {
    fn target(&self) -> &'static str {
        self.target
    }

    fn place(&self) -> Place {
        self.place
    }

    fn attach(&self, listeners: Box<dyn Any>) -> Result<(), AttachError> {
        let listeners = listeners
            .downcast::<Vec<F>>()
            .map_err(|_| AttachError::Signature)?;
        self.listeners
            .set(listeners.into_boxed_slice())
            .map_err(|_| AttachError::Filled)
    }
}

/// Whether `list` holds `name`, at compile time: the launcher's generated `main` checks each
/// point injection's method against the mod's `__HORIZON_POINTS` with it.
pub const fn lists(list: &[&str], name: &str) -> bool {
    let mut i = 0;
    while i < list.len() {
        let (a, b) = (list[i].as_bytes(), name.as_bytes());
        if a.len() == b.len() {
            let mut j = 0;
            while j < a.len() && a[j] == b[j] {
                j += 1;
            }
            if j == a.len() {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// Calls `call` with each attached listener, out of the pointed function's own code.
#[cold]
#[inline(never)]
pub fn fire<F>(listeners: &[F], mut call: impl FnMut(&F)) {
    for listener in listeners {
        call(listener);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type Listener = fn(&mut u32);

    #[test]
    fn a_point_is_filled_once_with_its_own_listener_type() {
        static POINT: Point<Listener> =
            Point::new("horizon_client::game::Game<'_>::tick", Place::Head);
        assert!(POINT.get().is_none());
        assert_eq!(
            POINT.attach(Box::new(vec![|n: &mut u32| *n += 1])),
            Err(AttachError::Signature)
        );
        let listeners: Vec<Listener> = vec![|n| *n += 1, |n| *n *= 10];
        POINT.attach(Box::new(listeners)).unwrap();
        let mut n = 1;
        fire(POINT.get().unwrap(), |listener| listener(&mut n));
        assert_eq!(n, 20);
        assert_eq!(
            POINT.attach(Box::new(Vec::<Listener>::new())),
            Err(AttachError::Filled)
        );
    }

    #[test]
    fn lists_finds_a_name_at_compile_time() {
        const LIST: &[&str] = &["tick", "tick_return"];
        const _: () = assert!(lists(LIST, "tick_return"));
        assert!(!lists(LIST, "tic") && !lists(LIST, "ticks") && !lists(&[], "tick"));
    }
}
