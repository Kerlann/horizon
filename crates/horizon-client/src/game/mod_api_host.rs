use super::{BlockStateId, Game};
use horizon_world::{
    attachments::{ATTACHMENTS_KEY, attachment_compound},
    level::Ticks,
};
use mod_api::BlockPos;

fn world_attachments(
    world: &mut horizon_client::world::World,
    holder: mod_api::Holder,
) -> Result<&mut mod_api::Attachments, mod_api::AttachmentError> {
    use mod_api::{AttachmentError, Holder};
    if holder == Holder::Level {
        return Ok(&mut world.attachments);
    }
    if let Holder::BlockEntity([x, y, z]) = holder {
        let pos = (x, y, z);
        let chunk = world
            .chunks
            .get(&(x >> 4, z >> 4))
            .ok_or(AttachmentError::MissingHolder)?;
        attachment_compound(
            chunk
                .saved
                .as_ref()
                .and_then(|e| e.block_entity_sources.iter().find(|(p, _)| *p == pos))
                .and_then(|(_, tag)| tag.get(ATTACHMENTS_KEY)),
        )?;
        let map = world
            .block_entities
            .attachments_mut(pos)
            .ok_or(AttachmentError::MissingHolder)?;
        world.modified.insert((x >> 4, z >> 4));
        return Ok(map);
    }
    let Holder::Chunk([x, z]) = holder else {
        return Err(AttachmentError::MissingHolder);
    };
    let chunk = world
        .chunks
        .get_mut(&(x, z))
        .ok_or(AttachmentError::MissingHolder)?;
    let chunk = std::sync::Arc::make_mut(chunk);
    let extras = std::sync::Arc::make_mut(chunk.saved.get_or_insert_with(Default::default));
    attachment_compound(extras.root.get(&ATTACHMENTS_KEY.into()))?;
    world.modified.insert((x, z));
    Ok(&mut extras.attachments)
}

