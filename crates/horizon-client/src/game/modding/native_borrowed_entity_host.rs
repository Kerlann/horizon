//! A read Host for one synchronous callback with a concrete actor loan.
//! Area forwarding is generated beside Game's forwarding, not copied here.
use super::{Game, native_entity_body_provider::EntityBodyProvider};
use mod_api::Host;

pub(super) struct BorrowedEntityHost<'a, 'w> {
    pub(super) game: &'a Game<'w>,
    pub(super) provider: &'a dyn EntityBodyProvider,
}
impl<'a, 'w> BorrowedEntityHost<'a, 'w> {
    pub(super) fn new(game: &'a Game<'w>, provider: &'a dyn EntityBodyProvider) -> Self {
        Self { game, provider }
    }
}
impl mod_api::Backend for BorrowedEntityHost<'_, '_> {}
impl Host for BorrowedEntityHost<'_, '_> {
    fn backend(&self) -> Option<mod_api::BackendRef<'_>> {
        Some(mod_api::__private::backend_ref(self))
    }
}
