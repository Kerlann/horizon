//! Opaque identities for fluid names retained from unavailable content.
use crate::{FluidKey, Host, UnresolvedFluid};
use std::{
    collections::HashMap,
    sync::{LazyLock, RwLock},
};

/// A registered fluid, or an opaque retained name from unavailable content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FluidIdentity {
    /// A registered fluid.
    Resolved(FluidKey),
    /// A name retained verbatim until its declaring mod returns.
    Unresolved(UnresolvedFluid),
}

impl FluidIdentity {
    /// Its registered key, or none when the fluid is unresolved.
    pub fn key(self) -> Option<FluidKey> {
        match self {
            Self::Resolved(key) => Some(key),
            Self::Unresolved(_) => None,
        }
    }
}

impl UnresolvedFluid {
    pub(crate) fn from_slot(slot: u64) -> Result<Self, &'static str> {
        let table = TABLE.read().unwrap();
        (slot < table.names.len() as u64)
            .then_some(Self(slot))
            .ok_or("unknown unresolved fluid")
    }

    /// The original stored name, read from the host's process-lifetime table.
    pub fn id(self, host: &dyn Host) -> String {
        host.unresolved_fluid_id(self)
    }
}

#[derive(Default)]
struct Table {
    names: Vec<String>,
    slots: HashMap<String, u64>,
}
static TABLE: LazyLock<RwLock<Table>> = LazyLock::new(Default::default);

/// Intern a stored name at an engine-owned data boundary.
pub fn intern(id: &str) -> UnresolvedFluid {
    let mut table = TABLE.write().unwrap();
    if let Some(&slot) = table.slots.get(id) {
        return UnresolvedFluid(slot);
    }
    let slot = table.names.len() as u64;
    table.names.push(id.to_owned());
    table.slots.insert(id.to_owned(), slot);
    UnresolvedFluid(slot)
}

/// Look up an already-interned identity without creating one.
pub fn lookup(id: &str) -> Option<UnresolvedFluid> {
    TABLE
        .read()
        .unwrap()
        .slots
        .get(id)
        .copied()
        .map(UnresolvedFluid)
}

/// Resolve an identity at an engine-owned boundary.
pub fn identity_name(identity: FluidIdentity) -> String {
    match identity {
        FluidIdentity::Resolved(key) => crate::registry_keys::fluid_id(key)
            .expect("resolved fluid key")
            .to_owned(),
        FluidIdentity::Unresolved(handle) => name(handle),
    }
}

/// Resolve an engine-issued process-lifetime handle.
pub fn name(fluid: UnresolvedFluid) -> String {
    TABLE.read().unwrap().names[fluid.0 as usize].clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn arbitrary_guest_input_cannot_intern_a_name() {
        use crate::{DataLoadOrigin, OwningLoad};
        use horizon_world::native_load_context::{LoadContext, LoadInputs};
        use std::sync::Arc;
        let load = LoadContext::new(
            DataLoadOrigin::ServerDataLoad,
            Arc::new(LoadInputs::new(Default::default(), Default::default())),
            [],
        );
        let name = "never_interned:guest_input_39218";
        assert!(lookup(name).is_none());
        assert!(matches!(
            crate::FluidIngredient::from_json(&serde_json::json!(name), &OwningLoad::new(&load)),
            Err(crate::FluidIngredientError::UnknownFluid(_))
        ));
        assert!(lookup(name).is_none());
    }

    #[test]
    fn handles_refuse_unissued_slots_and_survive_other_worlds() {
        let old = intern("absent:fluid");
        assert_eq!(old, intern("absent:fluid"));
        assert!(UnresolvedFluid::from_slot(u64::MAX).is_err());
        let next = intern("other:fluid");
        assert_ne!(old, next);
        assert_eq!(name(old), "absent:fluid");
    }
}
