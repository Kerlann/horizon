//! Entity.DATA_NO_GRAVITY on real client-side storage, shared by every family
//! that implements the flag. The ordered server stream owns these runtime IDs.
use super::Game;
use horizon_protocol::entity_data::Value;

impl Game<'_> {
    pub(super) fn remote_no_gravity_data(&mut self, id: i32, values: &[(u8, i32, Value)]) {
        let Some(value) = values
            .iter()
            .rev()
            .find_map(|(index, _, value)| match (index, value) {
                (5, Value::Bool(value)) => Some(*value),
                _ => None,
            })
        else {
            return;
        };
        if self.retain_pending_item_no_gravity(id, value) {
            return;
        }
        let id = id as u32;
        // Mutate the actual replica body, never a remembered read projection.
        macro_rules! apply {
            ($list:expr) => {
                let index = $list.iter().position(|entity| entity.id == id);
                if let Some(index) = index {
                    $list.update(index, |entity| entity.no_gravity = value);
                    return;
                }
            };
        }
        apply!(self.mobs);
        apply!(self.dropped);
        apply!(self.orbs);
        apply!(self.armor_stands);
        apply!(self.boats);
        apply!(self.carts);
        apply!(self.creatures.projectiles);
        apply!(self.tnts);
        apply!(self.tridents);
        apply!(self.falling);
    }
}
