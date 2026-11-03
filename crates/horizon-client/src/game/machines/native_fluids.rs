//! The game's own answer to the fluid capability (`FLUIDS`, docs/native-mods.md): the tanks a
//! mod block declared through `native_block_entity_fluids`, and a cauldron, read through a
//! side, filled and drained in millibuckets. They act where the world is simulated, never on a
//! client's copy, and only on the level in place.
use super::Game;
use horizon_client::containers::{BlockEntity, ModFluidTanks};
use horizon_client::mod_api::{BlockSide, FluidStack, Transaction};
use horizon_core::geometry::Direction;
use horizon_world::nbt::{Compound, JavaString, Tag};
use std::sync::Arc;

/// A cauldron level's contents: level `level` holds `level * 1000 / 3`
/// millibuckets (`LayeredCauldronBlock` has no finer unit than a bottle's
/// third of a bucket).
fn third(level: u8) -> i32 {
    i32::from(level) * 1000 / 3
}

/// The level a fill of `given` millibuckets raises `level` to: a level
/// rises only while its whole third fits what is left of the give, so no
/// sum ever grows past a bucket.
fn fill_level(level: u8, given: i32) -> u8 {
    let mut new = level;
    while new < 3 && third(new + 1) - third(level) <= given {
        new += 1;
    }
    new
}

/// What a drain of at most `amount` takes out of water level `level`, in
/// whole thirds — never more than was asked, as `IFluidHandler.drain`'s
/// `maxDrain`: less than the topmost whole third takes nothing. Answers
/// what it took and the level it left.
fn drain_take(level: u8, amount: i32) -> (i32, u8) {
    let mut taken = 0;
    let mut left = level;
    while left > 0 {
        let step = third(left) - third(left - 1);
        if taken + step > amount {
            break;
        }
        taken += step;
        left -= 1;
    }
    (taken, left)
}

/// What holds fluids at a position: a mod block's tanks, or a cauldron
/// (`CauldronFluids`). An empty cauldron is fillable; powder snow is not a
/// fluid (`FluidKind` has none), so a powder snow cauldron holds none.
enum FluidAt {
    Mod(Arc<ModFluidTanks>),
    Cauldron(CauldronFluids),
}
#[derive(Clone, Copy)]
enum CauldronFluids {
    Empty,
    Water(u8),
    Lava,
}
impl CauldronFluids {
    /// What a fill of `amount` of `fluid` takes, and what the cauldron then holds: water rises
    /// by whole thirds, lava fills an empty cauldron only whole. None when it takes nothing.
    fn fill(self, fluid: &str, amount: i32) -> Option<(i32, CauldronFluids)> {
        match (self, fluid) {
            (Self::Empty | Self::Water(_), "minecraft:water") => {
                let level = match self {
                    Self::Water(level) => level,
                    _ => 0,
                };
                let new = fill_level(level, amount.max(0));
                (new > level).then(|| (third(new) - third(level), Self::Water(new)))
            }
            // Water into lava would turn it to stone in vanilla's world, not in a tank.
            (Self::Empty, "minecraft:lava") => (amount >= 1000).then_some((1000, Self::Lava)),
            _ => None,
        }
    }
    /// What a drain of at most `amount` takes: the fluid, how much, and what the cauldron then
    /// holds. Water drains in whole thirds, lava only whole. None when it gives nothing.
    fn drain(self, amount: i32) -> Option<(&'static str, i32, CauldronFluids)> {
        match self {
            Self::Water(level) => {
                let (taken, left) = drain_take(level, amount);
                let left = if left > 0 {
                    Self::Water(left)
                } else {
                    Self::Empty
                };
                (taken > 0).then_some(("minecraft:water", taken, left))
            }
            Self::Lava => (amount >= 1000).then_some(("minecraft:lava", 1000, Self::Empty)),
            Self::Empty => None,
        }
    }
}

/// The tanks of a mod block its side `side` reaches (none: every tank), in the order the side
/// numbers them for the fluid capability.
fn side_tanks(tanks: &ModFluidTanks, side: Option<Direction>) -> Vec<usize> {
    let n = tanks.tanks.len();
    match side {
        Some(face) => tanks.faces[face as usize]
            .iter()
            .copied()
            .filter(|&tank| tank < n)
            .collect(),
        None => (0..n).collect(),
    }
}

