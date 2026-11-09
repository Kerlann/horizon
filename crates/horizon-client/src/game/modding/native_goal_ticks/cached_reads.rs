//! Explicit OWN cached-row materialization uses the same publication as every
//! foreign reader. Callback entry and successful apply refresh it, not getters.
use super::GoalCx;
use mod_api::*;

impl GoalCx<'_, '_> {
    pub(super) fn r_entity_cached_row(&self, entity: EntityRef) -> Option<EntityReadRow> {
        mod_api::__private::backend_read(self.host, |backend| backend.entity_cached_row(entity))
            .flatten()
    }
    pub(super) fn r_living_cached_row(&self, living: LivingRef) -> Option<LivingReadRow> {
        mod_api::__private::backend_read(self.host, |backend| backend.living_cached_row(living))
            .flatten()
    }
    pub(super) fn r_mob_cached_row(&self, mob: MobRef) -> Option<MobReadRow> {
        mod_api::__private::backend_read(self.host, |backend| backend.mob_cached_row(mob)).flatten()
    }
    pub(super) fn r_merchant_cached_row(&self, merchant: MerchantRef) -> Option<MerchantReadRow> {
        mod_api::__private::backend_read(self.host, |backend| backend.merchant_cached_row(merchant))
            .flatten()
    }
}
