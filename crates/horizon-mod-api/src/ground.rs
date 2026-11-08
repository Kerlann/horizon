//! The ground a tree decorator places (vanilla's `AlterGroundDecorator`, the
//! podzol under a big spruce), and the provider a mod puts in its place in
//! `alter_ground` (NeoForge's `AlterGroundEvent`).
//!
//! The decorator asks its provider once per ground position it tries, on the
//! worldgen worker threads (and on the server thread when a sapling grows), so a
//! provider keeps no mutable state and gets no `Host`: it reads the region the
//! tree stands in and draws from the decorator's own random through
//! [`GroundLevel`], so with the decorator's own provider kept, the blocks and the
//! draws are vanilla's.
use crate::{BlockStateId, StructureRandom};
use horizon_world::native_worldgen_hooks::{AlterGroundTree, GroundAccess, ModGroundProvider};
use std::sync::Arc;

/// A mod's ground provider: NeoForge's `AlterGroundEvent.StateProvider`.
/// Implemented once, kept by the game, deterministic and stateless.
pub trait GroundStateProvider: Send + Sync + 'static {
    /// `StateProvider.getState`: the state to place at `pos`, or None to try
    /// the next position down the column, as the decorator's own provider
    /// answers where it finds no ground it replaces. Draw only from
    /// `level.random()`, so the same seed gives the same ground.
    fn state_at(&self, level: &mut dyn GroundLevel, pos: [i32; 3]) -> Option<BlockStateId>;
}

/// What a ground provider reads while the decorator asks it: vanilla's
/// `WorldGenLevel` as far as ground needs it, and the decorator's random.
pub trait GroundLevel {
    /// `BlockGetter.getBlockState`: the block at `pos` in the region the tree
    /// decorates.
    fn block_state(&mut self, pos: [i32; 3]) -> BlockStateId;
    /// The decorator's own provider at `pos`
    /// (`BlockStateProvider.getOptionalState`), drawing from the same random:
    /// what [`GroundProvider::Decorator`] answers.
    fn decorator_state(&mut self, pos: [i32; 3]) -> Option<BlockStateId>;
    /// The decorator's random (`TreeDecorator.Context.random()`): a provider's
    /// draws come between the decorator's own, in the order it asks.
    fn random(&mut self) -> &mut dyn StructureRandom;
}

/// The provider a tree decorator places its ground with:
/// `AlterGroundEvent.getStateProvider`.
#[derive(Clone)]
// exhaustive: the decorator's own or a mod's; a third kind is a contract change
pub enum GroundProvider {
    /// The decorator's own (`AlterGroundDecorator.provider`).
    Decorator,
    /// A mod's, in place of the one before. To change the ground only where
    /// it likes, a mod wraps the provider it found and asks it for the rest.
    Mod(Arc<dyn GroundStateProvider>),
}

/// The same provider: both the decorator's own, or the same mod provider object.
impl PartialEq for GroundProvider {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Decorator, Self::Decorator) => true,
            (Self::Mod(a), Self::Mod(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }
}

impl GroundProvider {
    /// This provider's state at `pos`, or None to try the next position down.
    pub fn state_at(&self, level: &mut dyn GroundLevel, pos: [i32; 3]) -> Option<BlockStateId> {
        match self {
            Self::Decorator => level.decorator_state(pos),
            Self::Mod(provider) => provider.state_at(level, pos),
        }
    }
}

/// `AlterGroundDecorator.place` is about to alter the ground around
/// `positions`: the provider it places with is `provider`.
#[inline(never)]
pub fn native_alter_ground(
    context: &horizon_world::native_load_context::WorldgenContext,
    logs: &[[i32; 3]],
    leaves: &[[i32; 3]],
    roots: &[[i32; 3]],
    positions: &[[i32; 3]],
    provider: &mut GroundProvider,
) {
    crate::dispatch_alter_ground(
        &crate::OwningWorldgen::new(context),
        logs,
        leaves,
        roots,
        positions,
        provider,
    );
}

