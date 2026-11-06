pub(crate) fn resolved_fluids() -> &'static OnceLock<super::registry_keys::Names> {
    static EMPTY: OnceLock<super::registry_keys::Names> = OnceLock::new();
    installed().map_or(&EMPTY, |installed| &installed.fluids)
}

/// What a mod's [`Hooks::register`](super::Hooks::register) attaches its parts with, and
/// declares its block entity types and ticket controllers on. A thing takes one part of each
/// kind: the same kind twice on one thing refuses the mods at startup, naming both.
///
/// Mods call the generic methods on `dyn Registrar` after requesting registry keys;
/// they reach the trait's own methods, one implementation for native and wasm mods.
pub trait Registrar {
    /// Request a fluid by canonical registry id, checked when the registry loads.
    fn fluid_key(&mut self, id: &str) -> super::FluidKey;
    /// Declare a tank-only fluid in this mod's namespace and return its key.
    fn declare_fluid(&mut self, path: &str) -> super::FluidKey;
}

impl Registrar for PartRegistrar {
    fn fluid_key(&mut self, id: &str) -> super::FluidKey {
        match self
            .registry_keys
            .borrow_mut()
            .fluids
            .request(self.mod_id, id)
        {
            Ok(slot) => super::FluidKey(slot),
            Err(error) => {
                self.refused.push(error);
                super::FluidKey(u32::MAX)
            }
        }
    }

    fn declare_fluid(&mut self, path: &str) -> super::FluidKey {
        let id = format!("{}:{path}", self.mod_id);
        if path.contains(':') || !horizon_mods::content::namespaced_id(&id) {
            self.refused.push(format!(
                "mod {}: invalid fluid declaration {id:?}",
                self.mod_id
            ));
            return super::FluidKey(u32::MAX);
        }
        let previous = self
            .registry_keys
            .borrow_mut()
            .declared_fluids
            .insert(id.clone(), self.mod_id);
        if let Some(other) = previous {
            self.refused.push(format!(
                "fluid {id} declared by both {other} and {}",
                self.mod_id
            ));
        }
        self.fluid_key(&id)
    }
}
/// Fixtures that tests of other crates use to install Parts.
#[cfg(any(test, feature = "testing"))]
pub mod tests {

    #[test]
    fn invalid_and_duplicate_fluid_declarations_are_refused() {
        let mut registrar = super::PartRegistrar::new("maker");
        registrar.declare_fluid("wrong:namespace");
        registrar.declare_fluid("chocolate");
        registrar.declare_fluid("chocolate");
        assert_eq!(registrar.refused.len(), 2);
        assert!(registrar.refused[0].contains("maker"));
        assert!(registrar.refused[1].contains("maker:chocolate"));
    }

