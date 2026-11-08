//! Native, non-owning implementations of ordinary callable resources.
use crate::snapshot::Root;
use mod_api::{
    RowError, RowFunction,
    wire::{self, Reader, WireError, Writer},
};
use std::{cell::RefCell, sync::Arc};

type Invoke = unsafe fn(*const (), *const (), *const (), &[u8]) -> Result<Vec<u8>, RowError>;

/// A caller-owned mutable callable, reachable only during its marked call and value scope.
#[doc(hidden)]
pub struct FunctionSource {
    call: wire::ScopeToken,
    value: wire::ScopeToken,
    root: *const (),
    decode: *const (),
    encode: *const (),
    invoke: Invoke,
}
// SAFETY: both tokens are checked on every invocation before touching the native root;
// wrong-thread and expired calls are refused. Dropping only drops tokens and pointer values.
unsafe impl Send for FunctionSource {}
unsafe impl Sync for FunctionSource {}
impl FunctionSource {
    /// Capture a callable and generated codecs. The decoder reads the entire argument tuple;
    /// trailing bytes are refused before the callable is borrowed or invoked.
    ///
    /// # Safety
    /// Construct only during the marked call from the caller's `with_root` handle. Native
    /// values referenced by the callable and codec functions must remain valid until the value
    /// lease ends. The codecs must decode and encode the declared native types faithfully.
    pub unsafe fn new<F, A, R>(
        root: &Root<'_, RefCell<F>>,
        decode: fn(&mut Reader<'_>) -> Result<A, WireError>,
        encode: fn(&R, &mut Writer),
    ) -> Result<Arc<dyn RowFunction>, RowError>
    where
        F: FnMut(A) -> R,
    {
        unsafe fn invoke<F, A, R>(
            root: *const (),
            decode: *const (),
            encode: *const (),
            bytes: &[u8],
        ) -> Result<Vec<u8>, RowError>
        where
            F: FnMut(A) -> R,
        {
            // SAFETY: new pairs each pointer with this monomorphization and checks both leases.
            let decode: fn(&mut Reader<'_>) -> Result<A, WireError> =
                unsafe { std::mem::transmute(decode) };
            let encode: fn(&R, &mut Writer) = unsafe { std::mem::transmute(encode) };
            let mut reader = Reader::new(bytes);
            let arguments = decode(&mut reader)?;
            if !reader.rest().is_empty() {
                return Err(WireError::Trailing.into());
            }
            // SAFETY: both leases were checked by the caller; root points to the original RefCell.
            let cell = unsafe { &*root.cast::<RefCell<F>>() };
            let mut callable = cell.try_borrow_mut().map_err(|_| RowError::Reentrant)?;
            let result = callable(arguments);
            let mut writer = Writer::new();
            encode(&result, &mut writer);
            if let Some(error) = writer.error() {
                return Err(error.into());
            }
            Ok(writer.into_bytes())
        }
        Ok(Arc::new(Self {
            call: wire::borrow_token()?,
            value: root.token,
            root: std::ptr::from_ref(root.value).cast(),
            decode: decode as *const (),
            encode: encode as *const (),
            invoke: invoke::<F, A, R>,
        }))
    }
}
impl RowFunction for FunctionSource {
    fn invoke(&self, arguments: Vec<u8>) -> Result<Vec<u8>, RowError> {
        self.call.check()?;
        self.value.check()?;
        // SAFETY: the checked tokens and new's contract protect the paired root and codecs.
        unsafe { (self.invoke)(self.root, self.decode, self.encode, &arguments) }
    }
}