impl Game<'_> {
    /// A guest's attachments back in their record, for `playerdata` to save.
    pub(super) fn guest_attachments_into(
        attachments: &std::collections::HashMap<u32, horizon_world::attachments::Attachments>,
        guest: &mut horizon_client::lan::guest::Guest,
    ) {
        guest.attachments = attachments.get(&guest.conn).cloned().unwrap_or_default();
    }

    /// Whether entity `id` is in the level, as a holder of attachments: one walk
    /// over the entity lists (a kind added there holds attachments too), then the
    /// kinds that are not on it.
    fn attachment_entity_exists(&self, id: u32) -> bool { let (id,) = match crate::points_generated::P2786.get() { Some(__l) => { let mut __a = (id,); mod_api::point::fire(__l, |__f| __f(self, &mut __a)); __a } None => (id,) };
        use super::entity::{Entity as _, entity_lists_of, for_each_entity};
        let mut found = false;
        for_each_entity!(entity_lists_of!(self), entity => {
            found |= entity.id() == id && !entity.is_removed();
        });
        found || self.entity_ids_off_walk().any(|e| e == id)
    }

    /// The attachments of entities that left the level, dropped: each tick
    /// while any entity has some. Only those entities are looked at, and
    /// nothing else of the entities' memory changes.
    pub(super) fn prune_entity_attachments(&mut self) { if let Some(__l) = crate::points_generated::P2785.get() { let mut __a = (); mod_api::point::fire(__l, |__f| __f(&mut *self, &mut __a)); }
        let alive = self.live_entity_ids();
        let gone: Vec<[i32; 4]> = self
            .persist
            .uuids
            .iter()
            .filter(|(id, uuid)| {
                self.entity_attachments.contains_key(*uuid) && !alive.contains(*id)
            })
            .map(|(_, uuid)| *uuid)
            .collect();
        for uuid in gone {
            self.entity_attachments.remove(&uuid);
        }
    }

    pub(super) fn native_authoritative(&self) -> bool { if let Some(__l) = crate::points_generated::P2784.get() { let mut __a = (); mod_api::point::fire(__l, |__f| __f(self, &mut __a)); }
        self.serving.is_some() || self.options.dedicated.is_some()
    }

    /// Access a holder's attachment map, to lend to a mod: a server's, whose
    /// synced changes are sent at the pass's end, or a client's copy of what
    /// its server synced.
    pub(super) fn attachment_map(
        &mut self,
        holder: mod_api::Holder,
    ) -> Result<&mut mod_api::Attachments, mod_api::AttachmentError> { if let Some(__l) = crate::points_generated::P2783.get() { let mut __a = (); mod_api::point::fire(__l, |__f| __f(&mut *self, &mut __a)); }
        if self.native_authoritative() {
            self.world.attachment_syncs.insert(holder);
            return self.attachment_map_quiet(holder);
        }
        match self.remote.as_mut() {
            Some(remote) => Ok(remote.attachments.entry(holder).or_default()),
            None => Err(mod_api::AttachmentError::MissingHolder),
        }
    }

    /// `attachment_map` without recording the holder for the sync. Inside a transaction, the
    /// map is kept as it was on its first access, so an abort puts it back.
    pub(super) fn attachment_map_quiet(
        &mut self,
        holder: mod_api::Holder,
    ) -> Result<&mut mod_api::Attachments, mod_api::AttachmentError> { if let Some(__l) = crate::points_generated::P2782.get() { let mut __a = (); mod_api::point::fire(__l, |__f| __f(&mut *self, &mut __a)); }
        if !self.native_authoritative() {
            return Err(mod_api::AttachmentError::MissingHolder);
        }
        self.journal_attachments(holder);
        self.attachment_slot(holder)
    }

    /// Where `holder`'s attachment map lives now: an entity's by its UUID, a player's where
    /// their state is (swapped in for a guest's turn, or kept beside), the world's others.
    pub(super) fn attachment_slot(
        &mut self,
        holder: mod_api::Holder,
    ) -> Result<&mut mod_api::Attachments, mod_api::AttachmentError> { if let Some(__l) = crate::points_generated::P2781.get() { let mut __a = (); mod_api::point::fire(__l, |__f| __f(&mut *self, &mut __a)); }
        use mod_api::AttachmentError;
        if let mod_api::Holder::Entity(id) = holder {
            if !self.attachment_entity_exists(id) {
                return Err(AttachmentError::MissingHolder);
            }
            attachment_compound(
                self.persist
                    .raw
                    .get(&id)
                    .and_then(|tag| tag.get(ATTACHMENTS_KEY)),
            )?;
            let uuid = self.uuid_for(id);
            return Ok(self.entity_attachments.entry(uuid).or_default());
        }
        if let mod_api::Holder::Player(uuid) = holder {
            if self.lan_acting_uuid == Some(uuid) {
                return Ok(&mut self.player_attachments);
            }
            let conn = self
                .lan
                .as_ref()
                .and_then(|host| host.hub.holder_guest(uuid, self.dim.id()))
                .map(|guest| guest.conn)
                .ok_or(AttachmentError::MissingHolder)?;
            return Ok(self.guest_attachments.entry(conn).or_default());
        }
        world_attachments(&mut self.world, holder)
    }
}

pub(super) mod generated {
    include!(concat!(env!("OUT_DIR"), "/mod_api_host_generated.rs"));
}

pub(super) fn level_side<'a>(level: &'a super::GameLevel<'_>) -> mod_api::LevelSide<'a> {
    if horizon_world::level::LevelAccess::is_client_side(level) {
        mod_api::LevelSide::Client(level)
    } else {
        mod_api::LevelSide::Server(level)
    }
}

