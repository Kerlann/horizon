/// Fixtures that tests of other crates use to install Parts.
#[cfg(any(test, feature = "testing"))]
pub mod tests {

    /// A mod's battery: the game's at its position, misanswering by 100 as `lie` says.
    struct ModBattery {
        at: BlockSide,
        lie: Lie,
    }
    impl EnergyHandler for ModBattery {
        fn amount(&self, host: &mut dyn ServerHost) -> u32 {
            crate::backend_of(host)
                .and_then(|backend| backend.energy_amount(self.at))
                .unwrap_or(0)
        }
        fn capacity(&self, host: &mut dyn ServerHost) -> u32 {
            crate::backend_of(host).map_or(0, |backend| backend.energy_capacity(self.at))
        }
        fn insert(&self, host: &mut dyn ServerHost, tx: &Transaction, amount: u32) -> u32 {
            let went = crate::backend_mut_of(host)
                .map_or(0, |backend| backend.energy_insert(self.at, amount, tx));
            match self.lie {
                Lie::TakesMore => went + 100,
                Lie::GivesMore => went,
            }
        }
        fn extract(&self, host: &mut dyn ServerHost, tx: &Transaction, amount: u32) -> u32 {
            let more = match self.lie {
                Lie::GivesMore => 100,
                Lie::TakesMore => 0,
            };
            crate::backend_mut_of(host).map_or(0, |backend| {
                backend.energy_extract(self.at, amount + more, tx)
            })
        }
    }

    #[cfg(test)]
    /// Batteries as the game answers them, by position: what each holds, and the most one
    /// insert moves, of a capacity of 1000; and the mod block entity types standing at some.
    #[derive(Clone, Default)]
    struct EnergyWorld {
        batteries: HashMap<[i32; 3], (u32, u32)>,
        types: HashMap<[i32; 3], &'static str>,
        kept: Vec<Self>,
    }
    #[cfg(test)]
    impl EnergyWorld {
        #[cfg(test)]
        fn amounts(&self, at: &[[i32; 3]]) -> Vec<u32> {
            at.iter().map(|pos| self.batteries[pos].0).collect()
        }
    }
    #[cfg(test)]
    impl Journal for EnergyWorld {
        fn kept(&mut self) -> &mut Vec<Self> {
            &mut self.kept
        }
    }
    #[cfg(test)]
    impl Host for EnergyWorld {
        fn block_entity(&self, level: LevelRef, position: [i32; 3]) -> Option<BlockEntityRef> {
            Some(crate::__private::game::block_entity_ref(level, position, 1))
        }
        fn backend(&self) -> Option<BackendRef<'_>> {
            Some(BackendRef::of(self))
        }
    }
    #[cfg(test)]
    impl ServerHost for EnergyWorld {
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
    impl Backend for EnergyWorld {
        fn level_in_place(&self, level: LevelRef) -> bool {
            level == here()
        }
        fn block_entity_kind(
            &self,
            BlockEntityRef { position: pos, .. }: BlockEntityRef,
        ) -> Option<BlockEntityTypeKey> {
            self.types.get(&pos).map(|id| type_key(id))
        }
        fn energy_amount(&self, BlockSide { pos, .. }: BlockSide) -> Option<u32> {
            Some(self.batteries.get(&pos)?.0)
        }
        fn energy_capacity(&self, _: BlockSide) -> u32 {
            1000
        }
    }
    #[cfg(test)]
    impl BackendMut for EnergyWorld {
        fn energy_insert(&mut self, at: BlockSide, amount: u32, _: &Transaction) -> u32 {
            let Some((held, limit)) = self.batteries.get_mut(&at.pos) else {
                return 0;
            };
            let went = amount.min(*limit).min(1000 - *held);
            *held += went;
            went
        }
        fn energy_extract(&mut self, at: BlockSide, amount: u32, _: &Transaction) -> u32 {
            let Some((held, _)) = self.batteries.get_mut(&at.pos) else {
                return 0;
            };
            let came = amount.min(*held);
            *held -= came;
            came
        }
    }

    #[test]
    fn an_energy_move_takes_what_the_targets_room_and_limit_let_in_and_loses_none() {
        game_thread();
        let mut world = EnergyWorld::default();
        world.batteries.insert(SOURCE, (900, 1000));
        world.batteries.insert(TANK, (700, 1000));
        world.batteries.insert(OTHER, (0, 250));
        let mut moved = |from, to, max| {
            let host: &mut dyn ServerHost = &mut world;
            let source = here().capability(host, &ENERGY, from, None).unwrap();
            let target = here().capability(host, &ENERGY, to, None).unwrap();
            host.transaction(|host, tx| move_energy(host, &source, &target, max, tx))
        };
        // The target's room, then its limit per insert, each by a move tried again with what
        // it took; and no more than the source holds.
        assert_eq!(moved(SOURCE, TANK, 1000), Ok(300));
        assert_eq!(moved(SOURCE, OTHER, 1000), Ok(250));
        assert_eq!(moved(OTHER, SOURCE, 1000), Ok(250));
        assert_eq!(world.amounts(&[SOURCE, TANK, OTHER]), [600, 1000, 0]);
    }

    #[test]
    fn a_battery_answering_more_than_it_was_asked_aborts_the_whole_move_and_loses_no_energy() {
        game_thread();
        for (liar, at) in [
            ("test:battery_gives_more", SOURCE),
            ("test:battery_takes_more", TANK),
        ] {
            let mut world = EnergyWorld::default();
            world.batteries.insert(SOURCE, (500, 1000));
            world.batteries.insert(OTHER, (500, 1000));
            world.batteries.insert(TANK, (0, 1000));
            world.types.insert(at, liar);
            let host: &mut dyn ServerHost = &mut world;
            let ends = |host: &mut dyn ServerHost, pos| here().capability(host, &ENERGY, pos, None);
            let (source, other, tank) = (
                ends(host, SOURCE).unwrap(),
                ends(host, OTHER).unwrap(),
                ends(host, TANK).unwrap(),
            );
            // Two moves into the battery at `TANK` in one transaction, the second from the
            // source: a move through the liar fails, and its error aborts the transaction.
            let moved = host.transaction(|host, tx| {
                let first = move_energy(host, &other, &tank, 200, tx)?;
                let second = move_energy(host, &source, &tank, 200, tx)?;
                Ok(first + second)
            });
            let owner = "doubler".to_owned();
            assert_eq!(moved, Err(Overanswered { owner }), "{liar}");
            assert_eq!(
                world.amounts(&[SOURCE, OTHER, TANK]),
                [500, 500, 0],
                "{liar}"
            );
        }
    }
}
