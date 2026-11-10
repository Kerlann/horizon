//! Owner-local serializers whose actual bodies have dependent save state.
use super::{native_non_mob_nbt_body::{NonMobNbt, SaveInputs}, native_mob_nbt_save::MobNbtOwner, native_nbt_save_owner::{AdmittedSaveOwner, SaveError}, native_nbt_save_requirements::{SaveRequirement, SaveRequirements}, persist::EntityMemory};
use horizon_client::{entity_nbt, join_seq::{JoinList, residency::Kind}, mobs::{Mob, Who, leash::Holder}};
use horizon_core::{geometry::Vec3, nbt::Tag};

impl NonMobNbt for super::boats::Boat {
    fn id(&self) -> u32 { self.id }
    fn kind(&self) -> Kind { Kind::Boat }
    fn parts(&self, out: &mut SaveRequirements) {
        out.require(SaveRequirement::Body(Kind::Boat)); out.require(SaveRequirement::Persistence); out.require(SaveRequirement::MobRoster);
        if self.leash == Some(Holder::Player) && self.leash_player.is_none() { out.require(SaveRequirement::WorldPlayerUuid); }
    }
    fn references(&self, mobs: &JoinList<Mob>, persist: &EntityMemory, out: &mut SaveRequirements) -> Result<(), SaveError> {
        out.uuid(self.id);
        out.mob_passengers(mobs, persist, self.id)?;
        if let Some(Holder::Mob(id) | Holder::Boat(id)) = self.leash { out.uuid(id); }
        Ok(())
    }
    fn save(&self, owner: &mut AdmittedSaveOwner<'_>, _: &SaveInputs<'_>) -> Result<Option<Tag>, SaveError> {
        let uuid = owner.uuid_for(self.id)?;
        let mut tag = self.to_nbt(uuid, owner.persist.raw.get(&self.id));
        owner.put_mob_passengers(&mut tag, self.id)?;
        let player = self.leash_player.or(owner.player_uuid);
        super::boat_leads::write_boat_leash_tag(&mut tag, self.leash, self.leash_pending, player, |id| owner.uuid_for(id))?;
        Ok(Some(tag))
    }
}
impl NonMobNbt for super::minecarts::Minecart {
    fn id(&self) -> u32 { self.id }
    fn kind(&self) -> Kind { Kind::Cart }
    fn parts(&self, out: &mut SaveRequirements) {
        out.require(SaveRequirement::Body(Kind::Cart)); out.require(SaveRequirement::Persistence); out.require(SaveRequirement::MobRoster);
    }
    fn references(&self, mobs: &JoinList<Mob>, persist: &EntityMemory, out: &mut SaveRequirements) -> Result<(), SaveError> {
        out.uuid(self.id); out.mob_passengers(mobs, persist, self.id)
    }
    fn save(&self, owner: &mut AdmittedSaveOwner<'_>, _: &SaveInputs<'_>) -> Result<Option<Tag>, SaveError> {
        let uuid = owner.uuid_for(self.id)?;
        let mut tag = self.to_nbt(uuid, owner.persist.raw.get(&self.id));
        owner.put_mob_passengers(&mut tag, self.id)?;
        Ok(Some(tag))
    }
}
impl NonMobNbt for super::FallingBlock {
    fn id(&self) -> u32 { self.id }
    fn kind(&self) -> Kind { Kind::Falling }
    fn parts(&self, out: &mut SaveRequirements) {
        out.require(SaveRequirement::Body(Kind::Falling)); out.require(SaveRequirement::Persistence); out.require(SaveRequirement::BlockRegistry);
    }
    fn save(&self, owner: &mut AdmittedSaveOwner<'_>, _: &SaveInputs<'_>) -> Result<Option<Tag>, SaveError> {
        let (uuid, raw) = match &self.saved { Some(saved) => (saved.0, Some(&saved.1)), None => (owner.uuid_for(self.id)?, None) };
        let tag = entity_nbt::falling_block_to_nbt(entity_nbt::state_to_nbt(owner.registry, self.state), self.position, self.velocity, self.age, self.drop_item, self.damage, self.fall_distance, self.on_ground, self.rotation, uuid);
        Ok(Some(super::persist::with_raw(tag, raw)))
    }
}
impl NonMobNbt for horizon_client::projectiles::Projectile {
    fn id(&self) -> u32 { self.id }
    fn kind(&self) -> Kind { Kind::Projectile }
    fn parts(&self, out: &mut SaveRequirements) {
        out.require(SaveRequirement::Body(Kind::Projectile)); out.require(SaveRequirement::Persistence); out.require(SaveRequirement::BlockRegistry); out.require(SaveRequirement::PortalStates);
        if self.owner == Some(Who::Player) { out.require(SaveRequirement::CommandPlayerUuid); }
    }
    fn save(&self, owner: &mut AdmittedSaveOwner<'_>, input: &SaveInputs<'_>) -> Result<Option<Tag>, SaveError> {
        let uuid = owner.uuid_for(self.id)?;
        let who = match &self.kind { horizon_client::projectiles::ProjectileKind::ShulkerBullet { target, .. } => *target, _ => None };
        // This intentionally reads a retained identity map, not live target state.
        let target = who.and_then(|who| entity_nbt::who_uuid(who, &mut |id| owner.persist.uuid_of(id)));
        let player = (self.owner == Some(Who::Player)).then_some(input.command_player_uuid).flatten();
        let block = self.arrow.as_ref().and_then(|a| a.last_state).map(|state| entity_nbt::state_to_nbt(owner.registry, state));
        let raw = owner.persist.raw.get(&self.id);
        let tag = entity_nbt::arrow_to_nbt(self, uuid, self.owner_uuid.or(player), block, raw)
            .or_else(|| entity_nbt::fireball_to_nbt(self, uuid, player, raw))
            .or_else(|| entity_nbt::throwable_to_nbt(self, uuid, player, raw))
            .or_else(|| entity_nbt::spit_or_bullet_to_nbt(self, uuid, player, target, raw));
        Ok(tag.map(|mut tag| {
            if self.arrow.is_none() && let Tag::Compound(fields) = &mut tag {
                fields.insert("PortalCooldown".into(), Tag::Int(input.portal_states.get(&self.id).map_or(0, |p| p.cooldown)));
            }
            tag
        }))
    }
}
impl NonMobNbt for super::tnt::PrimedTnt {
    fn id(&self) -> u32 { self.id }
    fn kind(&self) -> Kind { Kind::Tnt }
    fn parts(&self, out: &mut SaveRequirements) {
        out.require(SaveRequirement::Body(Kind::Tnt)); out.require(SaveRequirement::Persistence); out.require(SaveRequirement::BlockRegistry);
        match self.owner { Some(Who::Mob(_)) => out.require(SaveRequirement::MobRoster), Some(Who::Player) => match self.guest { Some(conn) => out.require(SaveRequirement::GuestUuid(conn)), None => out.require(SaveRequirement::WorldPlayerUuid) }, None => {} }
    }
    fn references(&self, _: &JoinList<Mob>, _: &EntityMemory, out: &mut SaveRequirements) -> Result<(), SaveError> {
        out.uuid(self.id); if let Some(Who::Mob(id)) = self.owner { out.uuid(id); } Ok(())
    }
    fn save(&self, owner: &mut AdmittedSaveOwner<'_>, input: &SaveInputs<'_>) -> Result<Option<Tag>, SaveError> {
        let uuid = owner.uuid_for(self.id)?;
        let source = match self.owner {
            Some(Who::Player) => if self.guest.is_some() { input.tnt_guest_uuid } else { owner.player_uuid },
            Some(Who::Mob(id)) if owner.mobs.iter().any(|m| m.id == id) => Some(owner.uuid_for(id)?),
            Some(Who::Mob(id)) => owner.persist.uuids.get(&id).copied(), None => None,
        };
        Ok(Some(entity_nbt::tnt_to_nbt(self.position, self.velocity, self.on_ground, self.fall_distance, self.fuse, self.power, self.block_state_nbt(owner.registry), self.rotation, uuid, source, owner.persist.raw.get(&self.id))))
    }
}
impl NonMobNbt for super::dragon::Crystal {
    fn id(&self) -> u32 { self.id }
    fn kind(&self) -> Kind { Kind::Crystal }
    fn save(&self, owner: &mut AdmittedSaveOwner<'_>, _: &SaveInputs<'_>) -> Result<Option<Tag>, SaveError> {
        let raw = super::persist::anon_raw(owner.persist, "minecraft:end_crystal", self.position);
        let uuid = owner.persist.uuids.get(&self.id).copied().unwrap_or_else(horizon_client::save::random_uuid);
        Ok(Some(super::persist::with_raw(self.to_nbt(uuid, raw.as_ref()), owner.persist.raw.get(&self.id))))
    }
}
impl NonMobNbt for super::dragon::Dragon {
    fn id(&self) -> u32 { self.id }
    fn kind(&self) -> Kind { Kind::Dragon }
    fn save(&self, owner: &mut AdmittedSaveOwner<'_>, _: &SaveInputs<'_>) -> Result<Option<Tag>, SaveError> {
        let raw = super::persist::anon_raw(owner.persist, "minecraft:ender_dragon", self.position);
        let uuid = owner.persist.uuids.get(&self.id).copied().unwrap_or_else(horizon_client::save::random_uuid);
        Ok(Some(super::persist::with_raw(self.to_nbt(uuid, raw.as_ref()), owner.persist.raw.get(&self.id))))
    }
}
impl NonMobNbt for super::ominous_item_spawner::OminousItemSpawner {
    fn id(&self) -> u32 { self.id }
    fn kind(&self) -> Kind { Kind::Spawner }
    fn save(&self, _: &mut AdmittedSaveOwner<'_>, _: &SaveInputs<'_>) -> Result<Option<Tag>, SaveError> {
        Ok(Some(entity_nbt::ominous_item_spawner_to_nbt(&self.item, Vec3::new(self.position[0], self.position[1], self.position[2]), self.spawn_item_after, self.uuid, self.raw.as_deref())))
    }
}
impl NonMobNbt for super::leads::Knot {
    fn id(&self) -> u32 { self.id }
    fn kind(&self) -> Kind { Kind::Knot }
    fn save(&self, owner: &mut AdmittedSaveOwner<'_>, _: &SaveInputs<'_>) -> Result<Option<Tag>, SaveError> {
        let Some(&uuid) = owner.persist.uuids.get(&self.id) else { return Ok(None) };
        let position = horizon_client::mobs::leash::knot_state(self.fence).position;
        let mut tag = entity_nbt::entity_base_tag("minecraft:leash_knot", position, Vec3::ZERO, [0.0, 0.0], uuid, owner.persist.raw.get(&self.id));
        if let Tag::Compound(fields) = &mut tag { fields.insert("Fire".into(), Tag::Short(0)); }
        Ok(Some(tag))
    }
}
impl NonMobNbt for super::dragon::Breath {
    fn id(&self) -> u32 { self.id }
    fn kind(&self) -> Kind { Kind::Cloud }
    fn save(&self, owner: &mut AdmittedSaveOwner<'_>, _: &SaveInputs<'_>) -> Result<Option<Tag>, SaveError> {
        let uuid = owner.uuid_for(self.id)?;
        let raw = owner.persist.raw.get(&self.id);
        if self.potion.is_some() { return Ok(Some(super::potions::potion_cloud_to_nbt(self, uuid, raw))); }
        let mut tag = entity_nbt::entity_base_tag("minecraft:area_effect_cloud", Vec3::new(self.at[0], self.at[1], self.at[2]), Vec3::ZERO, [0.0, 0.0], uuid, raw);
        super::persist::write_static_entity_fields(&mut tag);
        if let Tag::Compound(fields) = &mut tag {
            fields.insert("Age".into(), Tag::Int(self.age)); fields.insert("Duration".into(), Tag::Int(self.duration)); fields.insert("Radius".into(), Tag::Float(self.radius.to_bits())); fields.insert("WaitTime".into(), Tag::Int(20));
        }
        Ok(Some(tag))
    }
}
