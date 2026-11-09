//! A native-only borrowed root behind the ordinary RowPages resource interface.
use super::{Builder, Rows};
use mod_api::wire::{ScopeLease as ValueLease, ScopeToken as ValueToken};
use mod_api::{RowError, RowPages, RowWindow, wire};
use std::{cell::RefCell, sync::Arc};

/// A borrowed handle to a caller's stable native value. Only `with_root` owns its lease;
/// forgetting this handle cannot leak that lease or extend the value's lifetime.
#[doc(hidden)]
pub struct Root<'a, T> {
    pub(crate) value: &'a T,
    pub(crate) token: ValueToken,
    lease: &'a RefCell<Option<ValueLease>>,
}
impl<T> Root<'_, T> {
    /// Invalidate views before mutable native record apply. This does not authorize writes
    /// through the shared value: writes still require the caller's original exclusive borrow.
    pub fn close(&self) {
        self.lease.borrow_mut().take();
    }
}
/// Borrow a caller's stable native root with a callback-owned private lease. Create this scope
/// outside encoding, around the actual guest call, not around an encoder-local `&self`.
/// The lease always ends when the callback returns or unwinds, even if it forgets its public
/// Root handle. Returned proxies are non-owning and refuse reads after this scope ends.
#[doc(hidden)]
pub fn with_root<T, R>(value: &T, fire: impl FnOnce(Root<'_, T>) -> R) -> R {
    let lease = ValueLease::new();
    let token = lease.token();
    let lease = RefCell::new(Some(lease));
    fire(Root {
        value,
        token,
        lease: &lease,
    })
}

/// A window after its native boundary has checked integer width and resource limits.
#[derive(Clone, Copy)]
pub struct Window {
    node: u32,
    start: usize,
    rows: usize,
    depth: u16,
    lease: Option<ValueToken>,
}
impl Window {
    /// The value lease of the source serving this window, which callables built while reading
    /// it share. `None` for a window not served by a source.
    pub fn lease(self) -> Option<ValueToken> {
        self.lease
    }
    /// Make an owned-output builder using this already-validated window's resource limits.
    pub fn builder(self) -> Builder {
        Builder::limited(self)
    }
    /// Finish a bounded builder into owned rows without copying.
    pub fn finish(self, builder: Builder) -> Rows {
        builder.finish().into_rows()
    }
    /// Source-local node; the source schema owns checking that it exists.
    pub fn node(self) -> u32 {
        self.node
    }
    /// First field or element, representable on this host.
    pub fn start(self) -> usize {
        self.start
    }
    /// Maximum rows, at most the call's cell budget.
    pub fn rows(self) -> usize {
        self.rows
    }
    /// Maximum child depth, at most the shared nesting limit.
    pub fn depth(self) -> u16 {
        self.depth
    }
}
impl TryFrom<RowWindow> for Window {
    type Error = RowError;
    fn try_from(request: RowWindow) -> Result<Self, RowError> {
        if request.rows > 65_536 {
            return Err(RowError::Budget);
        }
        if request.depth > wire::MAX_DEPTH {
            return Err(RowError::Depth);
        }
        Ok(Self {
            node: request.node,
            start: usize::try_from(request.start).map_err(|_| RowError::Bounds)?,
            rows: request.rows as usize,
            depth: request.depth,
            lease: None,
        })
    }
}

type Read = unsafe fn(*const (), *const (), Window) -> Result<Rows, RowError>;
/// A stable native root borrowed only for one marked runtime call. This object owns no root
/// data: retaining it cannot retain either the engine borrow or per-call locator/cache storage.
/// Only the owned rows returned by its interface are transferable beyond the call.
#[doc(hidden)]
pub struct PageSource {
    token: wire::ScopeToken,
    value: ValueToken,
    root: *const (),
    reader: *const (),
    read: Read,
}
// SAFETY: every method checks the non-owning root token before accessing the root. A token
// rejects another thread, so !Send/!Sync native roots are never read there. Drop touches only
// pointer values and the token, never the pointee, including after the call has ended.
unsafe impl Send for PageSource {}
unsafe impl Sync for PageSource {}
impl PageSource {
    /// Borrow a native call's stable root through the nearest arena-native marker.
    ///
    /// # Safety
    /// The trusted reader must not invoke mutating host operations while it reads. It must
    /// check schema-local node/range bounds and only emit owned values using the common Window
    /// builder or native_read, preserving all row, depth and value budgets. If the root contains
    /// borrowed engine pointers, their pointees must remain valid throughout `with_root`;
    /// locators must resolve movable elements afresh rather than cache element pointers.
    /// Construct during marked encoding, from a caller's `with_root` handle, not an encoder's
    /// temporary `&self`. The callback frame's private lease protects its value's shorter
    /// lifetime even if its public handle is forgotten. The call marker alone cannot do so.
    // TODO(#2273): a `new` that returns a shared trait object, not `Self`, by design.
    #[allow(clippy::new_ret_no_self)]
    pub unsafe fn new<T>(
        root: &Root<'_, T>,
        reader: fn(&T, Window) -> Result<Rows, RowError>,
    ) -> Result<Arc<dyn RowPages>, RowError> {
        unsafe fn read<T>(
            root: *const (),
            reader: *const (),
            request: Window,
        ) -> Result<Rows, RowError> {
            // SAFETY: new paired this reader with exactly T. The caller checks the marker
            // before reaching this shim, and new's contract keeps the root stable until then.
            let reader: fn(&T, Window) -> Result<Rows, RowError> =
                unsafe { std::mem::transmute(reader) };
            reader(unsafe { &*root.cast::<T>() }, request)
        }
        let token = wire::borrow_token()?;
        Ok(Arc::new(Self {
            token,
            value: root.token,
            root: std::ptr::from_ref(root.value).cast(),
            reader: reader as *const (),
            read: read::<T>,
        }))
    }
}
impl RowPages for PageSource {
    fn window(&self, request: RowWindow) -> Result<Rows, RowError> {
        self.token.check()?;
        self.value.check()?;
        let request = Window {
            lease: Some(self.value),
            ..Window::try_from(request)?
        };
        if request.rows() == 0 {
            return Ok(Vec::new());
        }
        // SAFETY: the marker is live on this thread; new attests the root's stable address,
        // no overlapping mutable access, and the shim's exact reader/root pairing.
        unsafe { (self.read)(self.root, self.reader, request) }
    }
}
