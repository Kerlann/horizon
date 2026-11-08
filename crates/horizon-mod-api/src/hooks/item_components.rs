pub trait Notes {
    #[notes = r#"## Equipment slot

`ItemResource::equippable_slot(host)` answers which slot a stack goes in when worn
(`Equippable.slot`), from the item's `equippable` component after the stack's patch, so a
command's or a data pack's override counts. It does not say whether a given entity may wear it;
that depends on the wearer and is not answered here."#]
    fn item_components_note0();
    #[notes = r#"## Enchantment conflicts

`Host::enchantments_compatible(a, b)` answers `Enchantment.areCompatible` from the world's
enchantment definitions, so a data pack's `exclusive_set` counts. A mod that adds an enchantment
to a stack asks it for each enchantment already on it, as the enchanting table and the anvil do."#]
    fn item_components_note1();
    #[notes = r#"## Effective stack properties

`ItemResource` reads the stack's component overrides and removals before item defaults:
`damageable(host)`, `foil(host)`, `rarity(host)` and `enchantable(host)`.
`repairable(host, material)` takes another resource; only its item identity and tags matter,
not its count or component patch. `bar(host)` returns an optional `ItemBar`, with width out of
13 pixels and RGB colour, before client drawing hooks; bundles use their fullness bar.
`use_duration(host)` returns ticks, including the gameplay use-duration Part. Vanilla's
current item classes do not require a player argument; Parts see the host's current acting
player. These reads use a world-phase host; tag-based repair checks need its authoritative world tags.
Duration and bar reads need an engine backend and return `None` for an unknown item."#]
    fn item_components_note2();
    #[notes = r#"## Tooltip lines and random enchanting

`ItemResource::tooltip(client, advanced, creative)` returns styled text and line colours
for overlays that need the ordinary inventory tooltip. It uses the client's current viewer
and the requested flags. The base lines do not call mod listeners or lay out a tooltip, so
calling this from a tooltip listener does not dispatch that listener again.

`ItemResource::enchant_randomly(server, level, power, pool)` rolls from the specified
enchantment tag with the active server level's random source and the stack's effective
enchantability. It returns a detached result. Use the normal item-place or capability
transaction to replace a stored stack. Discarding the result does not rewind the random source."#]
    fn item_components_note3();
}
