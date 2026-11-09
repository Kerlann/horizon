//! Owned API materialization from the publication retained on the authoritative
//! identity row. No accessor scans entity kinds or clones the retained payload.
use super::{
    Game,
    native_base_read_rows::CompactEntityReadRow,
    native_fixed_read_rows::{NativePlayerReadSnapshot, NativeReadIdentifiers},
    native_mob_read_rows::{NativeMerchantReadRow, NativeMobReadRow},
};
use horizon_client::join_seq::residency::read_pass::ReadPasses;
use horizon_client::join_seq::residency::table::EntityLookup;
use mod_api::*;
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
    sync::Arc,
};

#[derive(Default)]
pub(super) struct NativeReadState {
    pub(super) passes: Rc<RefCell<ReadPasses>>,
    pub(super) mob_backing: super::native_mob_backing::MobBackingRevision,
    pub(super) commands: Option<super::native_entity_commands::EntityCommands>,
    pub(super) identities: HashSet<Arc<str>>,
    pub(super) player_identifiers: NativeReadIdentifiers,
    pub(super) raids: HashMap<RaidRef, RaidReadRow>,
    pub(super) kind_pool: super::native_base_read_source::BaseReadKindPool,
    pub(super) mob_relations: HashMap<LevelRef, NativeMobRelationPass>,
}

#[derive(Default)]
pub(super) struct NativeMobRelationPass {
    pub(super) tick: Option<TickId>,
    pub(super) index: super::native_mob_read_context::NativeMobRelations,
}

impl NativeReadState {
    pub(super) fn reset(&mut self) {
        self.passes.borrow_mut().reset();
        self.mob_backing = Default::default();
        self.commands = None;
        self.identities.clear();
        self.player_identifiers = NativeReadIdentifiers::default();
        self.raids.clear();
        self.kind_pool = Default::default();
        self.mob_relations.clear();
    }
}

pub(super) struct NativeReadPublication {
    pub(super) silent_overlay: super::native_silent_overlay::SilentOverlay,
    pub(super) capture: EntityReadCapture,
    pub(super) base: CompactEntityReadRow,
    pub(super) details: NativeReadDetails,
    pub(super) join_generation: u64,
    pub(super) target: super::commands::Target,
    pub(super) mob_target_facts: Option<super::native_mob_read_context::NativeMobTargetFacts>,
}

pub(super) enum NativeReadDetails {
    Base,
    Living(LivingReadRow),
    Mob {
        living: LivingReadRow,
        mob: NativeMobReadRow,
        merchant: Option<NativeMerchantReadRow>,
    },
    Player {
        living: LivingReadRow,
        player: NativePlayerReadSnapshot,
    },
    Dragon {
        living: LivingReadRow,
        dragon: DragonReadRow,
    },
    Display(DisplayReadRow),
    Item(ItemEntityReadRow),
    Projectile(ProjectileReadRow),
    Hook(FishingHookReadRow),
}

impl NativeReadDetails {
    pub(super) fn living(&self) -> Option<&LivingReadRow> {
        match self {
            Self::Living(row)
            | Self::Mob { living: row, .. }
            | Self::Player { living: row, .. }
            | Self::Dragon { living: row, .. } => Some(row),
            _ => None,
        }
    }
}

