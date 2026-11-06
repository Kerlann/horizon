//! Scoped context for a registered cauldron interaction.
use super::{Game, transactions::DeferredEffect};
use horizon_client::items::ItemStack;
use mod_api::{
    BlockKey, BlockStateId, CauldronInteractionContext, CauldronInteractionError as Error,
    CauldronPredictionContext, EffectError, NativeStatType, StateConstraint,
};

pub(super) struct Context<'g, 'a> {
    pub game: &'g mut Game<'a>,
    pub pos: [i32; 3],
    pub slot: usize,
    pub player: u128,
    pub wrote: bool,
}
impl CauldronInteractionContext for Context<'_, '_> {
    fn state(&self) -> BlockStateId {
        self.game.world.block(self.pos[0], self.pos[1], self.pos[2])
    }
    fn held(&self) -> Option<ItemStack> {
        self.game.stack_in(self.slot).cloned()
    }
    fn creative(&self) -> bool {
        self.game.creative
    }
    fn set_state(&mut self, target: BlockKey, properties: &[StateConstraint]) -> Result<(), Error> {
        let resolved = mod_api::__private::resolved_block(target).ok_or(Error::Stale)?;
        let registry = self.game.pack.assets.registry;
        let mut state = registry
            .state(resolved.default)
            .cloned()
            .ok_or(Error::Stale)?;
        if !horizon_client::cauldron::vanilla_block(&state.name)
            && !mod_api::__private::parts("cauldron").cauldron_family_contains(resolved.default)
        {
            return Err(Error::OutsideCauldronFamily);
        }
        let definition = self
            .game
            .pack
            .block_registries
            .blocks
            .get(&state.name)
            .ok_or(Error::Stale)?;
        super::native_block_states::match_state_properties(
            definition,
            true,
            |_, _| true,
            properties,
        )
        .map_err(|_| Error::InvalidState)?;
        for (i, p) in properties.iter().enumerate() {
            if properties[..i].iter().any(|earlier| earlier.name == p.name) {
                return Err(Error::InvalidState);
            }
            state.properties.insert(p.name.clone(), p.value.clone());
        }
        let state = registry.resolve(&state).map_err(|_| Error::InvalidState)?;
        if !self.game.journal_set_block(self.pos, state) {
            return Err(Error::Stale);
        }
        self.wrote = true;
        Ok(())
    }
    fn take_held(&mut self, amount: u32) -> Result<(), Error> {
        let held = self.game.stack_in_mut(self.slot);
        let stack = held.as_mut().ok_or(Error::InsufficientItems)?;
        if stack.count < amount {
            return Err(Error::InsufficientItems);
        }
        stack.count -= amount;
        if stack.count == 0 {
            *held = None;
        }
        self.wrote |= amount != 0;
        Ok(())
    }
    fn give(&mut self, stack: ItemStack) -> Result<(), Error> {
        if stack.count == 0 || self.game.items().get(&stack.item).is_none() {
            return Err(Error::InvalidStack);
        }
        let mut hand = super::powder_snow::HeldSlot {
            game: self.game,
            slot: self.slot,
            dropper: Some(self.player),
        };
        horizon_client::cauldron::create_filled_result(&mut hand, stack, true);
        self.wrote = true;
        Ok(())
    }
    fn sound(
        &mut self,
        sound: &str,
        at: [f64; 3],
        volume: f32,
        pitch: f32,
    ) -> Result<(), EffectError> {
        if !super::native_hooks::mod_sound_ok(sound, at, 0, volume, pitch) {
            return Err(EffectError::InvalidSound);
        }
        self.game.defer_effect(DeferredEffect::OwnedSound {
            sound: sound.into(),
            at,
            volume,
            pitch,
        });
        self.wrote = true;
        Ok(())
    }
    fn award_stat(
        &mut self,
        stat_type: NativeStatType,
        id: &str,
        amount: i32,
    ) -> Result<(), EffectError> {
        if horizon_world::identifier::Identifier::parse_namespaced(id).is_none() {
            return Err(EffectError::InvalidStat);
        }
        self.game.defer_effect(DeferredEffect::Stat {
            player: self.player,
            stat_type,
            id: id.into(),
            amount,
        });
        self.wrote = true;
        Ok(())
    }
}

pub(super) struct Prediction {
    pub state: BlockStateId,
    pub held: Option<ItemStack>,
    pub creative: bool,
}
impl CauldronPredictionContext for Prediction {
    fn state(&self) -> BlockStateId {
        self.state
    }
    fn held(&self) -> Option<ItemStack> {
        self.held.clone()
    }
    fn creative(&self) -> bool {
        self.creative
    }
}
