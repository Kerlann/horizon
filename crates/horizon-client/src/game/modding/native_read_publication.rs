//! Source-lane publication. Capture all projections before borrowing the identity table:
//! relationship resolution may itself consult that table.
use super::{
    Game,
    commands::Target,
    native_base_read_rows::BaseReadSource,
    native_base_read_source::{BaseReadOwner, snapshot_base_read_source},
    native_entity_read_cache::{NativeReadDetails, NativeReadPublication},
    native_mob_read_context::NativeMobTargetFacts,
};
use horizon_client::{
    join_seq::{
        Joined,
        residency::{Kind, Owner, read_pass::ReadScope, table::Location},
    },
    mobs::{Mob, MobWorld},
};
use mod_api::*;

impl Game<'_> {
    /// The caller lends the actor from this level, including when its list is
    /// physically lifted. GoalHost calls this at a real callback/write boundary.
    pub(super) fn publish_lent_mob_reads(
        &self,
        source: &Mob,
        _world: &MobWorld<'_>,
        phase: EntityReadCapturePhase,
    ) -> bool {
        self.publish_mob_read_source(source, phase)
    }

    pub(super) fn publish_mob_read_source(
        &self,
        source: &Mob,
        phase: EntityReadCapturePhase,
    ) -> bool {
        self.publish_mob_read_source_at(
            source,
            Location {
                source: Default::default(),
                owner: Owner {
                    dimension: self.dim.key(),
                    kind: Kind::Mob,
                },
                join: source.join_seq(),
                source: Default::default(),
            },
            phase,
        )
    }

    pub(super) fn publish_mob_read_source_at(
        &self,
        source: &Mob,
        location: Location,
        phase: EntityReadCapturePhase,
    ) -> bool {
        if source.removed || !self.is_logical_server() {
            return false;
        }
        let (Some(epoch), Some(tick)) = (
            self.native_lifecycle.epoch(),
            self.native_lifecycle.capture_tick(),
        ) else {
            return false;
        };
        let level = LevelRef::new(epoch, location.owner.dimension);
        let owner = location.owner;
        if owner.kind != Kind::Mob || location.join != source.join_seq() {
            return false;
        }
        let parked = if owner.dimension == self.dim.key() {
            None
        } else {
            let Some(parked) = self.levels.parked_of(level.dimension_id()) else {
                return false;
            };
            Some(parked)
        };
        let source_world = if parked.is_some() {
            let Some(world) = self.levels.parked_world_of(level.dimension_id()) else {
                return false;
            };
            world
        } else {
            &self.world
        };
        let clock = self.entity_read_state.borrow().passes.clone();
        let current = clock.borrow_mut().stamp_active_owner(owner, phase);
        let mut scope = None;
        let capture = match current {
            Ok(Some(capture)) => capture,
            Ok(None) => {
                // Opening callbacks can occur outside the ordinary owner pass.
                // Their one-source publication still has a real scoped clock.
                scope = match ReadScope::begin(clock, true, owner, tick) {
                    Ok(scope) => scope,
                    Err(error) => {
                        log::error!("cannot open entity read publication: {error:?}");
                        return false;
                    }
                };
                let Some(active) = &scope else {
                    return false;
                };
                match active.stamp(phase) {
                    Ok(capture) => capture,
                    Err(error) => {
                        log::error!("cannot stamp entity read publication: {error:?}");
                        return false;
                    }
                }
            }
            Err(error) => {
                log::error!("cannot stamp active entity read publication: {error:?}");
                return false;
            }
        };
        let entity = EntityRef::new(epoch, source.id);
        let publication = {
            let mut state = self.entity_read_state.borrow_mut();
            let state = &mut *state;
            let relations = state.mob_relations.entry(level).or_default();
            if relations.tick != Some(tick) {
                self.rebuild_mob_read_relations(level, &mut relations.index);
                relations.tick = Some(tick);
            }
            relations.index.update_mob_rider(source);
            let resolve = |id| self.read_resident_identity(id);
            let player_leash = |uuid| self.resolve_mob_read_player(uuid, level, &relations.index);
            let pearl_cooldown = |id| self.pearl_portal_cooldown(id);
            let type_facts = |kind: &str| self.res.entity_type_geometry().get(kind);
            let context = BaseReadOwner {
                level,
                epoch,
                logical_server: true,
                resolve: &resolve,
                player_leash: &player_leash,
                pearl_cooldown: &pearl_cooldown,
                type_facts: &type_facts,
                default_player_uuid: self
                    .lan_acting_uuid
                    .map(horizon_client::lan::playerdata::uuid_ints)
                    .or(self.level.player_uuid),
                local_rotation: [0.0; 2],
                local_head_yaw: 0.0,
                local_spectator: false,
                local_invisible: None,
                client_shift: None,
                player_portal_cooldown: None,
            };
            let base = match parked {
                Some(parked) => {
                    let Some(raw) = parked.resident_read_source(location) else {
                        return false;
                    };
                    snapshot_base_read_source(
                        raw.source,
                        raw.id,
                        raw.persist,
                        raw.portals,
                        &mut state.kind_pool,
                        &context,
                    )
                }
                None => snapshot_base_read_source(
                    BaseReadSource::Mob(source),
                    source.id,
                    &self.persist,
                    &self.portals.entities,
                    &mut state.kind_pool,
                    &context,
                ),
            };
            let Some(base) = base else {
                return false;
            };
            let mob_context = self.mob_read_context(source, level, &relations.index);
            let mob = self.capture_mob_read_row(capture, entity, source, mob_context);
            let (kill_credit, last_attacker) = self.raw_mob_living_relations(
                source,
                level,
                &relations.index,
                self.last_hurt_by_player.get(&source.id).copied(),
            );
            let Some(living) = self.snapshot_mob_living_read_row(
                capture,
                LivingRef::new(entity),
                (source, source_world),
                kill_credit,
                last_attacker,
                self.raw_mob_item_use_remaining(source),
                // The direct-source mob producer always supplies this known bool.
                mob.can_use_portal.unwrap(),
            ) else {
                return false;
            };
            let merchant_context = self.merchant_read_context(source, epoch, &relations.index);
            let merchant = self.capture_merchant_read_row(
                capture,
                entity,
                source,
                merchant_context,
                &mut state.identities,
            );
            NativeReadPublication {
                silent_overlay: Default::default(),
                capture,
                base,
                details: NativeReadDetails::Mob {
                    living,
                    mob,
                    merchant,
                },
                join_generation: source.join_seq().get(),
                target: Target::Mob(source.id),
                mob_target_facts: Some(NativeMobTargetFacts::capture(source)),
            }
        };
        let published = self.entity_residency.borrow_mut().publish_rows_merging(
            [(source.id, location, publication)],
            NativeReadPublication::merge_pending_silent,
        ) == 1;
        drop(scope);
        published
    }
}
