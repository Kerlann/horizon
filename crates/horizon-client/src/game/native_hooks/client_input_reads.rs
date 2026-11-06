use super::*;

/// Helpers for native mods, kept as stable as the hook points.
// Native mods use these; nothing in Horizon does.
#[allow(dead_code)]
impl Game<'_> {
    /// Playing, with nothing over the world: keys and zoom apply.
    pub(in crate::game) fn native_playing(&self) -> bool {
        self.screen == Screen::Playing
            && self.title.is_none()
            && self.sign_edit.is_none()
            && self.book.is_none()
    }
    /// The current window modifier keys in the same GLFW encoding as input hooks.
    /// Cleared when the window loses focus; available even outside a world.
    pub(in crate::game) fn native_modifiers(&self) -> ClientModifiers {
        ClientModifiers::new(crate::game::glfw_modifiers(self.modifiers))
    }
    /// A vanilla mapping (`key.forward`, `key.jump`, `key.attack`…): whether
    /// it is held (`KeyMapping.isDown`), and its key's name as Controls shows
    /// it. None for a mapping it doesn't answer for.
    pub(in crate::game) fn native_mapping(&self, name: &str) -> Option<NativeKeyMapping> {
        let &(_, key) = MAPPINGS.iter().find(|m| m.0 == name)?;
        let down = match key {
            Some(k) => self.movement_key_held(k),
            None => self.mouse_held()[usize::from(name == "key.use")],
        };
        Some(NativeKeyMapping {
            down,
            label: keybinds::display_name(self.settings.keys.get(name)),
        })
    }
}
