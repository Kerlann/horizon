//! Injection points (prototype): slots compiled into the game at function boundaries, filled once
//! at load with the listeners that mods attach. An empty slot costs one atomic load and a branch.
use std::any::Any;
use std::sync::OnceLock;

/// One compiled-in point. `F` is the listener's function type, fixed by the point's signature.
pub struct Point<F: 'static> {
    name: &'static str,
    listeners: OnceLock<Box<[F]>>,
}

impl<F: 'static> Point<F> {
    /// An empty point named by its target (the horizon-mixins function id).
    pub const fn new(name: &'static str) -> Self {
        Self {
            name,
            listeners: OnceLock::new(),
        }
    }

    /// The attached listeners, or `None` while nothing listens (the hot path).
    #[inline(always)]
    pub fn get(&self) -> Option<&[F]> {
        self.listeners.get().map(|listeners| &**listeners)
    }
}

/// A point the loader can fill by name, whatever its listener type.
pub trait Attach {
    /// The point's target id.
    fn name(&self) -> &'static str;
    /// Fill the point once; `listeners` must be a `Vec<F>` of this point's `F`.
    fn attach(&self, listeners: Box<dyn Any>) -> Result<(), AttachError>;
}

/// Why a listener list was refused.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum AttachError {
    /// The listeners' type is not this point's signature.
    Signature,
    /// The point was already filled.
    Filled,
}

impl<F: Send + Sync + 'static> Attach for Point<F> {
    fn name(&self) -> &'static str {
        self.name
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

/// Call `call` with each attached listener, off the hot path.
#[cold]
#[inline(never)]
pub fn fire<F>(listeners: &[F], mut call: impl FnMut(&F)) {
    for listener in listeners {
        call(listener);
    }
}
