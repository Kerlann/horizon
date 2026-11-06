use super::*;

/// The hook points. Each does nothing in Horizon.
impl Game<'_> {
    /// Client-side evaluation of a mod's namespaced conditional item model property.
    /// Set value only for a property your mod owns; unhandled properties stay false.
    /// The borrowed stack is the one being resolved, including nested bundle items.
    pub(in crate::game) fn native_item_model_property(
        &self,
        property: &str,
        stack: &StackView,
        value: &mut bool,
    ) {
        self.fire_item_model_property(property, stack, value);
    }
}
