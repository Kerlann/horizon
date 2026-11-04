//! The `merchant_offer_traded` hooks and helpers: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// immediately after this call. Server-side.
    /// A trade of a villager or a wandering trader was made and its `offer`
    /// (the live one in the trading session, not a copy) is in the state the
    /// trade left it in: `uses` counted. A mod may change it (Bukkit's
    /// `PlayerTradeEvent#getTrade` is mutable): `uses`, `max_uses`, `demand`,
    /// `special_price_diff`, `price_multiplier`, `xp`. The change stands: the
    /// next click and the offers sent to the player see it. Once per trade (a
    /// shift-click that trades several times asks for each trade only after
    /// the click, so the stock it ran down in that click is not given back
    /// before the click ends), before the trade's statistic and
    /// `trade_with_villager`, which hear the offer as the trade left it.
    /// `merchant` is the trader's entity id, `kind` its type. Not asked for a
    /// trade window without a trader. Server-side.
    #[hook(
        side = "server",
        contract = "collect",
        out = "offer",
        neoforge = "none (NeoForge has no such event; Bukkit's PlayerTradeEvent#getTrade is mutable)",
        cancellable = false
    )]
    fn merchant_offer_traded();
}
