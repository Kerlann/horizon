//! Native item data component helpers.
use super::Game;
use horizon_client::items::ItemStack;
use horizon_world::nbt::Tag;

/// Failure to access a native item component.
#[derive(Debug)]
pub(crate) enum NativeComponentError {
    UnknownType,
    InvalidValue(horizon_protocol::Error),
}
impl std::fmt::Display for NativeComponentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownType => f.write_str("unregistered component type"),
            Self::InvalidValue(error) => write!(f, "invalid component value: {error}"),
        }
    }
}
impl std::error::Error for NativeComponentError {}

impl Game<'_> {
    /// Item `item`'s default value of component `component`, in its prototype
    /// (`Item.components().get`): `minecraft:max_damage` on an iron sword is `250`, data
    /// packs' and native mods' changes included. Read from the item report's JSON, so a
    /// number is an `Int`, `Long` or `Double` and a boolean a `Byte`. None where the item
    /// hasn't the component, and for an unknown item.
    pub(super) fn native_item_default_component(&self, item: &str, component: &str) -> Option<Tag> {
        let prototype = self.pack.item_renderer.models.prototypes.get(item)?;
        Some(horizon_client::scoreboard::json_to_tag(
            prototype.get(component)?,
        ))
    }
    /// Whether an enchantment on `stack` has effect component `effect`, by the world's
    /// enchantment definitions (`EnchantmentHelper.has`), behind
    /// `ItemResource::has_enchantment_effect`, through the opaque backend. API
    /// @API:item_components@.
    pub(super) fn native_item_has_enchantment_effect(
        &self,
        stack: &ItemStack,
        effect: &str,
    ) -> bool {
        world_stations!(self)
            .enchantments
            .has_component(stack, effect)
    }
}

// Native mods use these helpers.
#[allow(dead_code)]
impl Game<'_> {
    /// Read a native component, including the item's default when not patched.
    pub(super) fn native_component<'a>(
        &'a self,
        stack: &'a ItemStack,
        id: &str,
    ) -> Result<Option<&'a Tag>, NativeComponentError> {
        if horizon_protocol::item::custom::id(id).is_none() {
            return Err(NativeComponentError::UnknownType);
        }
        if let Some(data) = stack.data() {
            if data.other(&format!("!{id}")).is_some() {
                return Ok(None);
            }
            if let Some(value) = data.other(id) {
                return Ok(Some(value));
            }
        }
        Ok(self
            .items()
            .get(&stack.item)
            .and_then(|item| item.custom_components.get(id)))
    }

    /// Set a registered native component after checking the network NBT limits.
    pub(super) fn native_set_component(
        &self,
        stack: &mut ItemStack,
        id: &str,
        value: Tag,
    ) -> Result<(), NativeComponentError> {
        if horizon_protocol::item::custom::id(id).is_none() {
            return Err(NativeComponentError::UnknownType);
        }
        horizon_protocol::item::custom::validate(&value)
            .map_err(NativeComponentError::InvalidValue)?;
        let default = self
            .items()
            .get(&stack.item)
            .and_then(|item| item.custom_components.get(id));
        let value = (default != Some(&value)).then_some(value);
        let removed = format!("!{id}");
        let data = stack.data_mut();
        data.set_other(&removed, None);
        data.set_other(id, value);
        stack.tidy();
        Ok(())
    }

    /// Remove a native component, including its item default.
    pub(super) fn native_remove_component(
        &self,
        stack: &mut ItemStack,
        id: &str,
    ) -> Result<(), NativeComponentError> {
        if horizon_protocol::item::custom::id(id).is_none() {
            return Err(NativeComponentError::UnknownType);
        }
        let default = self
            .items()
            .get(&stack.item)
            .is_some_and(|item| item.custom_components.contains_key(id));
        let removed = format!("!{id}");
        let data = stack.data_mut();
        data.set_other(id, None);
        data.set_other(&removed, default.then(|| Tag::Compound(Default::default())));
        stack.tidy();
        Ok(())
    }
}