    #[test]
    fn fluid_keys_resolve_declarations_after_requests_and_reject_unknown() {
        use crate::registry_keys::{fluid_id, fluid_key, resolve_fluids};
        struct Request;
        #[mod_api::__subscriptions]
        impl Hooks for Request {
            fn register(&self, r: &mut dyn Registrar, _: &RegistrationContext) {
                assert_eq!(r.fluid_key("water"), r.fluid_key("minecraft:water"));
                r.fluid_key("maker:chocolate");
            }
        }
        struct Declare;
        #[mod_api::__subscriptions]
        impl Hooks for Declare {
            fn register(&self, r: &mut dyn Registrar, _: &RegistrationContext) {
                r.declare_fluid("chocolate");
            }
        }
        let mods: Mods = vec![
            ModEntry::new("reader", Box::new(Request)),
            ModEntry::new("maker", Box::new(Declare)),
        ]
        .into();
        super::super::install(mods.clone());
        install_parts(&mods).unwrap();
        let error = resolve_fluids(&[]).unwrap_err();
        assert_eq!(
            error,
            vec![crate::UnknownFluid {
                mod_id: "reader",
                id: "minecraft:water".to_owned()
            }]
        );
        resolve_fluids(&["minecraft:water", "minecraft:lava"]).unwrap();
        let chocolate = fluid_key("maker:chocolate").unwrap();
        assert_eq!(fluid_id(chocolate), Some("maker:chocolate"));
        for name in ["minecraft:water", "minecraft:lava", "maker:chocolate"] {
            let key = fluid_key(name).unwrap();
            assert_eq!(
                crate::registry_keys::fluid_key_utf16(&name.encode_utf16().collect::<Vec<_>>()),
                Some(key)
            );
            assert_eq!(fluid_id(key), Some(name));
        }
        for path in ["water", "lava"] {
            assert_eq!(
                crate::registry_keys::fluid_key_utf16(&path.encode_utf16().collect::<Vec<_>>()),
                fluid_key(path)
            );
        }
        assert_eq!(FluidKey::from_slot(chocolate.0), Ok(chocolate));
        assert!(fluid_key("missing:fluid").is_none());
    }
    /// A mod's tank: the game's tank at its position, reached from every side but its top, and
    /// misanswering as `lie` says.
    struct ModTank {
        at: BlockSide,
        lie: Option<Lie>,
    }
    impl ResourceHandler<FluidResource> for ModTank {
        fn size(&self, host: &mut dyn ServerHost) -> usize {
            if self.at.side == Some(Direction::Up) {
                return 0;
            }
            crate::backend_of(host)
                .and_then(|backend| backend.fluid_tank_count(self.at))
                .unwrap_or(0)
        }
        fn resource(&self, host: &mut dyn ServerHost, index: usize) -> Option<FluidResource> {
            Some(FluidResource::of(
                &crate::backend_of(host).and_then(|backend| backend.fluid_tank(self.at, index))?,
            ))
        }
        fn amount(&self, host: &mut dyn ServerHost, index: usize) -> u32 {
            crate::backend_of(host)
                .and_then(|backend| backend.fluid_tank(self.at, index))
                .map_or(0, |held| held.amount)
        }
        fn capacity(
            &self,
            host: &mut dyn ServerHost,
            index: usize,
            resource: &FluidResource,
        ) -> u32 {
            crate::backend_of(host).map_or(0, |backend| {
                backend.fluid_tank_capacity(self.at, index, &resource.to_stack(1))
            })
        }
        fn insert(
            &self,
            host: &mut dyn ServerHost,
            tx: &Transaction,
            index: usize,
            resource: &FluidResource,
            amount: u32,
        ) -> u32 {
            let Some(backend) = crate::backend_mut_of(host) else {
                return 0;
            };
            let rest = backend.fluid_tank_insert(self.at, index, resource.to_stack(amount), tx);
            let went = amount - rest.map_or(0, |rest| rest.amount);
            match self.lie {
                Some(Lie::TakesMore) => went + 100,
                _ => went,
            }
        }
        fn extract(
            &self,
            host: &mut dyn ServerHost,
            tx: &Transaction,
            index: usize,
            resource: &FluidResource,
            amount: u32,
        ) -> u32 {
            let held =
                crate::backend_of(host).and_then(|backend| backend.fluid_tank(self.at, index));
            if !held.is_some_and(|held| resource.matches(&held)) {
                return 0;
            }
            let more = if self.lie == Some(Lie::GivesMore) {
                100
            } else {
                0
            };
            crate::backend_mut_of(host)
                .and_then(|backend| backend.fluid_tank_extract(self.at, index, amount + more, tx))
                .map_or(0, |out| out.amount)
        }
    }

