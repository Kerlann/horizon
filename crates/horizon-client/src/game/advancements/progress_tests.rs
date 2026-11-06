use super::*;
use crate::game::{
    native_entity_refs::tests::run_fresh_cases, native_guest_turn_tests::with_guests,
};
use mod_api::{
    AdvancementProgress, Hooks, ModEntry, Mods, PlayerAdvancementProgressEvent, ServerHost,
};
use std::sync::Mutex;

const ID: &str = "testmod:progress";
const DATE: &str = "2020-01-02 03:04:05 +0000";

#[derive(Debug)]
struct Observation {
    player: u128,
    criterion: String,
    kind: NativeProgressType,
    progress: AdvancementProgress,
    rewarded: Option<bool>,
}

struct Observer(Arc<Mutex<Vec<Observation>>>);
#[mod_api::__subscriptions]
impl Hooks for Observer {
    fn player_advancement_progress(
        &self,
        host: &mut dyn ServerHost,
        event: &mut PlayerAdvancementProgressEvent<'_>,
    ) {
        if event.advancement != ID {
            return;
        }
        // Read through the Host during dispatch: no progress storage borrow is retained.
        let rewarded = host
            .player(event.player)
            .and_then(|player| player.has_discovered_recipe(host, "testmod:reward"));
        self.0.lock().unwrap().push(Observation {
            player: event.player,
            criterion: event.criterion.to_owned(),
            kind: event.progress_type,
            progress: event.progress.clone(),
            rewarded,
        });
    }
}

