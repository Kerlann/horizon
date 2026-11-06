//! Scoped, transaction-safe cauldron interaction resources.
use crate::{BlockKey, BlockStateId, Identifier, ItemStack, NativeStatType, StateConstraint};

/// A failure to queue a transactional interaction effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum EffectError {
    /// The sound identifier or parameters are invalid.
    InvalidSound,
    /// The statistic identifier is invalid.
    InvalidStat,
    /// The effect cannot be queued.
    Unavailable,
}

/// A cauldron handler could not complete atomically.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum CauldronInteractionError {
    /// The interaction or its player is no longer available.
    Stale,
    /// The target block is outside the resolved cauldron family.
    OutsideCauldronFamily,
    /// The replacement state's properties are invalid.
    InvalidState,
    /// The requested stack is invalid.
    InvalidStack,
    /// The held stack has fewer items than requested.
    InsufficientItems,
    /// A handler returned Pass after making a write.
    WriteOnPass,
    /// An interaction effect could not be queued.
    Effect(EffectError),
    /// The callback could not execute, including a trapped wasm group.
    Unavailable,
}
impl From<(Identifier, String)> for CauldronInteractionError {
    fn from(_: (Identifier, String)) -> Self {
        Self::Unavailable
    }
}

/// The result of an authoritative cauldron interaction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum CauldronOutcome {
    /// The item passes to the next interaction.
    Pass,
    /// The interaction succeeds and swings the hand.
    Success,
    /// The interaction consumes the action without a swing.
    Consume,
}

/// A client-only prediction; Unknown still sends the interaction to the server.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum CauldronPrediction {
    /// The item passes to the next interaction.
    Pass,
    /// Predict success and a hand swing.
    Success,
    /// Predict a consumed action without a swing.
    Consume,
    /// Send the use without assuming a client-side result.
    Unknown,
}

/// A frozen, read-only view for client prediction.
pub trait CauldronPredictionContext {
    /// The target block state.
    fn state(&self) -> BlockStateId;
    /// The acting hand's stack, if any.
    fn held(&self) -> Option<ItemStack>;
    /// Whether the acting player is in creative mode.
    fn creative(&self) -> bool;
}

/// The target cauldron and the acting player's hand, lent for one server call.
/// All writes and effects join the current transaction; Pass after a write is refused.
pub trait CauldronInteractionContext {
    /// The target block state.
    fn state(&self) -> BlockStateId;
    /// The acting hand's stack, if any.
    fn held(&self) -> Option<ItemStack>;
    /// Whether the acting player is in creative mode.
    fn creative(&self) -> bool;
    /// Replace this target with a valid cauldron state.
    fn set_state(
        &mut self,
        target: BlockKey,
        properties: &[StateConstraint],
    ) -> Result<(), CauldronInteractionError>;
    /// Consume from the acting hand; creative consumption is explicit to the handler.
    fn take_held(&mut self, amount: u32) -> Result<(), CauldronInteractionError>;
    /// Give the filled result using vanilla hand, inventory and overflow behavior.
    fn give(&mut self, stack: ItemStack) -> Result<(), CauldronInteractionError>;
    /// Queue a positional sound for the outermost successful commit.
    fn sound(
        &mut self,
        sound: &str,
        at: [f64; 3],
        volume: f32,
        pitch: f32,
    ) -> Result<(), EffectError>;
    /// Queue a statistic for the outermost successful commit.
    fn award_stat(
        &mut self,
        stat_type: NativeStatType,
        id: &str,
        amount: i32,
    ) -> Result<(), EffectError>;
}

/// A mod's pair-specific cauldron interaction, registered once at startup.
pub trait CauldronInteraction: Send + Sync + 'static {
    /// Predict without writes; Unknown still sends the use to the server.
    fn predict(&self, context: &dyn CauldronPredictionContext) -> CauldronPrediction;
    /// Execute atomically; errors and writes followed by Pass roll back.
    fn interact(
        &self,
        context: &mut dyn CauldronInteractionContext,
    ) -> Result<CauldronOutcome, CauldronInteractionError>;
}
