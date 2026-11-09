//! Call-scoped access to an actual lent body. Actor permission alone is not
//! such a provider, and no payload is copied into an identity or retained row.
use super::{minecarts::Minecart, native_base_read_rows::BaseReadSource};
use horizon_client::{join_seq::residency::write_queue::Target, mobs::spawner::Spawner};

/// Actual values borrowed at this callback. A separately lent spawner overrides
/// the temporarily empty cart.spawner; it is not a saved or reconstructed copy.
#[derive(Clone, Copy)]
pub(super) struct LentEntityReadSource<'a> {
    pub(super) body: BaseReadSource<'a>,
    pub(super) cart_spawner: Option<&'a Spawner>,
}

/// A host may expose only a concrete loan it actually received. The requested
/// address has already passed admission; matching it selects ownership, not a
/// second handle/UUID/family validation. A match invokes visit exactly once.
/// The higher-ranked visitor cannot retain a body reference after this call.
pub(super) trait EntityBodyProvider {
    fn with_read_source(
        &self,
        address: Target,
        visit: &mut dyn FnMut(LentEntityReadSource<'_>),
    ) -> bool;
}

/// The cart and its evolving spawner are borrowed together only for the
/// callback. Construct this from the cart's canonical actor address and actual
/// local values, never the old Game cart or cart.spawner while it is taken.
pub(super) struct CartSpawnerBodyProvider<'a> {
    address: Target,
    cart: &'a Minecart,
    spawner: &'a Spawner,
}
impl<'a> CartSpawnerBodyProvider<'a> {
    pub(super) fn new(address: Target, cart: &'a Minecart, spawner: &'a Spawner) -> Self {
        Self {
            address,
            cart,
            spawner,
        }
    }
}
impl EntityBodyProvider for CartSpawnerBodyProvider<'_> {
    fn with_read_source(
        &self,
        address: Target,
        visit: &mut dyn FnMut(LentEntityReadSource<'_>),
    ) -> bool {
        if address != self.address {
            return false;
        }
        visit(LentEntityReadSource {
            body: BaseReadSource::Cart(self.cart),
            cart_spawner: Some(self.spawner),
        });
        true
    }
}