fn assert_progress(progress: &AdvancementProgress, obtained: [bool; 3], groups: u32, done: bool) {
    assert_eq!(
        progress
            .criteria
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        ["alpha", "beta", "gamma"]
    );
    assert_eq!(
        progress
            .criteria
            .iter()
            .map(|c| c.obtained_at.is_some())
            .collect::<Vec<_>>(),
        obtained
    );
    assert_eq!(
        progress.requirements,
        vec![vec!["alpha", "beta"], vec!["gamma"]]
    );
    assert_eq!(progress.completed_requirements, groups);
    assert_eq!(progress.done, done);
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn progress_events_capture_each_players_full_mutation_state() {
    if run_fresh_cases(
        "game::advancements::progress_tests::progress_events_capture_each_players_full_mutation_state",
        "HORIZON_PROGRESS_SNAPSHOT_CHILD",
        &["acting", "first", "reload"],
    ) {
        return;
    }
    let observed = Arc::new(Mutex::new(Vec::new()));
    let mods: Mods = vec![ModEntry::new(
        "observer",
        Box::new(Observer(observed.clone())),
    )]
    .into();
    mod_api::__private::install(mods);
    let scratch = std::env::current_dir().unwrap().join("progress-data");
    let dir = scratch.join("data/testmod/advancement");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("progress.json"),
        r#"{
        "criteria": {
            "alpha": {"trigger": "minecraft:tick"},
            "beta": {"trigger": "minecraft:tick"},
            "gamma": {"trigger": "minecraft:tick"}
        },
        "requirements": [["alpha", "beta"], ["gamma"]],
        "rewards": {"recipes": ["testmod:reward"]}
    }"#,
    )
    .unwrap();
    let case = std::env::var("HORIZON_PROGRESS_SNAPSHOT_CHILD").unwrap();
    with_guests(|game| {
        if case == "acting" {
            game.adv.tree = Arc::new(Tree::load(&scratch.join("data")));
            // Another player has unrelated progress: the acting snapshot must not use it.
            game.lan
                .as_mut()
                .unwrap()
                .hub
                .guest_mut(13)
                .unwrap()
                .advancements
                .grant(ID, "gamma", DATE);
            game.as_guest(12, |game| {
                game.adv.progress.grant(ID, "alpha", DATE);
                let index = game.adv.tree.by_id[ID];
                assert!(game.grant_advancement(index, Some("beta")));
                assert!(
                    !game.grant_advancement(index, Some("beta")),
                    "no duplicate event"
                );
                assert!(game.grant_advancement(index, Some("gamma")));
                assert!(game.revoke_advancement(index, Some("alpha")));
                assert!(game.revoke_advancement(index, Some("beta")));
                assert!(game.revoke_advancement(index, Some("gamma")));
                assert!(
                    !game.revoke_advancement(index, Some("gamma")),
                    "no no-op event"
                );
            })
            .unwrap();
            let events = observed.lock().unwrap();
            assert_eq!(events.len(), 5);
            assert!(events.iter().all(|event| event.player == 12));
            assert_eq!(
                events
                    .iter()
                    .map(|event| (event.criterion.as_str(), event.kind))
                    .collect::<Vec<_>>(),
                vec![
                    ("beta", NativeProgressType::Grant),
                    ("gamma", NativeProgressType::Grant),
                    ("alpha", NativeProgressType::Revoke),
                    ("beta", NativeProgressType::Revoke),
                    ("gamma", NativeProgressType::Revoke),
                ]
            );
            assert_progress(&events[0].progress, [true, true, false], 1, false);
            assert_progress(&events[1].progress, [true, true, true], 2, true);
            assert_progress(&events[2].progress, [false, true, true], 2, true);
            assert_progress(&events[3].progress, [false, false, true], 1, false);
            assert_progress(&events[4].progress, [false, false, false], 0, false);
            assert_eq!(
                events[0].progress.criteria[0].obtained_at.as_deref(),
                Some(DATE)
            );
            assert_eq!(
                events[0].progress.criteria[1].obtained_at,
                events[1].progress.criteria[1].obtained_at
            );
            assert_eq!(
                events[1].rewarded,
                Some(false),
                "completion is captured before rewards"
            );
        } else {
            let mut sync = horizon_client::lan::data::SyncData::empty();
            sync.advancements = horizon_client::lan::ServerAdvancements::load(
                &scratch,
                &horizon_world::loot::PackOverlay::NONE,
                Default::default(),
                Default::default(),
            );
            let reload = case == "reload";
            let host = game.lan.as_mut().unwrap();
            for guest in &mut host.hub.guests {
                guest.advancements_loaded = reload;
            }
            host.hub
                .guest_mut(13)
                .unwrap()
                .advancements
                .grant(ID, "gamma", DATE);
            host.hub.mods_loaded = reload;
            host.hub.reload_resources(Arc::new(sync));
            if reload {
                game.run_native_advancement_grants();
            } else {
                game.tick_lan(false);
            }
            let events = observed.lock().unwrap();
            assert_eq!(
                events.len(),
                4,
                "one mutation at a time, including each guest's prior progress"
            );
            assert!(
                events
                    .iter()
                    .all(|event| event.kind == NativeProgressType::Grant
                        && event.rewarded == Some(false))
            );
            let first: Vec<_> = events.iter().filter(|event| event.player == 12).collect();
            assert_eq!(
                first
                    .iter()
                    .map(|event| event.criterion.as_str())
                    .collect::<Vec<_>>(),
                ["alpha", "beta", "gamma"]
            );
            assert_progress(&first[0].progress, [true, false, false], 1, false);
            assert_progress(&first[1].progress, [true, true, false], 1, false);
            assert_progress(&first[2].progress, [true, true, true], 2, true);
            let second: Vec<_> = events.iter().filter(|event| event.player == 13).collect();
            assert_eq!(second.len(), 1);
            assert_eq!(second[0].criterion, "alpha");
            assert_progress(&second[0].progress, [true, false, true], 2, true);
            assert_eq!(
                second[0].progress.criteria[2].obtained_at.as_deref(),
                Some(DATE)
            );
            for conn in [12, 13] {
                assert!(
                    game.lan
                        .as_ref()
                        .unwrap()
                        .hub
                        .guest(conn)
                        .unwrap()
                        .recipe_book
                        .knows("testmod:reward")
                );
            }
        }
    });
    std::fs::remove_dir_all(scratch).unwrap();
}