/// The engine's `alter_ground` callback: `replaced` is set only when a mod put
/// its provider in place of the decorator's own. Nothing is built or read while
/// no mod listens.
// hidden: the executable installs it; mods never call it
#[doc(hidden)]
pub fn alter_ground(tree: &AlterGroundTree<'_>, replaced: &mut Option<Arc<dyn ModGroundProvider>>) {
    if !crate::listened(crate::generated::HookId::AlterGround) {
        return;
    }
    let Some(context) = horizon_world::native_worldgen_hooks::context() else {
        return;
    };
    let mut provider = GroundProvider::Decorator;
    native_alter_ground(
        &context,
        tree.logs,
        tree.leaves,
        tree.roots,
        tree.positions,
        &mut provider,
    );
    if let GroundProvider::Mod(provider) = provider {
        *replaced = Some(Arc::new(Engine(provider)));
    }
}

/// A mod's provider, as the decorator asks it.
struct Engine(Arc<dyn GroundStateProvider>);
impl ModGroundProvider for Engine {
    fn state_at(&self, access: &mut dyn GroundAccess, pos: [i32; 3]) -> Option<BlockStateId> {
        self.0.state_at(&mut Level(access), pos)
    }
}

/// The decorator's run, as a provider reads it.
struct Level<'a>(&'a mut dyn GroundAccess);
impl GroundLevel for Level<'_> {
    fn block_state(&mut self, pos: [i32; 3]) -> BlockStateId {
        self.0.block_state(pos)
    }
    fn decorator_state(&mut self, pos: [i32; 3]) -> Option<BlockStateId> {
        self.0.decorator_state(pos)
    }
    fn random(&mut self) -> &mut dyn StructureRandom {
        self
    }
}
impl StructureRandom for Level<'_> {
    fn next_int_bound(&mut self, bound: i32) -> i32 {
        self.0.next_int_bound(bound)
    }
    fn next_long(&mut self) -> i64 {
        self.0.next_long()
    }
    fn next_float(&mut self) -> f32 {
        self.0.next_float()
    }
    fn next_double(&mut self) -> f64 {
        self.0.next_double()
    }
    fn next_boolean(&mut self) -> bool {
        self.0.next_boolean()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AlterGroundEvent, Hooks, ModEntry, WorldgenHost, testing};
    use horizon_world::native_load_context::{DataLoadOrigin, LoadContext, LoadInputs};
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    const GRASS: BlockStateId = BlockStateId(1);
    const PODZOL: BlockStateId = BlockStateId(2);
    const MOSS: BlockStateId = BlockStateId(3);
    const MUD: BlockStateId = BlockStateId(4);

    /// The server's context, published as a server's resource load does.
    fn publish() {
        let registries =
            BTreeMap::from([("block".to_owned(), vec!["minecraft:moss_block".to_owned()])]);
        let root = std::env::temp_dir().join("horizon-alter-ground-no-settings");
        let load = LoadContext::new(
            DataLoadOrigin::ServerDataLoad,
            Arc::new(LoadInputs::new(registries, root)),
            [],
        );
        horizon_world::native_worldgen_hooks::publish_context(Arc::new(load.worldgen_context([])));
    }

    /// A tree on grass whose decorator turns grass to podzol, counting its draws.
    #[derive(Default)]
    struct Meadow {
        draws: u32,
    }
    impl GroundAccess for Meadow {
        fn block_state(&mut self, _: [i32; 3]) -> BlockStateId {
            GRASS
        }
        fn decorator_state(&mut self, pos: [i32; 3]) -> Option<BlockStateId> {
            self.draws += 1;
            (self.block_state(pos) == GRASS).then_some(PODZOL)
        }
        fn next_int_bound(&mut self, _: i32) -> i32 {
            self.draws += 1;
            0
        }
        fn next_long(&mut self) -> i64 {
            self.draws += 1;
            0
        }
        fn next_float(&mut self) -> f32 {
            self.draws += 1;
            0.0
        }
        fn next_double(&mut self) -> f64 {
            self.draws += 1;
            0.0
        }
        fn next_boolean(&mut self) -> bool {
            self.draws += 1;
            false
        }
    }

    const LOGS: [[i32; 3]; 2] = [[0, 64, 0], [0, 65, 0]];
    fn tree() -> AlterGroundTree<'static> {
        AlterGroundTree {
            logs: &LOGS,
            leaves: &[],
            roots: &[],
            positions: &LOGS[..1],
        }
    }
    fn alter() -> Option<Arc<dyn ModGroundProvider>> {
        publish();
        let mut replaced = None;
        alter_ground(&tree(), &mut replaced);
        replaced
    }

    /// Moss where the provider it found places podzol: a mod that wraps the one before.
    struct Moss(GroundProvider);
    impl GroundStateProvider for Moss {
        fn state_at(&self, level: &mut dyn GroundLevel, pos: [i32; 3]) -> Option<BlockStateId> {
            let found = self.0.state_at(level, pos);
            if found == Some(PODZOL) {
                Some(MOSS)
            } else {
                found
            }
        }
    }
    /// Mud where the provider it found places moss, after one draw of its own.
    struct Mud(GroundProvider);
    impl GroundStateProvider for Mud {
        fn state_at(&self, level: &mut dyn GroundLevel, pos: [i32; 3]) -> Option<BlockStateId> {
            level.random().next_int_bound(2);
            let found = self.0.state_at(level, pos);
            if found == Some(MOSS) {
                Some(MUD)
            } else {
                found
            }
        }
    }

    /// Swaps in its provider, recording what the event and the context told it.
    struct Swaps {
        wrap: fn(GroundProvider) -> Arc<dyn GroundStateProvider>,
        heard: Arc<Mutex<Vec<String>>>,
    }
    #[crate::__subscriptions]
    impl Hooks for Swaps {
        fn alter_ground(&self, context: &dyn WorldgenHost, event: &mut AlterGroundEvent<'_>) {
            let blocks = context.registry("block").map(|names| names.join(","));
            self.heard.lock().unwrap().push(format!(
                "{:?} {:?} {}",
                event.positions,
                blocks,
                matches!(event.provider, GroundProvider::Decorator)
            ));
            *event.provider = GroundProvider::Mod((self.wrap)(event.provider.clone()));
        }
    }
    /// Listens and leaves the decorator's own provider.
    struct Keeps;
    #[crate::__subscriptions]
    impl Hooks for Keeps {
        fn alter_ground(&self, _: &dyn WorldgenHost, event: &mut AlterGroundEvent<'_>) {
            assert!(matches!(event.provider, GroundProvider::Decorator));
        }
    }

    #[test]
    fn with_no_listener_or_the_decorators_own_kept_nothing_is_replaced() {
        testing::with_mods(Vec::<ModEntry>::new().into(), || assert!(alter().is_none()));
        testing::with_mods(vec![ModEntry::new("keeps", Box::new(Keeps))].into(), || {
            assert!(alter().is_none());
        });
    }

    #[test]
    fn a_mod_swaps_the_provider_and_sees_the_tree_and_the_servers_registries() {
        let heard = Arc::new(Mutex::new(Vec::new()));
        let mods = vec![ModEntry::new(
            "moss",
            Box::new(Swaps {
                wrap: |found| Arc::new(Moss(found)),
                heard: heard.clone(),
            }),
        )];
        let replaced = testing::with_mods(mods.into(), alter).expect("the mod's provider");
        assert_eq!(
            *heard.lock().unwrap(),
            [r#"[[0, 64, 0]] Some("minecraft:moss_block") true"#]
        );
        let mut meadow = Meadow::default();
        assert_eq!(replaced.state_at(&mut meadow, [3, 63, 0]), Some(MOSS));
        // The wrapped decorator's own provider drew once, as it does alone.
        assert_eq!(meadow.draws, 1);
    }

    #[test]
    fn each_mod_wraps_the_provider_the_one_before_left() {
        let heard = Arc::new(Mutex::new(Vec::new()));
        let mods = vec![
            ModEntry::new(
                "moss",
                Box::new(Swaps {
                    wrap: |found| Arc::new(Moss(found)),
                    heard: heard.clone(),
                }),
            ),
            ModEntry::new(
                "mud",
                Box::new(Swaps {
                    wrap: |found| Arc::new(Mud(found)),
                    heard: heard.clone(),
                }),
            ),
        ];
        let replaced = testing::with_mods(mods.into(), alter).expect("the mods' provider");
        let decorator = r#"[[0, 64, 0]] Some("minecraft:moss_block")"#;
        assert_eq!(
            *heard.lock().unwrap(),
            [format!("{decorator} true"), format!("{decorator} false")]
        );
        let mut meadow = Meadow::default();
        assert_eq!(replaced.state_at(&mut meadow, [3, 63, 0]), Some(MUD));
        // Mud's own draw, then the decorator's, in the order the providers asked.
        assert_eq!(meadow.draws, 2);
    }
}
