//! Fluid held by items (arismoko/horizon#41 §7): [`ITEM_FLUIDS`], NeoForge 26.3's
//! `Capabilities.Fluid.ITEM`, reached where the item's stack lives, an [`ItemPlace`], as
//! [`ITEM_ITEMS`](super::ITEM_ITEMS) is. A handler acts on one item of the stack and may swap it
//! for another (an empty bucket becomes a water bucket): every write goes through
//! [`set_place_stack`](dyn Host::set_place_stack) with `one_by_one`, so from a stack of more
//! than one the changed item is split off, and a write that can't place it moves nothing.
use super::{FluidStack, Host, ItemCapabilityKey, ItemPlace};
use std::{
    marker::PhantomData,
    sync::{Arc, LazyLock},
};

/// Fluid held by an item, as NeoForge 26.3's `Capabilities.Fluid.ITEM`. The game answers it for
/// vanilla's empty, water and lava buckets and for a mod's containers (`item_fluid_tank`).
pub const ITEM_FLUIDS: ItemCapabilityKey<dyn StackFluidHandler> = ItemCapabilityKey {
    name: "horizon:item_fluids",
    default: Some(|host, stack| {
        static GAME: LazyLock<Arc<dyn StackFluidHandler>> =
            LazyLock::new(|| Arc::new(GameItemFluids));
        (host.item_fluid_tanks(stack) > 0).then(|| GAME.clone())
    }),
    handler: PhantomData,
};

/// The tanks of the item at `at`, numbered from 0, in millibuckets. A change rewrites the item at
/// `at` through [`set_place_stack`](dyn Host::set_place_stack) with `one_by_one`; when that hands
/// the item back, nothing changed, and the handler reports nothing moved. `simulate` changes
/// nothing and must answer exactly as the real call would, in amount and in fluid and
/// components, as [`FluidHandler`](super::FluidHandler)'s must: pass it on to `set_place_stack`.
pub trait StackFluidHandler: Send + Sync + 'static {
    fn tanks(&self, host: &mut dyn Host, at: &ItemPlace) -> usize;
    /// What tank `tank` holds.
    fn fluid(&self, host: &mut dyn Host, at: &ItemPlace, tank: usize) -> Option<FluidStack>;
    /// How much of `fluid` tank `tank` holds when empty; 0 where it may not go in.
    fn capacity(&self, host: &mut dyn Host, at: &ItemPlace, tank: usize, fluid: &FluidStack)
    -> u32;
    /// Puts as much of `fluid` as tank `tank` takes; what didn't go in.
    fn insert(
        &self,
        host: &mut dyn Host,
        at: &ItemPlace,
        tank: usize,
        fluid: FluidStack,
        simulate: bool,
    ) -> Option<FluidStack>;
    /// Takes up to `max` out of tank `tank`; what came out.
    fn extract(
        &self,
        host: &mut dyn Host,
        at: &ItemPlace,
        tank: usize,
        max: u32,
        simulate: bool,
    ) -> Option<FluidStack>;
}

/// The game's own [`ITEM_FLUIDS`], over the `item_fluid` helpers: one tank.
struct GameItemFluids;
impl StackFluidHandler for GameItemFluids {
    fn tanks(&self, host: &mut dyn Host, at: &ItemPlace) -> usize {
        host.place_stack(at)
            .map_or(0, |held| host.item_fluid_tanks(&held))
    }
    fn fluid(&self, host: &mut dyn Host, at: &ItemPlace, tank: usize) -> Option<FluidStack> {
        let held = host.place_stack(at).filter(|_| tank == 0)?;
        host.item_fluid_held(&held)
    }
    fn capacity(
        &self,
        host: &mut dyn Host,
        at: &ItemPlace,
        tank: usize,
        fluid: &FluidStack,
    ) -> u32 {
        host.place_stack(at)
            .filter(|_| tank == 0)
            .map_or(0, |held| host.item_fluid_capacity(&held, fluid))
    }
    fn insert(
        &self,
        host: &mut dyn Host,
        at: &ItemPlace,
        tank: usize,
        fluid: FluidStack,
        simulate: bool,
    ) -> Option<FluidStack> {
        let Some((went, changed)) = host
            .place_stack(at)
            .filter(|_| tank == 0)
            .and_then(|held| host.item_fluid_filled(&held, &fluid))
        else {
            return Some(fluid);
        };
        if went == 0 || host.set_place_stack(at, changed, true, simulate).is_some() {
            return Some(fluid);
        }
        (went < fluid.amount).then(|| fluid.with_amount(fluid.amount - went))
    }
    fn extract(
        &self,
        host: &mut dyn Host,
        at: &ItemPlace,
        tank: usize,
        max: u32,
        simulate: bool,
    ) -> Option<FluidStack> {
        let held = host.place_stack(at).filter(|_| tank == 0)?;
        let (out, changed) = host.item_fluid_drained(&held, max)?;
        if host.set_place_stack(at, changed, true, simulate).is_some() {
            return None;
        }
        Some(out)
    }
}
