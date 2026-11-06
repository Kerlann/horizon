//! Fluid state definitions from the loaded registry.
use crate::{Backend, BackendRef, FluidKey, FluidState, Host, StateConstraint, StateProperty};

/// The definition for a registered fluid identity.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FluidDefinition {
    /// A world fluid with its default state and legal properties.
    World {
        /// The single typed default state.
        default: FluidState,
        /// Property names and legal serialized values.
        properties: Vec<StateProperty>,
    },
    /// A declared fluid that only exists in tanks.
    TankOnly,
}

/// An invalid fluid-state constraint, identifying its index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FluidStateError {
    /// The definition has no such property.
    UnknownProperty(u32),
    /// The property does not allow this value.
    InvalidValue(u32),
    /// The property appears more than once.
    Duplicate(u32),
}

/// Fluid-state facts available from a host.
// exhaustive: only a host constructs this service
pub struct FluidStates<'a> {
    backend: Option<&'a dyn Backend>,
}

/// Reaches fluid-state facts from any host.
pub trait HostFluidStates {
    /// The current registry's fluid-state facts.
    fn fluid_states(&self) -> FluidStates<'_>;
}
impl<H: Host + ?Sized> HostFluidStates for H {
    fn fluid_states(&self) -> FluidStates<'_> {
        FluidStates {
            backend: self.backend().map(BackendRef::open),
        }
    }
}
impl FluidStates<'_> {
    /// A definition, or none when the registry is unavailable.
    pub fn definition(&self, fluid: FluidKey) -> Option<FluidDefinition> {
        self.backend?.fluid_state_definition(fluid)
    }
    /// Validate partial constraints. None means the table is unavailable or the fluid is tank-only.
    pub fn validate(
        &self,
        fluid: FluidKey,
        constraints: &[StateConstraint],
    ) -> Option<Result<(), FluidStateError>> {
        self.backend?.fluid_state_validate(fluid, constraints)
    }
}