impl mod_api::LevelHost for super::GameLevel<'_> {
    fn block_state(&self, pos: [i32; 3]) -> BlockStateId {
        horizon_world::level::LevelAccess::block_state(self, BlockPos::new(pos[0], pos[1], pos[2]))
    }
    fn state_block(&self, state: BlockStateId) -> Option<&str> {
        self.native_state_block(state)
    }
    fn state_string(&self, state: BlockStateId) -> Option<String> {
        self.registry
            .state(state)
            .map(super::native_block_states::serialize)
    }
    fn min_y(&self) -> i32 {
        self.world.shape.min_y
    }
    fn height(&self) -> i32 {
        self.world.shape.height
    }
    fn block_state_in_tag(&self, state: BlockStateId, tag: &str) -> Option<bool> {
        self.native_authoritative
            .then(|| super::native_world_read::state_in_tag(self.items, self.registry, state, tag))
    }
    fn item_stack_in_tag(&self, stack: &mod_api::ItemStack, tag: &str) -> Option<bool> {
        self.native_authoritative
            .then(|| super::native_world_read::stack_in_tag(self.items, stack, tag))
    }
    fn fluid_state(&self, pos: [i32; 3]) -> Option<mod_api::FluidState> {
        let [x, y, z] = pos;
        if !(self.world.shape.contains_y(y) && self.world.loaded(x, z)) {
            return None;
        }
        self.state_fluid_state(self.world.block(x, y, z))
    }
    fn state_fluid_state(&self, state: BlockStateId) -> Option<mod_api::FluidState> {
        super::native_world_read::state_fluid(self.behaviours, self.registry, state)
    }
    fn fluid_in_tag(&self, fluid: mod_api::FluidState, tag: &str) -> Option<bool> {
        self.native_authoritative
            .then(|| super::native_world_read::fluid_in_tag(self.items, fluid, tag))
    }
    fn state_with_property(
        &self,
        state: BlockStateId,
        key: &str,
        value: &str,
    ) -> Option<BlockStateId> {
        self.native_state_with_property(state, key, value)
    }
    fn brightness(&self, layer: mod_api::LightLayer, pos: [i32; 3]) -> i32 {
        super::native_level_refs::world_brightness(self.world, layer, pos)
    }
    fn raw_brightness(&self, pos: [i32; 3], sky_darken: i32) -> i32 {
        horizon_world::level::raw_brightness(
            self,
            BlockPos::new(pos[0], pos[1], pos[2]),
            sky_darken,
        )
    }
    fn is_loaded(&self, pos: [i32; 3]) -> bool {
        self.world.is_loaded_at(pos[0], pos[1], pos[2])
    }
    fn is_raining_at(&self, pos: [i32; 3]) -> bool {
        self.world.is_loaded_at(pos[0], pos[1], pos[2])
            && self
                .behaviours
                .is_raining_at(self, BlockPos::new(pos[0], pos[1], pos[2]))
    }
    fn enabled_features(&self) -> mod_api::FeatureFlags<'_> {
        mod_api::FeatureFlags::new(self.enabled_features)
    }
}

impl mod_api::LevelRandomHost for super::GameLevel<'_> {
    fn attachments(
        &mut self,
        holder: mod_api::Holder,
    ) -> Result<&mut mod_api::Attachments, mod_api::AttachmentError> {
        if !self.native_authoritative {
            return Err(mod_api::AttachmentError::MissingHolder);
        }
        self.world.attachment_syncs.insert(holder);
        world_attachments(self.world, holder)
    }
    fn random(&mut self) -> Option<&mut mod_api::LegacyRandom> {
        self.native_level_random()
    }
}

impl mod_api::LevelHost for super::native_hooks::HitLevel<'_> {
    fn block_state(&self, pos: [i32; 3]) -> BlockStateId {
        self.0.block(pos)
    }
    fn state_block(&self, state: BlockStateId) -> Option<&str> {
        self.0.registry().state(state).map(|s| &*s.name)
    }
    fn state_string(&self, state: BlockStateId) -> Option<String> {
        self.0
            .registry()
            .state(state)
            .map(super::native_block_states::serialize)
    }
    fn min_y(&self) -> i32 {
        self.0.shape().min_y
    }
    fn height(&self) -> i32 {
        self.0.shape().height
    }
    fn state_with_property(
        &self,
        state: BlockStateId,
        key: &str,
        value: &str,
    ) -> Option<BlockStateId> {
        super::native_hooks::with_property(self.0.registry(), state, key, value)
    }
}

impl mod_api::LevelRandomHost for super::native_hooks::HitLevel<'_> {
    fn attachments(
        &mut self,
        holder: mod_api::Holder,
    ) -> Result<&mut mod_api::Attachments, mod_api::AttachmentError> {
        self.0.attachments(holder)
    }
    fn random(&mut self) -> Option<&mut mod_api::LegacyRandom> {
        Some(self.0.random())
    }
}

pub(super) struct LevelTicks<'a, L> {
    pub level: L,
    pub ticks: &'a mut Ticks,
}

