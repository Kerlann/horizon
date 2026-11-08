//! Owned row windows over a resource whose lifetime is checked by its provider.

/// A node id, parent id, declaration-order field/index id, type key and plain Wire value.
/// Nodes without a plain value have empty value bytes and are read through their children.
/// Ids belong to one source; neither a node nor a type key is an engine address.
pub type Row = (u32, Option<u32>, u32, u64, Vec<u8>);

// exhaustive: generated boundary code builds it with a struct literal; a new field breaks that code
/// One bounded traversal of a row source. Providers check this at their native boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RowWindow {
    /// The source-local node whose fields or elements are requested.
    pub node: u32,
    /// First declaration-order field or collection index.
    pub start: u64,
    /// Maximum rows, including structural nodes needed to describe the result.
    pub rows: u32,
    /// Maximum child depth; zero requests only the node itself.
    pub depth: u16,
}

/// Why a row window cannot be read. A refusal exposes no partial window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum RowError {
    /// The source's native call ended. Retaining an interface never retains its borrow.
    Stale,
    /// The source's owning thread is not the calling thread.
    WrongThread,
    /// The node or range does not exist in this source.
    Bounds,
    /// The window exceeds its row or encoded-byte budget.
    Budget,
    /// The requested child depth exceeds the boundary's nesting limit.
    Depth,
    /// The callable is already mutably borrowed by an invocation.
    Reentrant,
    /// The provider does not expose this operation or node kind.
    Unsupported,
}
impl crate::NoBackend for RowError {
    fn no_backend() -> Self {
        Self::Unsupported
    }
}
impl From<(crate::Identifier, String)> for RowError {
    fn from(_: (crate::Identifier, String)) -> Self {
        Self::Unsupported
    }
}
impl From<crate::wire::WireError> for RowError {
    fn from(error: crate::wire::WireError) -> Self {
        match error {
            crate::wire::WireError::Length => Self::Budget,
            crate::wire::WireError::Depth => Self::Depth,
            _ => Self::Unsupported,
        }
    }
}
impl From<crate::wire::ScopeError> for RowError {
    fn from(error: crate::wire::ScopeError) -> Self {
        match error {
            crate::wire::ScopeError::Stale => Self::Stale,
            crate::wire::ScopeError::WrongThread => Self::WrongThread,
        }
    }
}

/// A scoped callable exchanging owned ordinary Wire values. The argument bytes encode one
/// tuple of arguments; the returned bytes encode its result. Providers reject malformed or
/// trailing input before invoking native code. A retained callable cannot extend its native
/// borrow and cannot be called from another thread.
pub trait RowFunction: Send + Sync {
    /// Invoke the callable, or return a typed refusal without exposing native memory.
    fn invoke(&self, arguments: Vec<u8>) -> Result<Vec<u8>, RowError>;
}

/// A read-only source of owned, flattened row windows. The interface carries no engine type.
/// Native borrowed providers check the arena's native-root token before accessing their root.
/// A retained or transferred interface cannot extend that lifetime or permit another thread to
/// read it. Every successful result is owned and remains valid independently of the provider.
pub trait RowPages: Send + Sync {
    /// Read a bounded window or refuse it completely. Providers cap rows, bytes and depth before
    /// materialization; opaque children remain opaque rather than exposing engine memory.
    fn window(&self, request: RowWindow) -> Result<Vec<Row>, RowError>;
}
