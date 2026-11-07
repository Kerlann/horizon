//! One-shot spawn injection while a production tick owns a lifted batch.
use crate::game::Game;
use std::cell::Cell;

type Spawn = fn(&mut Game<'_>);
thread_local! {
    static AFTER_TAKE: Cell<Option<Spawn>> = const { Cell::new(None) };
}

pub(in crate::game) fn with_spawn(spawn: Spawn, tick: impl FnOnce()) {
    assert!(AFTER_TAKE.replace(Some(spawn)).is_none());
    tick();
    assert!(AFTER_TAKE.take().is_none(), "tick did not lift its batch");
}

pub(in crate::game) fn after_take(game: &mut Game<'_>) {
    if let Some(spawn) = AFTER_TAKE.take() {
        spawn(game);
    }
}

/// Storage adapters for the same production lifecycle walk across entity kinds.
pub(in crate::game) struct LifecycleFixture {
    pub seed: fn(&mut Game<'_>, &[(u32, [f64; 3])]),
    pub ids: fn(&Game<'_>) -> Vec<u32>,
    pub target: fn(u32) -> crate::game::commands::Target,
    pub unloads_with_chunk: bool,
    pub remote: Option<RemoteLifecycle>,
}

pub(in crate::game) struct RemoteLifecycle {
    pub prepare: fn(&mut Game<'_>),
    pub apply: fn(&mut Game<'_>, u32, u32),
}

impl LifecycleFixture {
    /// Adapters seed owners and read ordered identities; all exchanges and
    /// removals run through production entry points, never a substitute model.
    pub fn run(self) {
        crate::game::native_entity_refs::tests::with_game(|game| {
            let root = std::env::temp_dir().join(format!(
                "horizon-lifecycle-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            game.world_root = root.clone();
            (self.seed)(
                game,
                &[
                    (101, [1.0, 80.0, 1.0]),
                    (102, [17.0, 80.0, 1.0]),
                    (103, [33.0, 80.0, 1.0]),
                    (104, [2.0, 80.0, 1.0]),
                ],
            );
            assert_eq!((self.ids)(game), [101, 102, 103, 104]);
            game.kill_target((self.target)(102));
            assert_eq!((self.ids)(game), [101, 103, 104], "command removal");
            game.entity_chunks(&[(0, 0)].into_iter().collect(), true);
            let active = if self.unloads_with_chunk {
                vec![103]
            } else {
                vec![101, 103, 104]
            };
            assert_eq!(
                (self.ids)(game),
                active,
                "selected versus unselected chunk unload"
            );
            game.enter_level(crate::game::Dim::Nether).unwrap();
            assert!(
                (self.ids)(game).is_empty(),
                "new dimension inherited the active owners"
            );
            (self.seed)(game, &[(201, [1.0, 80.0, 1.0]), (202, [33.0, 80.0, 1.0])]);
            assert_eq!((self.ids)(game), [201, 202]);
            game.enter_level(crate::game::Dim::Overworld).unwrap();
            assert_eq!((self.ids)(game), active, "active owner restore");
            game.enter_level(crate::game::Dim::Nether).unwrap();
            assert_eq!((self.ids)(game), [201, 202], "parked owner restore");
            game.enter_level(crate::game::Dim::Overworld).unwrap();
            assert_eq!((self.ids)(game), active, "second active restore");
            if let Some(remote) = self.remote {
                (remote.prepare)(game);
                (self.seed)(
                    game,
                    &[
                        (301, [1.0, 80.0, 1.0]),
                        (302, [17.0, 80.0, 1.0]),
                        (303, [33.0, 80.0, 1.0]),
                    ],
                );
                (remote.apply)(game, 302, 303);
                assert_eq!((self.ids)(game), [301, 303], "remote update and removal");
            }
            let _ = std::fs::remove_dir_all(root);
        });
    }
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn lifecycle_fixture_walks_fangs_owners() {
    LifecycleFixture {
        seed: |game, entries| {
            game.fangs.list.clear();
            for &(id, at) in entries {
                game.insert_fangs(id, at, 0.0, 0, 0);
            }
        },
        ids: |game| game.fangs.list.iter().map(|f| f.id).collect(),
        target: crate::game::commands::Target::Fangs,
        unloads_with_chunk: true,
        remote: None,
    }
    .run();
}