impl<'a: 'b, 'b, L: std::ops::Deref<Target = super::GameLevel<'a>>> mod_api::LevelHost
    for LevelTicks<'b, L>
{
    fn block_state(&self, pos: [i32; 3]) -> BlockStateId {
        mod_api::LevelHost::block_state(&*self.level, pos)
    }
    fn state_block(&self, state: BlockStateId) -> Option<&str> {
        self.level.registry.state(state).map(|s| &*s.name)
    }
    fn state_string(&self, state: BlockStateId) -> Option<String> {
        mod_api::LevelHost::state_string(&*self.level, state)
    }
    fn min_y(&self) -> i32 {
        mod_api::LevelHost::min_y(&*self.level)
    }
    fn height(&self) -> i32 {
        mod_api::LevelHost::height(&*self.level)
    }
    fn block_state_in_tag(&self, state: BlockStateId, tag: &str) -> Option<bool> {
        mod_api::LevelHost::block_state_in_tag(&*self.level, state, tag)
    }
    fn item_stack_in_tag(&self, stack: &mod_api::ItemStack, tag: &str) -> Option<bool> {
        mod_api::LevelHost::item_stack_in_tag(&*self.level, stack, tag)
    }
    fn fluid_state(&self, pos: [i32; 3]) -> Option<mod_api::FluidState> {
        mod_api::LevelHost::fluid_state(&*self.level, pos)
    }
    fn state_fluid_state(&self, state: BlockStateId) -> Option<mod_api::FluidState> {
        mod_api::LevelHost::state_fluid_state(&*self.level, state)
    }
    fn fluid_in_tag(&self, fluid: mod_api::FluidState, tag: &str) -> Option<bool> {
        mod_api::LevelHost::fluid_in_tag(&*self.level, fluid, tag)
    }
    fn state_with_property(
        &self,
        state: BlockStateId,
        key: &str,
        value: &str,
    ) -> Option<BlockStateId> {
        mod_api::LevelHost::state_with_property(&*self.level, state, key, value)
    }
    fn brightness(&self, layer: mod_api::LightLayer, pos: [i32; 3]) -> i32 {
        mod_api::LevelHost::brightness(&*self.level, layer, pos)
    }
    fn raw_brightness(&self, pos: [i32; 3], sky_darken: i32) -> i32 {
        mod_api::LevelHost::raw_brightness(&*self.level, pos, sky_darken)
    }
    fn is_loaded(&self, pos: [i32; 3]) -> bool {
        mod_api::LevelHost::is_loaded(&*self.level, pos)
    }
    fn is_raining_at(&self, pos: [i32; 3]) -> bool {
        mod_api::LevelHost::is_raining_at(&*self.level, pos)
    }
    fn enabled_features(&self) -> mod_api::FeatureFlags<'_> {
        mod_api::LevelHost::enabled_features(&*self.level)
    }
}

impl<'a: 'b, 'b> mod_api::LevelRandomHost for LevelTicks<'b, &mut super::GameLevel<'a>> {
    fn attachments(
        &mut self,
        holder: mod_api::Holder,
    ) -> Result<&mut mod_api::Attachments, mod_api::AttachmentError> {
        mod_api::LevelRandomHost::attachments(&mut *self.level, holder)
    }
    fn random(&mut self) -> Option<&mut mod_api::LegacyRandom> {
        mod_api::LevelRandomHost::random(&mut *self.level)
    }
}

impl<'a: 'b, 'b, L: std::ops::Deref<Target = super::GameLevel<'a>>> mod_api::LevelTicksHost
    for LevelTicks<'b, L>
{
    fn schedule_block_tick(&mut self, pos: [i32; 3], block: &str, delay: i32) -> bool {
        self.level
            .native_schedule_block_tick(self.ticks, pos, block, delay)
    }
    fn schedule_fluid_tick(&mut self, pos: [i32; 3], fluid: &str, delay: i32) -> bool {
        self.level
            .native_schedule_fluid_tick(self.ticks, pos, fluid, delay)
    }
}

impl<'a: 'b, 'b> mod_api::LevelWriteHost for LevelTicks<'b, &mut super::GameLevel<'a>> {
    fn set_block(
        &mut self,
        pos: [i32; 3],
        state: BlockStateId,
        flags: mod_api::NativeBlockUpdateFlags,
    ) -> bool {
        self.level
            .native_set_block(self.ticks, pos, state, flags.bits())
    }

    fn set_block_property(&mut self, pos: [i32; 3], key: &str, value: &str, flags: i32) -> bool {
        self.level
            .native_set_block_property(self.ticks, pos, key, value, flags)
    }

    fn play_sound(
        &mut self,
        sound: &str,
        at: [f64; 3],
        source: usize,
        volume: f32,
        pitch: f32,
    ) -> bool {
        self.level
            .native_play_sound(sound, at, source, volume, pitch)
    }

    fn send_particles(
        &mut self,
        options: &mod_api::ParticleOptions,
        state: Option<BlockStateId>,
        at: [f64; 3],
        spread: [f64; 3],
        speed: f64,
        count: i32,
    ) -> bool {
        self.level
            .native_send_particles(options, state, at, spread, speed, count)
    }

    fn level_event(&mut self, id: i32, pos: [i32; 3], data: i32) -> bool {
        self.level.native_level_event(id, pos, data)
    }
}
