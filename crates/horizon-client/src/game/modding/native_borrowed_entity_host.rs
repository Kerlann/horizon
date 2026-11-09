//! A read Host for one synchronous callback with a concrete actor loan.
//! Area forwarding is generated beside Game's forwarding, not copied here.
use super::{
    Game, minecarts::CartKind, native_base_read_rows::BaseReadSource,
    native_entity_body_provider::EntityBodyProvider, native_minecart_read_source::MinecartRead,
};
use horizon_client::mobs::spawner::Spawner;
use mod_api::{EntityRef, Host};

pub(super) struct BorrowedEntityHost<'a, 'w> {
    pub(super) game: &'a Game<'w>,
    provider: &'a dyn EntityBodyProvider,
}
impl<'a, 'w> BorrowedEntityHost<'a, 'w> {
    pub(super) fn new(game: &'a Game<'w>, provider: &'a dyn EntityBodyProvider) -> Self {
        Self { game, provider }
    }

    /// The canonical actor address proves membership for this immutable Host
    /// call. Epoch/id validate the incoming handle; the provider supplies the
    /// actual body. Neither an actor frame nor a read scope substitutes for it.
    /// None (outer) forwards a foreign read; Some(None) is ordinary wrong-family
    /// or subtype absence. No payload or body reference escapes the visitor.
    pub(super) fn with_owned_cart<R>(
        &self,
        entity: EntityRef,
        required_kind: Option<CartKind>,
        read: impl for<'s> FnOnce(MinecartRead<'s, 'w>, Option<&'s Spawner>) -> R,
    ) -> Option<Option<R>> {
        let actor = self.game.active_entity_writer()?.actor;
        if actor.runtime_id != entity.id() {
            return None;
        }
        let mut read = Some(read);
        let mut result = None;
        self.provider.with_read_source(actor, &mut |source| {
            if self.game.native_lifecycle.epoch() != Some(entity.epoch()) {
                result = Some(None);
                return;
            }
            let BaseReadSource::Cart(cart) = source.body else {
                result = Some(None);
                return;
            };
            if required_kind.is_some_and(|kind| kind != cart.kind) {
                result = Some(None);
                return;
            }
            let read = read.take().expect("a body provider visits its source once");
            result = Some(Some(read(
                MinecartRead {
                    game: self.game,
                    cart,
                },
                source.cart_spawner,
            )));
        });
        result
    }
}
impl mod_api::Backend for BorrowedEntityHost<'_, '_> {}
impl Host for BorrowedEntityHost<'_, '_> {
    fn backend(&self) -> Option<mod_api::BackendRef<'_>> {
        Some(mod_api::__private::backend_ref(self))
    }
}
