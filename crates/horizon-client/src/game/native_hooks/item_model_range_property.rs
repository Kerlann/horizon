use super::*;

/// The hook points. Each does nothing in Horizon.
impl Game<'_> {
    /// Client-side value of a numeric item model property a mod registered
    /// (`register_range_select_item_model_property`), asked each time a `minecraft:range_dispatch`
    /// on it picks a branch. Set value only for a property your mod owns; unanswered it stays 0.
    /// The model's `scale` is applied afterwards. The borrowed stack is the one being resolved,
    /// including nested bundle items.
    pub(in crate::game) fn native_item_model_range_property(
        &self,
        property: &str,
        stack: &StackView,
        value: &mut f32,
    ) {
        self.fire_item_model_range_property(property, stack, value);
    }
}