    #[cfg(test)]
    /// One tank, as the game answers it: what it holds, its capacity, and whether it fills and
    /// drains. Truthfully: a `halving` tank takes half of each offer, a `half_room` one at most
    /// half its room, a `short` one gives one less than asked, and a `quarters` one gives and
    /// takes only whole quarter buckets, as a cauldron its thirds.
    #[derive(Clone)]
    struct GameTank {
        held: Option<FluidStack>,
        capacity: u32,
        fills: bool,
        drains: bool,
        halving: bool,
        half_room: bool,
        short: bool,
        quarters: bool,
    }
    #[cfg(test)]
    /// Game tanks by position, and the mod block entity types standing at some.
    #[derive(Clone, Default)]
    struct TankWorld {
        tanks: HashMap<[i32; 3], GameTank>,
        types: HashMap<[i32; 3], &'static str>,
        kept: Vec<Self>,
    }
    #[cfg(test)]
    const SOURCE: [i32; 3] = [0, 64, 0];
    #[cfg(test)]
    const TANK: [i32; 3] = [1, 64, 0];
    #[cfg(test)]
    const OTHER: [i32; 3] = [2, 64, 0];
    #[cfg(test)]
    impl TankWorld {
        #[cfg(test)]
        fn tank(&mut self, pos: [i32; 3], held: Option<FluidStack>, fills: bool) -> &mut Self {
            let tank = GameTank {
                held,
                capacity: 1000,
                fills,
                drains: true,
                halving: false,
                half_room: false,
                short: false,
                quarters: false,
            };
            self.tanks.insert(pos, tank);
            self
        }
        #[cfg(test)]
        fn held(&self, pos: [i32; 3]) -> Option<FluidStack> {
            self.tanks[&pos].held.clone()
        }
        #[cfg(test)]
        fn amount(&self, pos: [i32; 3]) -> u32 {
            self.tanks[&pos].held.as_ref().map_or(0, |held| held.amount)
        }
        #[cfg(test)]
        /// Moves up to `max` from `from`'s handler through `from_side` into `to`'s through
        /// `to_side`, in a transaction of its own.
        fn moved(
            &mut self,
            (from, from_side): ([i32; 3], Option<Direction>),
            (to, to_side): ([i32; 3], Option<Direction>),
            max: u32,
        ) -> Result<u32, Overanswered> {
            let host: &mut dyn ServerHost = self;
            let source = here().capability(host, &FLUIDS, from, from_side).unwrap();
            let target = here().capability(host, &FLUIDS, to, to_side).unwrap();
            host.transaction(|host, tx| move_resources(host, &source, &target, |_| true, max, tx))
        }
    }
    #[cfg(test)]
    impl Journal for TankWorld {
        fn kept(&mut self) -> &mut Vec<Self> {
            &mut self.kept
        }
    }
    #[cfg(test)]
    impl Backend for TankWorld {
        fn level_in_place(&self, level: LevelRef) -> bool {
            level == here()
        }
        fn block_entity_kind(
            &self,
            BlockEntityRef { position: pos, .. }: BlockEntityRef,
        ) -> Option<BlockEntityTypeKey> {
            self.types.get(&pos).map(|id| type_key(id))
        }
        fn fluid_tank_count(&self, BlockSide { pos, .. }: BlockSide) -> Option<usize> {
            self.tanks.contains_key(&pos).then_some(1)
        }
        fn fluid_tank(&self, BlockSide { pos, .. }: BlockSide, _: usize) -> Option<FluidStack> {
            self.tanks.get(&pos)?.held.clone()
        }
        fn fluid_tank_capacity(
            &self,
            BlockSide { pos, .. }: BlockSide,
            _: usize,
            _: &FluidStack,
        ) -> u32 {
            self.tanks
                .get(&pos)
                .filter(|tank| tank.fills)
                .map_or(0, |tank| tank.capacity)
        }
    }
    #[cfg(test)]
    impl BackendMut for TankWorld {
        fn fluid_tank_insert(
            &mut self,
            BlockSide { pos, .. }: BlockSide,
            _: usize,
            fluid: FluidStack,
            _: &Transaction,
        ) -> Option<FluidStack> {
            let Some(tank) = self.tanks.get_mut(&pos).filter(|tank| tank.fills) else {
                return Some(fluid);
            };
            let held = tank.held.as_ref().map_or(0, |held| held.amount);
            if tank
                .held
                .as_ref()
                .is_some_and(|held| !held.same_fluid_same_components(&fluid))
            {
                return Some(fluid);
            }
            let offered = if tank.halving {
                fluid.amount / 2
            } else {
                fluid.amount
            };
            let room = if tank.half_room {
                (tank.capacity - held) / 2
            } else {
                tank.capacity - held
            };
            let mut add = offered.min(room);
            if tank.quarters {
                add -= add % 250;
            }
            if add > 0 {
                tank.held = Some(fluid.with_amount(held + add));
            }
            (add < fluid.amount).then(|| fluid.with_amount(fluid.amount - add))
        }
        fn fluid_tank_extract(
            &mut self,
            BlockSide { pos, .. }: BlockSide,
            _: usize,
            max: u32,
            _: &Transaction,
        ) -> Option<FluidStack> {
            let tank = self.tanks.get_mut(&pos).filter(|tank| tank.drains)?;
            let held = tank.held.clone()?;
            let asked = if tank.short {
                max.saturating_sub(1)
            } else {
                max
            };
            let mut take = held.amount.min(asked);
            if tank.quarters {
                take -= take % 250;
            }
            if take == 0 {
                return None;
            }
            tank.held = (take < held.amount).then(|| held.with_amount(held.amount - take));
            Some(held.with_amount(take))
        }
    }
    #[cfg(test)]
    impl Host for TankWorld {
        fn block_entity(&self, level: LevelRef, position: [i32; 3]) -> Option<BlockEntityRef> {
            Some(BlockEntityRef {
                level,
                position,
                generation: 1,
            })
        }
        fn backend(&self) -> Option<BackendRef<'_>> {
            Some(BackendRef::of(self))
        }
    }
    #[cfg(test)]
    impl ServerHost for TankWorld {
        fn transaction_open(&mut self, _: TransactionSeal) {
            open(self);
        }
        fn transaction_close(&mut self, _: TransactionSeal, commit: bool) {
            close(self, commit);
        }
        fn backend_mut(&mut self) -> Option<BackendMutRef<'_>> {
            Some(BackendMutRef::of(self))
        }
    }

    #[cfg(test)]
    fn potion(amount: u32, potion: &str) -> FluidStack {
        let mut components = horizon_items::item_data::StackData::default();
        components.potion = Some(Arc::from(potion));
        FluidStack {
            components: Some(Box::new(components)),
            ..FluidStack::new(
                crate::registry_keys::fluid_key("test:potion").unwrap(),
                amount,
            )
        }
    }

    #[test]
    fn a_fluid_move_fills_only_with_the_same_fluid_and_components() {
        game_thread();
        let mut world = TankWorld::default();
        world.tank(SOURCE, Some(potion(1500, "minecraft:healing")), true);
        world.tank(TANK, None, true).types.insert(TANK, "test:tank");
        // The mod's tank takes nothing from above.
        let up = (TANK, Some(Direction::Up));
        let west = (TANK, Some(Direction::West));
        assert_eq!(world.moved((SOURCE, None), up, 4000), Ok(0));
        // What the tank has room for moves, and nothing more leaves the source.
        assert_eq!(world.moved((SOURCE, None), west, 4000), Ok(1000));
        assert_eq!(world.moved(west, (SOURCE, None), 400), Ok(400));
        assert_eq!(world.held(SOURCE), Some(potion(900, "minecraft:healing")));
        assert_eq!(world.held(TANK), Some(potion(600, "minecraft:healing")));
        // The same fluid with other components, or another fluid, doesn't go in.
        for other in [
            potion(500, "minecraft:poison"),
            FluidStack::new(crate::registry_keys::fluid_key("test:potion").unwrap(), 500),
        ] {
            world.tank(SOURCE, Some(other.clone()), true);
            assert_eq!(world.moved((SOURCE, None), (TANK, None), 4000), Ok(0));
            assert_eq!(world.held(SOURCE), Some(other));
            assert_eq!(world.held(TANK), Some(potion(600, "minecraft:healing")));
        }
        // A drain-only tank has no capacity, and nothing moves.
        let water = FluidStack::new(
            crate::registry_keys::fluid_key("minecraft:water").unwrap(),
            1000,
        );
        world.tank(SOURCE, Some(water.clone()), true);
        world.tank(TANK, None, false);
        let host: &mut dyn ServerHost = &mut world;
        let target = here().capability(host, &FLUIDS, TANK, None).unwrap();
        assert_eq!(target.capacity(host, 0, &FluidResource::of(&water)), 0);
        assert_eq!(world.moved((SOURCE, None), (TANK, None), 1000), Ok(0));
        assert_eq!(world.held(SOURCE), Some(water));
    }

    #[test]
    fn a_fluid_move_tries_again_with_what_the_target_took_and_loses_nothing() {
        game_thread();
        let water = |amount| {
            FluidStack::new(
                crate::registry_keys::fluid_key("minecraft:water").unwrap(),
                amount,
            )
        };
        type Setup = fn(&mut GameTank, &mut GameTank);
        let cases: [(&str, Setup, u32); 6] = [
            // Half of each offer: no request is ever taken whole, so nothing moves.
            ("half of each offer", |_, t| t.halving = true, 0),
            (
                "half of each offer, from quarters",
                |s, t| (s.quarters, t.halving) = (true, true),
                0,
            ),
            (
                "half of each offer, from a drain-only tank",
                |s, t| (s.fills, t.halving) = (false, true),
                0,
            ),
            // Half its room: moves of 500, 250, then 125, which quarters can't give.
            (
                "half its room, from quarters",
                |s, t| (s.quarters, t.half_room) = (true, true),
                750,
            ),
            (
                "half its room, from a drain-only tank",
                |s, t| (s.fills, t.half_room) = (false, true),
                999,
            ),
            // One less than asked: the 999 it gives all go in.
            ("a source giving one less", |s, _| s.short = true, 999),
        ];
        for (case, setup, moves) in cases {
            let mut world = TankWorld::default();
            world.tank(SOURCE, Some(water(1000)), true);
            world.tank(TANK, None, true);
            let [source, target] = world.tanks.get_disjoint_mut([&SOURCE, &TANK]);
            setup(source.unwrap(), target.unwrap());
            let moved = world.moved((SOURCE, None), (TANK, None), 1000);
            assert_eq!(moved, Ok(moves), "{case}");
            assert_eq!(
                (world.amount(SOURCE), world.amount(TANK)),
                (1000 - moves, moves),
                "{case}"
            );
        }
    }

    #[test]
    fn a_handler_answering_more_than_it_was_asked_aborts_the_whole_move_and_loses_no_fluid() {
        game_thread();
        let water = |amount| {
            Some(FluidStack::new(
                crate::registry_keys::fluid_key("minecraft:water").unwrap(),
                amount,
            ))
        };
        for (liar, at) in [("test:gives_more", SOURCE), ("test:takes_more", TANK)] {
            let mut world = TankWorld::default();
            world.tank(SOURCE, water(1000), true);
            world.tank(OTHER, water(1000), true);
            world.tank(TANK, None, true).types.insert(at, liar);
            let host: &mut dyn ServerHost = &mut world;
            let ends = |host: &mut dyn ServerHost, pos| here().capability(host, &FLUIDS, pos, None);
            let (source, other, tank) = (
                ends(host, SOURCE).unwrap(),
                ends(host, OTHER).unwrap(),
                ends(host, TANK).unwrap(),
            );
            // Two moves into the tank in one transaction, the second from the source: the
            // first move through the liar fails, and its error aborts the transaction, so
            // every tank holds what it held.
            let moved = host.transaction(|host, tx| {
                let first = move_resources(host, &other, &tank, |_| true, 300, tx)?;
                let second = move_resources(host, &source, &tank, |_| true, 300, tx)?;
                Ok(first + second)
            });
            let owner = "doubler".to_owned();
            assert_eq!(moved, Err(Overanswered { owner }), "{liar}");
            let held = [SOURCE, OTHER, TANK].map(|pos| world.held(pos));
            assert_eq!(held, [water(1000), water(1000), None], "{liar}");
        }
    }
}