/// Whether the game's tanks at `at` take `fluid`: water or lava, the game's only fluids. A mod
/// block's saved tanks keep an id and an amount, so they take none with components. A cauldron
/// takes water and lava whatever their components and discards them, as NeoForge's
/// `CauldronFluidContent` does: the one place a fluid's components are dropped, and what comes
/// out of a cauldron is plain.
fn game_holds(fluid: &FluidStack, at: &FluidAt) -> bool {
    matches!(&*fluid.fluid, "minecraft:water" | "minecraft:lava")
        && (matches!(at, FluidAt::Cauldron(_)) || !fluid.has_components())
}

/// A capability amount in the fluid helpers' millibuckets.
fn mb(amount: u32) -> i32 {
    i32::try_from(amount).unwrap_or(i32::MAX)
}

/// The saved state of one tank: its fluid and amount in millibuckets.
type Tank = (Option<String>, i32);

/// The tanks in `nbt`, each `n` long: missing entries read as empty, and a
/// save with more tanks than the block now declares keeps the ones that
/// still exist (a mod update that shrank its tanks loses nothing it kept).
fn read_tanks(nbt: &Compound, n: usize) -> Vec<Tank> {
    let list = match nbt.get(&JavaString::from("fluids")) {
        Some(Tag::List(list)) => list,
        _ => return vec![(None, 0); n],
    };
    let read: Vec<Tank> = list
        .iter()
        .take(n)
        .map(|tag| match tag {
            Tag::Compound(map) => (
                match map.get(&JavaString::from("Fluid")) {
                    Some(Tag::String(fluid)) if fluid.to_string_lossy() != "minecraft:empty" => {
                        Some(fluid.to_string_lossy())
                    }
                    _ => None,
                },
                match map.get(&JavaString::from("Amount")) {
                    Some(Tag::Int(amount)) => *amount,
                    _ => 0,
                },
            ),
            _ => (None, 0),
        })
        .collect();
    let mut tanks = read;
    tanks.resize(n, (None, 0));
    tanks
}

/// `read_tanks`'s writer: the list back into `nbt`, an empty tank as
/// `minecraft:empty` so the tank numbers stay in place.
fn write_tanks(nbt: &mut Compound, tanks: &[Tank]) {
    let list = tanks
        .iter()
        .map(|&(ref fluid, amount)| {
            let mut map = Compound::new();
            map.insert(
                JavaString::from("Fluid"),
                Tag::String(JavaString::from(
                    fluid.as_deref().unwrap_or("minecraft:empty"),
                )),
            );
            map.insert(JavaString::from("Amount"), Tag::Int(amount));
            Tag::Compound(map)
        })
        .collect();
    nbt.insert(JavaString::from("fluids"), Tag::List(list));
}

