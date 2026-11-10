use super::*;

#[test]
fn a_candle_on_an_uneaten_cake_makes_a_candle_cake() {
    // CakeBlock.useItemOn: any candle on a cake with no bites turns it into the
    // matching unlit candle cake; a bitten cake does not.
    let mut w = world!();
    let pos = BlockPos::new(0, 1, 0);
    let cake = w.id("cake", &[("bites", "0")]);
    for (item, block) in [
        ("minecraft:candle", "candle_cake"),
        ("minecraft:red_candle", "red_candle_cake"),
    ] {
        w.put(pos, cake);
        assert_eq!(w.use_item(pos, item), InteractionResult::Success);
        assert_eq!(w.get(pos), w.id(block, &[("lit", "false")]));
    }
    let bitten = w.id("cake", &[("bites", "1")]);
    w.put(pos, bitten);
    w.use_item(pos, "minecraft:candle");
    assert_ne!(w.get(pos), w.id("candle_cake", &[("lit", "false")]));
    w.put(pos, cake);
    // Anything else answers TRY_WITH_EMPTY_HAND, and ServerPlayerGameMode
    // then calls useWithoutItem with whatever the main hand holds, so a stone
    // eats a slice of the cake instead of adding a candle to it.
    assert_eq!(
        w.use_item(pos, "minecraft:stone"),
        InteractionResult::Success
    );
    assert_eq!(w.get(pos), bitten);
}
