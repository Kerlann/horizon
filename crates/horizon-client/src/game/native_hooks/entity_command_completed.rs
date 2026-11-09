//! A real terminal, never an acceptance notification or a broadcast.
use super::Game;
impl Game<'_> {
    pub(in crate::game) fn native_entity_command_completed(
        &mut self,
        completion: &mod_api::EntityWriteCompletion,
    ) {
        self.fire_entity_command_completed(completion);
    }
}
