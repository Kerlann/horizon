//! Exact-generation access to real no-gravity storage; no inferred ownership.
macro_rules! no_gravity_slot {
    (@write, $list:expr, $at:expr, $id:expr, $value:expr, $sync:expr) => {{
        let index = $list.locate($at.join).ok_or(mod_api::EntityWriteError::OutsideScope)?;
        if $list[index].id != $id { return Err(mod_api::EntityWriteError::GenerationChanged); }
        $list.update(index, |entity| {
            entity.no_gravity = $value;
            ($sync)(entity);
            Ok(true)
        })
    }};
    (Mob, $list:expr, $at:expr, $id:expr, $value:expr) => {
        $crate::game::native_no_gravity_source::no_gravity_slot!(@write, $list, $at, $id, $value, |entity: &mut horizon_client::mobs::Mob| entity.needs_sync = true)
    };
    (Item, $list:expr, $at:expr, $id:expr, $value:expr) => { $crate::game::native_no_gravity_source::no_gravity_slot!(@write, $list, $at, $id, $value, |_| {}) };
    (Orb, $list:expr, $at:expr, $id:expr, $value:expr) => { $crate::game::native_no_gravity_source::no_gravity_slot!(@write, $list, $at, $id, $value, |_| {}) };
    (Stand, $list:expr, $at:expr, $id:expr, $value:expr) => { $crate::game::native_no_gravity_source::no_gravity_slot!(@write, $list, $at, $id, $value, |_| {}) };
    (Boat, $list:expr, $at:expr, $id:expr, $value:expr) => { $crate::game::native_no_gravity_source::no_gravity_slot!(@write, $list, $at, $id, $value, |_| {}) };
    (Cart, $list:expr, $at:expr, $id:expr, $value:expr) => { $crate::game::native_no_gravity_source::no_gravity_slot!(@write, $list, $at, $id, $value, |_| {}) };
    (Projectile, $list:expr, $at:expr, $id:expr, $value:expr) => { $crate::game::native_no_gravity_source::no_gravity_slot!(@write, $list, $at, $id, $value, |_| {}) };
    (Tnt, $list:expr, $at:expr, $id:expr, $value:expr) => { $crate::game::native_no_gravity_source::no_gravity_slot!(@write, $list, $at, $id, $value, |_| {}) };
    (Trident, $list:expr, $at:expr, $id:expr, $value:expr) => { $crate::game::native_no_gravity_source::no_gravity_slot!(@write, $list, $at, $id, $value, |_| {}) };
    (Falling, $list:expr, $at:expr, $id:expr, $value:expr) => { $crate::game::native_no_gravity_source::no_gravity_slot!(@write, $list, $at, $id, $value, |_| {}) };
    ($unsupported:ident, $list:expr, $at:expr, $id:expr, $value:expr) => { Err(mod_api::EntityWriteError::Unsupported) };
}
pub(crate) use no_gravity_slot;

impl super::native_base_read_rows::BaseReadSource<'_> {
    pub(super) fn no_gravity(self) -> Option<bool> {
        use super::native_base_read_rows::BaseReadSource::*;
        Some(match self {
            Mob(e) => e.no_gravity,
            Item(e) => e.no_gravity,
            Orb(e) => e.no_gravity,
            Stand(e) => e.no_gravity,
            Boat(e) => e.no_gravity,
            Cart(e) => e.no_gravity,
            Projectile(e) => e.no_gravity,
            Tnt(e) => e.no_gravity,
            Falling(e) => e.no_gravity,
            Trident(e) => e.no_gravity,
            _ => return None,
        })
    }
}