// Native mods use these; nothing in Horizon does.
#[allow(dead_code)]
impl Game<'_> {
    /// What holds fluids at `pos` in the level in place: a mod block's tanks (its block
    /// entity's, and not the placeholder standing in while it ticks), or a cauldron.
    fn native_fluid_at(&self, pos: [i32; 3]) -> Option<FluidAt> {
        if !self.native_simulates() {
            return None;
        }
        let key = (pos[0], pos[1], pos[2]);
        let state = self.world.block(pos[0], pos[1], pos[2]);
        if let Some(tanks) = self.world.block_entities.mod_fluids(state.0) {
            // The block entity is out of the world while it ticks.
            return matches!(
                self.world.block_entities.get(&key),
                Some(horizon_client::containers::BlockEntity::Mod(m)) if !m.id().is_empty()
            )
            .then(|| FluidAt::Mod(Arc::clone(tanks)));
        }
        let info = self.pack.assets.registry.state(state)?;
        match &*info.name {
            "minecraft:cauldron" => Some(FluidAt::Cauldron(CauldronFluids::Empty)),
            "minecraft:water_cauldron" => {
                let level = info
                    .properties
                    .get("level")
                    .and_then(|l| l.parse::<u8>().ok())?;
                Some(FluidAt::Cauldron(CauldronFluids::Water(level)))
            }
            "minecraft:lava_cauldron" => Some(FluidAt::Cauldron(CauldronFluids::Lava)),
            _ => None,
        }
    }
    /// The cauldron at `pos` set to hold `to`. Whether it changed.
    fn cauldron_set(&mut self, pos: [i32; 3], to: CauldronFluids) -> bool {
        let (name, level) = match to {
            CauldronFluids::Empty | CauldronFluids::Water(0) => ("minecraft:cauldron", None),
            CauldronFluids::Water(level) => ("minecraft:water_cauldron", Some(level)),
            CauldronFluids::Lava => ("minecraft:lava_cauldron", None),
        };
        let mut properties = std::collections::BTreeMap::new();
        if let Some(level) = level {
            properties.insert("level".to_owned(), level.to_string());
        }
        let state = horizon_world::chunk::BlockState {
            name: name.to_owned(),
            properties: properties.into(),
        };
        match self.pack.assets.registry.resolve(&state) {
            Ok(id) => self.journal_set_block(pos, id),
            Err(_) => false,
        }
    }
    /// The saved tanks of the mod block entity at `pos`, `n` of them.
    fn tank_state(&self, pos: [i32; 3], n: usize) -> Option<Vec<Tank>> {
        match self.world.block_entities.get(&(pos[0], pos[1], pos[2]))? {
            BlockEntity::Mod(m) => Some(read_tanks(&m.nbt, n)),
            _ => None,
        }
    }
    /// `tank_state`'s writer.
    fn set_tank_state(&mut self, pos: [i32; 3], state: &[Tank]) {
        let key = (pos[0], pos[1], pos[2]);
        self.journal_block_entity(key);
        if let Some(BlockEntity::Mod(m)) = self.world.block_entities.get_mut(&key) {
            write_tanks(&mut m.nbt, state);
            self.world.modified.insert((key.0 >> 4, key.2 >> 4));
        }
    }
    /// The tanks the game's own fluid capability reaches at `at`: in the level in place only.
    fn capability_tanks(&self, at: BlockSide) -> Option<FluidAt> {
        if !self.native_level_in_place(at.level) {
            return None;
        }
        self.native_fluid_at(at.pos)
    }
    /// How many tanks of the block at `at` in the level in place its side reaches
    /// (none: every tank), for the game's own fluid capability: a mod block's by its table, a
    /// cauldron's one. None where no tanks are.
    pub(in crate::game) fn native_fluid_tank_count(&self, at: BlockSide) -> Option<usize> {
        match self.capability_tanks(at)? {
            FluidAt::Mod(tanks) => Some(side_tanks(&tanks, at.side).len()),
            FluidAt::Cauldron(_) => Some(1),
        }
    }
    /// What tank `tank` of those `at`'s side reaches holds (`native_fluid_tank_count`).
    pub(in crate::game) fn native_fluid_tank(
        &self,
        at: BlockSide,
        tank: usize,
    ) -> Option<FluidStack> {
        match self.capability_tanks(at)? {
            FluidAt::Mod(tanks) => {
                let real = *side_tanks(&tanks, at.side).get(tank)?;
                let (fluid, amount) = self
                    .tank_state(at.pos, tanks.tanks.len())?
                    .swap_remove(real);
                let amount = u32::try_from(amount).ok().filter(|&amount| amount > 0)?;
                Some(FluidStack::new(&fluid?, amount))
            }
            // All a cauldron holds is what a bucket's drain takes.
            FluidAt::Cauldron(kind) if tank == 0 => {
                let (fluid, amount, _) = kind.drain(1000)?;
                Some(FluidStack::new(fluid, amount.unsigned_abs()))
            }
            FluidAt::Cauldron(_) => None,
        }
    }
    /// How much of `fluid` tank `tank` of those `at`'s side reaches holds when empty: a mod
    /// block's fillable tank its capacity, a cauldron a bucket; none for another tank, a
    /// drain-only one, or a fluid the tanks there don't take: other than water and lava, or
    /// with components into a mod block's tanks.
    pub(in crate::game) fn native_fluid_tank_capacity(
        &self,
        at: BlockSide,
        tank: usize,
        fluid: &FluidStack,
    ) -> u32 {
        match self.capability_tanks(at) {
            Some(at) if !game_holds(fluid, &at) => 0,
            Some(FluidAt::Mod(tanks)) => side_tanks(&tanks, at.side)
                .get(tank)
                .filter(|real| tanks.inputs.contains(real))
                .map_or(0, |&real| tanks.tanks[real].max(0).unsigned_abs()),
            Some(FluidAt::Cauldron(_)) if tank == 0 => 1000,
            _ => 0,
        }
    }
    /// Puts as much of `fluid` as tank `tank` of those `at`'s side reaches takes: a mod
    /// block's fillable tank, empty or holding that fluid, up to its capacity; a cauldron by
    /// whole thirds, lava only whole into an empty one, discarding the fluid's components as
    /// NeoForge's `CauldronFluidContent` does. What didn't fit. The write is kept by
    /// transaction `tx`.
    pub(in crate::game) fn native_fluid_tank_insert(
        &mut self,
        at: BlockSide,
        tank: usize,
        fluid: FluidStack,
        tx: &Transaction,
    ) -> Option<FluidStack> {
        let _ = tx;
        if fluid.amount == 0 {
            return None;
        }
        let amount = mb(fluid.amount);
        let took = match self.capability_tanks(at) {
            Some(at) if !game_holds(&fluid, &at) => 0,
            Some(FluidAt::Mod(tanks)) => {
                let real = side_tanks(&tanks, at.side)
                    .get(tank)
                    .copied()
                    .filter(|real| tanks.inputs.contains(real));
                let state = real.zip(self.tank_state(at.pos, tanks.tanks.len()));
                let Some((real, mut state)) = state else {
                    return Some(fluid);
                };
                let entry = &mut state[real];
                if entry.1 > 0 && entry.0.as_deref() != Some(&*fluid.fluid) {
                    return Some(fluid);
                }
                let add = (tanks.tanks[real] - entry.1).clamp(0, amount);
                if add > 0 {
                    entry.0 = Some(fluid.fluid.to_string());
                    entry.1 += add;
                    self.set_tank_state(at.pos, &state);
                }
                add
            }
            Some(FluidAt::Cauldron(kind)) if tank == 0 => match kind.fill(&fluid.fluid, amount) {
                Some((took, to)) if self.cauldron_set(at.pos, to) => took,
                _ => 0,
            },
            _ => 0,
        };
        let rest = fluid.amount - took.unsigned_abs();
        (rest > 0).then(|| fluid.with_amount(rest))
    }
    /// Takes up to `max` out of tank `tank` of those `at`'s side reaches: a mod block's
    /// drainable tank, a cauldron by whole thirds and lava only whole. What came out. The
    /// write is kept by transaction `tx`.
    pub(in crate::game) fn native_fluid_tank_extract(
        &mut self,
        at: BlockSide,
        tank: usize,
        max: u32,
        tx: &Transaction,
    ) -> Option<FluidStack> {
        let _ = tx;
        let max = mb(max);
        match self.capability_tanks(at)? {
            FluidAt::Mod(tanks) => {
                let real = side_tanks(&tanks, at.side)
                    .get(tank)
                    .copied()
                    .filter(|real| tanks.outputs.contains(real))?;
                let mut state = self.tank_state(at.pos, tanks.tanks.len())?;
                let entry = &mut state[real];
                let take = entry.1.clamp(0, max);
                let fluid = entry.0.clone().filter(|_| take > 0)?;
                entry.1 -= take;
                if entry.1 == 0 {
                    entry.0 = None;
                }
                self.set_tank_state(at.pos, &state);
                Some(FluidStack::new(&fluid, take.unsigned_abs()))
            }
            FluidAt::Cauldron(kind) if tank == 0 => {
                let (fluid, took, to) = kind.drain(max)?;
                self.cauldron_set(at.pos, to)
                    .then(|| FluidStack::new(fluid, took.unsigned_abs()))
            }
            FluidAt::Cauldron(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cauldron_level_holds_a_third_of_a_bucket() {
        assert_eq!(third(1), 333);
        assert_eq!(third(2), 666);
        assert_eq!(third(3), 1000);
    }

    #[test]
    fn a_cauldron_never_hands_out_more_than_was_asked() {
        // The top third of a full cauldron is 334, then 333, 333.
        assert_eq!(drain_take(3, 1), (0, 3));
        assert_eq!(drain_take(3, 333), (0, 3));
        assert_eq!(drain_take(3, 334), (334, 2));
        assert_eq!(drain_take(3, 500), (334, 2));
        assert_eq!(drain_take(3, 666), (334, 2));
        assert_eq!(drain_take(3, 667), (667, 1));
        assert_eq!(drain_take(3, 1000), (1000, 0));
        assert_eq!(drain_take(1, 332), (0, 1));
        assert_eq!(drain_take(1, 333), (333, 0));
        assert_eq!(drain_take(1, i32::MAX), (333, 0));
    }

    #[test]
    fn a_cauldron_fill_stays_within_a_bucket_whatever_the_amount() {
        assert_eq!(fill_level(0, 332), 0);
        assert_eq!(fill_level(0, 333), 1);
        assert_eq!(fill_level(1, 332), 1);
        assert_eq!(fill_level(1, 333), 2);
        assert_eq!(fill_level(1, 666), 2);
        assert_eq!(fill_level(1, 667), 3);
        assert_eq!(fill_level(0, i32::MAX), 3);
        assert_eq!(fill_level(2, i32::MAX), 3);
    }

    #[test]
    fn a_cauldron_takes_and_gives_water_by_whole_thirds_and_lava_only_whole() {
        use super::CauldronFluids::{Empty, Lava, Water};
        let level = |fluids| match fluids {
            Empty => (0, false),
            Water(level) => (level, false),
            Lava => (0, true),
        };
        let fill = |kind: super::CauldronFluids, fluid, amount| {
            kind.fill(fluid, amount).map(|(took, to)| (took, level(to)))
        };
        // 500 mB raises an empty cauldron one level, 333 mB; a full one takes none.
        assert_eq!(fill(Empty, "minecraft:water", 500), Some((333, (1, false))));
        assert_eq!(
            fill(Water(1), "minecraft:water", 1000),
            Some((667, (3, false)))
        );
        assert_eq!(fill(Water(3), "minecraft:water", 1000), None);
        assert_eq!(fill(Empty, "minecraft:lava", 999), None);
        assert_eq!(fill(Empty, "minecraft:lava", 1500), Some((1000, (0, true))));
        assert_eq!(fill(Water(1), "minecraft:lava", 1000), None);
        assert_eq!(fill(Lava, "minecraft:water", 1000), None);
        let drain = |kind: super::CauldronFluids, amount| {
            kind.drain(amount)
                .map(|(fluid, took, to)| (fluid, took, level(to)))
        };
        assert_eq!(
            drain(Water(3), 500),
            Some(("minecraft:water", 334, (2, false)))
        );
        assert_eq!(drain(Water(1), 332), None);
        assert_eq!(
            drain(Water(2), 1000),
            Some(("minecraft:water", 666, (0, false)))
        );
        assert_eq!(drain(Lava, 999), None);
        assert_eq!(
            drain(Lava, 1000),
            Some(("minecraft:lava", 1000, (0, false)))
        );
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn a_cauldron_move_the_target_takes_part_of_gives_the_whole_levels_that_fit() {
        use super::super::super::native_entity_refs::tests::with_game;
        use horizon_client::chunk::Chunk;
        use horizon_client::mod_api::{FLUIDS, Host, NativeBlockUpdateFlags, move_resources};
        with_game(|game| {
            game.options.dedicated = Some(Vec::new());
            game.world
                .chunks
                .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
            let level = game.native_level("minecraft:overworld").unwrap();
            let water = |game: &Game<'_>, n: u8| {
                let state = horizon_world::chunk::BlockState {
                    name: "minecraft:water_cauldron".to_owned(),
                    properties: [("level".to_owned(), n.to_string())].into_iter().collect(),
                };
                game.pack.assets.registry.resolve(&state).unwrap()
            };
            let (full, one) = (water(game, 3), water(game, 1));
            let (from, to) = ([0, 64, 0], [2, 64, 0]);
            let flags = NativeBlockUpdateFlags::NEIGHBORS;
            assert!(game.native_level_set_block(level, from, full, flags));
            assert!(game.native_level_set_block(level, to, one, flags));
            // The target has room for two levels: the bucket the source first gives doesn't
            // fit, so the move is undone and tried again with what fit, two levels' worth.
            let host: &mut dyn Host = game;
            let source = level.capability(host, &FLUIDS, from, None).unwrap();
            let target = level.capability(host, &FLUIDS, to, None).unwrap();
            let moved = host
                .transaction(|host, tx| move_resources(host, &source, &target, |_| true, 1000, tx));
            assert_eq!(moved, Ok(667));
            assert_eq!(game.world.block(from[0], from[1], from[2]), one);
            assert_eq!(game.world.block(to[0], to[1], to[2]), full);
        });
    }

    #[test]
    fn a_cauldron_takes_water_and_lava_whatever_their_components_and_a_mod_tank_none_with_them() {
        use super::{CauldronFluids, FluidAt, FluidStack, game_holds};
        let potion = |fluid| FluidStack {
            components: Some(Box::new(horizon_client::item_data::StackData {
                potion: Some(Arc::from("minecraft:healing")),
                ..Default::default()
            })),
            ..FluidStack::new(fluid, 1000)
        };
        let cauldron = FluidAt::Cauldron(CauldronFluids::Empty);
        let tank = FluidAt::Mod(Arc::default());
        for fluid in ["minecraft:water", "minecraft:lava"] {
            assert!(game_holds(&potion(fluid), &cauldron), "{fluid}");
            assert!(game_holds(&FluidStack::new(fluid, 1000), &tank), "{fluid}");
            assert!(!game_holds(&potion(fluid), &tank), "{fluid}");
        }
        assert!(!game_holds(&potion("test:potion"), &cauldron));
        // What a cauldron gives is named by its fluid alone: its components are gone.
        let (fluid, amount, _) = CauldronFluids::Water(3).drain(1000).unwrap();
        assert_eq!(
            FluidStack::new(fluid, amount.unsigned_abs()),
            FluidStack::new("minecraft:water", 1000)
        );
    }

    fn tanks(list: &[(Option<&str>, i32)]) -> Vec<Tank> {
        list.iter()
            .map(|&(fluid, amount)| (fluid.map(str::to_owned), amount))
            .collect()
    }

    #[test]
    fn tank_state_round_trips_through_nbt_and_missing_entries_read_empty() {
        let mut nbt = Compound::new();
        let state = tanks(&[(Some("minecraft:water"), 3334), (None, 0)]);
        write_tanks(&mut nbt, &state);
        assert_eq!(read_tanks(&nbt, 2), state);
        // A tank the save never heard of reads as empty, not as gone.
        let longer = read_tanks(&nbt, 3);
        assert_eq!(longer[2], (None, 0));
        // Anything else under `fluids` is the mod's own, not a tank list.
        nbt.insert(JavaString::from("fluids"), Tag::Int(0));
        assert_eq!(read_tanks(&nbt, 2), vec![(None, 0), (None, 0)]);
    }

    #[test]
    fn a_shrunk_tank_table_keeps_the_tanks_that_still_exist() {
        // A mod update that declares one tank where the save has two keeps
        // the first tank's contents, instead of reading all as empty and
        // writing the wipe back.
        let mut nbt = Compound::new();
        let saved = tanks(&[
            (Some("minecraft:water"), 4000),
            (Some("minecraft:lava"), 1000),
        ]);
        write_tanks(&mut nbt, &saved);
        assert_eq!(
            read_tanks(&nbt, 1),
            tanks(&[(Some("minecraft:water"), 4000)])
        );
    }

    #[test]
    fn an_empty_tank_writes_the_empty_fluid_not_none() {
        let mut nbt = Compound::new();
        write_tanks(
            &mut nbt,
            &tanks(&[(Some("minecraft:lava"), 1000), (None, 0)]),
        );
        let Some(Tag::List(list)) = nbt.get(&JavaString::from("fluids")) else {
            panic!("no list");
        };
        let Some(Tag::Compound(empty)) = list.get(1) else {
            panic!("no compound");
        };
        assert_eq!(
            empty.get(&JavaString::from("Fluid")),
            Some(&Tag::String(JavaString::from("minecraft:empty")))
        );
    }
}
