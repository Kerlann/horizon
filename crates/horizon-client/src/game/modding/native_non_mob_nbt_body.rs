//! Serialization dispatch over one actual borrowed body, never a body copy.
use super::{native_mob_nbt_save::MobNbtOwner, native_nbt_save_owner::{AdmittedSaveOwner, SaveError}, native_nbt_save_requirements::{SaveRequirement, SaveRequirements}};
use horizon_client::{entity_nbt, join_seq::residency::Kind, mobs::Mob};
use horizon_core::{geometry::Vec3, nbt::Tag};
use horizon_items::items::Items;

// Reuse the existing owner discovery. The closure captures actual non-Mob
// fields precisely, leaving table/Mob/persistence loans disjoint. It returns
// one real body reference, not a directory or a fabricated serialization view.
macro_rules! project_non_mob_save_body {
    (($this:ident, $($projectiles:ident).+) Mob => Mob: mobs, $($kind:ident => $target:ident: $($field:ident).+,)*) => {
        |location: horizon_client::join_seq::residency::table::Location| -> Option<&dyn $crate::game::native_non_mob_nbt_body::NonMobNbt> {
            use horizon_client::join_seq::residency::Kind;
            match location.owner.kind {
                $(Kind::$kind => {
                    let index = $this.$($field).+.locate(location.join)?;
                    Some(&$this.$($field).+[index] as &dyn $crate::game::native_non_mob_nbt_body::NonMobNbt)
                },)*
                Kind::Projectile => {
                    let index = $this.$($projectiles).+.locate(location.join)?;
                    Some(&$this.$($projectiles).+[index] as &dyn $crate::game::native_non_mob_nbt_body::NonMobNbt)
                }
                Kind::Dragon => $this.dragon.as_ref().filter(|d| d.join == location.join).map(|d| d as &dyn $crate::game::native_non_mob_nbt_body::NonMobNbt),
                Kind::Mob | Kind::Player | Kind::Hook | Kind::DragonPart => None,
            }
        }
    };
}
pub(super) use project_non_mob_save_body;

pub(super) struct SaveInputs<'a> {
    pub(super) items: &'a Items,
    pub(super) command_player_uuid: Option<[i32; 4]>,
    /// The actual global guest identity selected by this TNT body's connection.
    pub(super) tnt_guest_uuid: Option<[i32; 4]>,
    pub(super) portal_states: &'a std::collections::HashMap<u32, horizon_client::nether_portal::PortalState>,
}

/// Cold-path dispatch through a reference to the real selected storage value.
/// Part discovery precedes reference discovery, so an unavailable Mob roster
/// cannot masquerade as an empty roster. Parent owns physical admission.
pub(super) trait NonMobNbt {
    fn id(&self) -> u32;
    fn kind(&self) -> Kind;
    fn parts(&self, out: &mut SaveRequirements) {
        out.require(SaveRequirement::Body(self.kind()));
        out.require(SaveRequirement::Persistence);
    }
    fn references(&self, _mobs: &horizon_client::join_seq::JoinList<Mob>, _persist: &super::persist::EntityMemory, out: &mut SaveRequirements) -> Result<(), SaveError> {
        out.uuid(self.id());
        Ok(())
    }
    fn save(&self, owner: &mut AdmittedSaveOwner<'_>, input: &SaveInputs<'_>) -> Result<Option<Tag>, SaveError>;
}
macro_rules! simple_save {
    ($ty:ty, $kind:ident, |$body:ident, $owner:ident, $input:ident| $save:block $(, parts |$part_body:ident, $out:ident| $parts:block)?) => {
        impl NonMobNbt for $ty {
            fn id(&self) -> u32 { self.id }
            fn kind(&self) -> Kind { Kind::$kind }
            $(fn parts(&self, $out: &mut SaveRequirements) {
                let $part_body = self;
                $out.require(SaveRequirement::Body(Kind::$kind));
                $out.require(SaveRequirement::Persistence);
                $parts
            })?
            fn save(&self, $owner: &mut AdmittedSaveOwner<'_>, $input: &SaveInputs<'_>) -> Result<Option<Tag>, SaveError> {
                let $body = self;
                let result: Result<Tag, SaveError> = $save;
                result.map(Some)
            }
        }
    };
}
simple_save!(horizon_client::entities::ItemEntity, Item, |body, owner, input| {
    let uuid = owner.uuid_for(body.id)?;
    Ok(entity_nbt::item_entity_to_nbt_with_items(body, uuid, owner.persist.raw.get(&body.id), input.items))
}, parts |_body, out| { out.require(SaveRequirement::ItemRegistry); });
simple_save!(horizon_client::entities::ArmorStand, Stand, |body, owner, _input| {
    let uuid = owner.uuid_for(body.id)?;
    Ok(entity_nbt::armor_stand_to_nbt(body, uuid, owner.persist.raw.get(&body.id)))
});
simple_save!(horizon_client::entities::ItemFrame, Frame, |body, owner, _input| {
    let uuid = owner.uuid_for(body.id)?;
    Ok(entity_nbt::item_frame_to_nbt(body, uuid, owner.persist.raw.get(&body.id)))
});
simple_save!(horizon_client::entities::Painting, Painting, |body, owner, _input| {
    let uuid = owner.uuid_for(body.id)?;
    Ok(entity_nbt::painting_to_nbt(body, uuid, owner.persist.raw.get(&body.id)))
});
simple_save!(horizon_client::entities::Cushion, Cushion, |body, owner, _input| {
    let uuid = owner.uuid_for(body.id)?;
    Ok(entity_nbt::cushion_to_nbt(body, uuid, owner.persist.raw.get(&body.id)))
});
simple_save!(horizon_client::entities::ExperienceOrb, Orb, |body, owner, _input| {
    let uuid = owner.uuid_for(body.id)?;
    Ok(super::persist::with_raw(entity_nbt::orb_to_nbt(body, uuid), owner.persist.raw.get(&body.id)))
});
simple_save!(super::fangs::Fangs, Fangs, |body, owner, _input| {
    let uuid = owner.uuid_for(body.id)?;
    Ok(entity_nbt::evoker_fangs_to_nbt(Vec3::new(body.pos[0], body.pos[1], body.pos[2]), body.yaw, body.warmup, owner.persist.uuids.get(&body.owner).copied(), uuid, owner.persist.raw.get(&body.id)))
});
simple_save!(super::commands::OtherEntity, Other, |body, owner, input| {
    let uuid = owner.uuid_for(body.id)?;
    Ok(super::persist::other_to_nbt(body, uuid, owner.persist.raw.get(&body.id), owner.registry, input.items))
}, parts |body, out| {
    if body.kind == "minecraft:block_display" { out.require(SaveRequirement::BlockRegistry); }
    if body.kind == "minecraft:item_display" { out.require(SaveRequirement::ItemRegistry); }
});
simple_save!(super::trident::ThrownTrident, Trident, |body, owner, _input| {
    let uuid = owner.uuid_for(body.id)?;
    Ok(body.to_nbt(uuid, owner.registry, owner.persist.raw.get(&body.id)))
}, parts |_body, out| { out.require(SaveRequirement::BlockRegistry); });
simple_save!(super::fireworks::Rocket, Rocket, |body, owner, input| {
    let uuid = owner.uuid_for(body.id)?;
    Ok(body.to_nbt(uuid, input.command_player_uuid, owner.persist.raw.get(&body.id)))
}, parts |_body, out| { out.require(SaveRequirement::CommandPlayerUuid); });
