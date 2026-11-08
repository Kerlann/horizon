//! Guest listeners sharing the ordinary hook export and wire arena.
use crate::{
    PointId,
    registry::{ApiKind, SlotListener},
};
use std::cell::RefCell;

thread_local! {
    static LISTENERS: RefCell<Vec<(usize, String, *const (), bool)>> = const { RefCell::new(Vec::new()) };
}

/// Installs a typed API listener in this guest and enables the generated export table.
/// The host registers the subscribed places in its ordinary ordered listener list.
pub fn register(listener: &SlotListener) -> Result<(), crate::RegisterError> {
    if let Some(ApiKind::Typed(function)) = listener.api_kind() {
        LISTENERS.with(|slots| {
            slots.borrow_mut().push((
                listener.place(),
                listener.injection().to_owned(),
                function,
                false,
            ))
        });
        mod_api::wire::guest::extend(0x8000_0000, 0xC000_0000, crate::guest_dispatch);
        Ok(())
    } else {
        Err(crate::RegisterError::Unsupported)
    }
}

/// A typed raw listener using the generated record and a read-only row snapshot.
pub struct RawListener {
    place: PointId,
    injection: &'static str,
    function: *const (),
}

impl RawListener {
    /// Constructed only after a generated alias checked this place's exact record type.
    ///
    /// # Safety
    /// The function must have this place's generated row listener signature.
    pub const unsafe fn new(place: PointId, injection: &'static str, function: *const ()) -> Self {
        Self {
            place,
            injection,
            function,
        }
    }
}

/// Install a generated raw listener in the same guest subscription table as API listeners.
pub fn register_raw(listener: &RawListener) -> Result<(), crate::RegisterError> {
    LISTENERS.with(|slots| {
        slots.borrow_mut().push((
            listener.place as usize,
            listener.injection.to_owned(),
            listener.function,
            true,
        ))
    });
    mod_api::wire::guest::extend(0x8000_0000, 0xC000_0000, crate::guest_dispatch);
    mod_api::wire::guest::extend(0xC000_0000, 0xC000_0000, crate::guest_dispatch);
    Ok(())
}

/// The places this guest listens at, in registration order. Bit 30 marks raw listeners.
pub fn subscriptions() -> Vec<(u32, String)> {
    LISTENERS.with(|slots| {
        slots
            .borrow()
            .iter()
            .map(|(place, injection, _, raw)| {
                (
                    *place as u32 | if *raw { 0x4000_0000 } else { 0 },
                    injection.clone(),
                )
            })
            .collect()
    })
}

pub(crate) fn fire(
    place: PointId,
    index: u32,
    raw: bool,
    call: impl FnOnce(*const ()),
) -> Result<(), mod_api::wire::WireError> {
    let function = LISTENERS.with(|slots| {
        slots
            .borrow()
            .get(index as usize)
            .filter(|(at, _, _, kind)| *at == place as usize && *kind == raw)
            .map(|(_, _, function, _)| *function)
    });
    let function = function.ok_or(mod_api::wire::WireError::Invalid("unknown point listener"))?;
    call(function);
    Ok(())
}

/// The full point catalog layout, checked by the host before this guest can subscribe.
#[cfg(target_arch = "wasm32")]
#[unsafe(no_mangle)]
pub extern "C" fn horizon_layout_ext() -> u64 {
    crate::POINT_LAYOUT
}
