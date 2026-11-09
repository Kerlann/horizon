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

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_client::join_seq::{
        JoinList,
        residency::{Kind, Owner, table::Location},
    };
    use horizon_core::geometry::Vec3;

    // Internal composition contract, not public Spawner-component API coverage.
    #[test]
    fn internal_cart_provider_lends_the_exact_separate_evolving_component() {
        let carts: JoinList<Minecart, false> = [
            Minecart::new(71, super::super::minecarts::CartKind::Spawner, Vec3::ZERO),
            Minecart::new(72, super::super::minecarts::CartKind::Rideable, Vec3::ZERO),
        ]
        .into_iter()
        .collect();
        let mut body = carts[0].clone();
        let mut spawner = body.spawner.take().unwrap();
        spawner.spawn_delay = 37;
        let address = Target {
            runtime_id: body.id,
            location: Location {
                source: horizon_client::join_seq::residency::Source::Ordinary,
                owner: Owner {
                    dimension: mod_api::DimensionKey::OVERWORLD,
                    kind: Kind::Cart,
                },
                join: body.join,
            },
        };
        let provider = CartSpawnerBodyProvider::new(address, &body, &spawner);
        let mut visits = 0;
        assert!(provider.with_read_source(address, &mut |source| {
            visits += 1;
            let BaseReadSource::Cart(actual) = source.body else {
                panic!("wrong supplied body");
            };
            assert!(std::ptr::eq(actual, &body));
            assert!(actual.spawner.is_none());
            let component = source.cart_spawner.unwrap();
            assert!(std::ptr::eq(component, &*spawner));
            assert_eq!(component.spawn_delay, 37);
        }));
        assert_eq!(visits, 1);
        let foreign = Target {
            runtime_id: 72,
            location: Location {
                join: carts[1].join,
                ..address.location
            },
        };
        assert!(
            !provider.with_read_source(foreign, &mut |_| panic!("foreign actor received the loan"))
        );
    }
}