impl Game<'_> {
    pub(super) fn with_mob_read_relations<R>(
        &self,
        level: LevelRef,
        read: impl FnOnce(&super::native_mob_read_context::NativeMobRelations) -> R,
    ) -> Option<R> {
        if self.native_lifecycle.epoch() != Some(level.epoch()) {
            return None;
        }
        let state = self.entity_read_state.borrow();
        let pass = state.mob_relations.get(&level)?;
        if pass.tick != self.native_lifecycle.capture_tick() {
            return None;
        }
        Some(read(&pass.index))
    }

    pub(super) fn read_resident_identity(
        &self,
        id: u32,
    ) -> Option<(EntityRef, horizon_client::join_seq::residency::Kind)> {
        let epoch = self.native_lifecycle.epoch()?;
        let mut table = self.entity_residency.borrow_mut();
        let (location, state, _, _) =
            table.server_admitted_read_lookup(id, |at| self.locate_server_resident_owner(at))?;
        let visible = match state {
            EntityLookup::Found {
                value: (_, visible),
                ..
            } => visible,
            EntityLookup::Taken => self.server_read_target_visible(
                location.owner.dimension,
                super::entity_residency::residency_target_at(location, id),
            ),
            EntityLookup::Missing => false,
        };
        visible.then_some((EntityRef::new(epoch, id), location.owner.kind))
    }

    pub(super) fn with_entity_read_publication<R>(
        &self,
        entity: EntityRef,
        read: impl FnOnce(&NativeReadPublication) -> Option<R>,
    ) -> Option<R> {
        if self.native_lifecycle.epoch() != Some(entity.epoch()) {
            return None;
        }
        let mut table = self.entity_residency.borrow_mut();
        let (state, cached) = table
            .server_entity_read_lookup(entity.id(), |at| self.locate_server_resident_owner(at));
        match state {
            EntityLookup::Missing
            | EntityLookup::Found {
                value: (_, false), ..
            } => None,
            EntityLookup::Taken | EntityLookup::Found { .. } => {
                let row = cached?;
                self.server_read_target_visible(row.base.level.dimension(), row.target)
                    .then_some(())?;
                read(row)
            }
        }
    }

    pub(super) fn entity_cached_row(&self, entity: EntityRef) -> Option<EntityReadRow> {
        self.with_entity_read_publication(entity, |row| {
            Some(
                row.base
                    .materialize(row.capture, entity, row.join_generation),
            )
        })
    }

    pub(super) fn living_cached_row(&self, entity: LivingRef) -> Option<LivingReadRow> {
        self.with_entity_read_publication(entity.entity(), |row| row.details.living().cloned())
    }

    pub(super) fn mob_cached_row(&self, entity: MobRef) -> Option<MobReadRow> {
        self.with_entity_read_publication(entity.entity(), |row| match &row.details {
            NativeReadDetails::Mob { mob, .. } => Some(mob.materialize()),
            _ => None,
        })
    }

    pub(super) fn merchant_cached_row(&self, entity: MerchantRef) -> Option<MerchantReadRow> {
        self.with_entity_read_publication(entity.entity(), |row| match &row.details {
            NativeReadDetails::Mob { merchant, .. } => {
                merchant.as_ref().map(NativeMerchantReadRow::materialize)
            }
            _ => None,
        })
    }

    pub(super) fn player_cached_row(&self, entity: PlayerRef) -> Option<PlayerReadRow> {
        self.with_entity_read_publication(entity.entity(), |row| match &row.details {
            NativeReadDetails::Player { player, .. } if player.1 == entity => {
                Some(player.materialize())
            }
            _ => None,
        })
    }

    pub(super) fn dragon_cached_row(&self, entity: DragonRef) -> Option<DragonReadRow> {
        self.with_entity_read_publication(entity.entity(), |row| match &row.details {
            NativeReadDetails::Dragon { dragon, .. } => Some(dragon.clone()),
            _ => None,
        })
    }

    pub(super) fn display_cached_row(&self, entity: DisplayRef) -> Option<DisplayReadRow> {
        self.with_entity_read_publication(entity.entity(), |row| match &row.details {
            NativeReadDetails::Display(display) => Some(display.clone()),
            _ => None,
        })
    }

    pub(super) fn item_entity_cached_row(
        &self,
        entity: ItemEntityRef,
    ) -> Option<ItemEntityReadRow> {
        self.with_entity_read_publication(entity.entity(), |row| match &row.details {
            NativeReadDetails::Item(item) => Some(item.clone()),
            _ => None,
        })
    }

    pub(super) fn projectile_cached_row(&self, entity: ProjectileRef) -> Option<ProjectileReadRow> {
        self.with_entity_read_publication(entity.entity(), |row| match &row.details {
            NativeReadDetails::Projectile(projectile) => Some(projectile.clone()),
            _ => None,
        })
    }

    pub(super) fn raid_cached_row(&self, raid: RaidRef) -> Option<RaidReadRow> {
        self.level_raid(raid.level(), raid.id())?;
        self.entity_read_state.borrow().raids.get(&raid).cloned()
    }

    pub(super) fn fishing_hook_cached_row(
        &self,
        entity: FishingHookRef,
    ) -> Option<FishingHookReadRow> {
        self.with_entity_read_publication(entity.entity(), |row| match &row.details {
            NativeReadDetails::Hook(hook) => Some(hook.clone()),
            _ => None,
        })
    }
}
