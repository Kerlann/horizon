//! Actual source flags through the LAN tracker, protocol bytes and client bodies.
#![cfg(test)]
use super::Game;
use horizon_client::lan::entity::{Entities, Riding, States, Tracker, Viewer};
use horizon_protocol::{
    buf::Reader,
    entity_data::{Value, serializer},
    packets::play::Clientbound,
};
use mod_api::*;

fn packets(
    entities: &mut Entities,
    tracker: &mut Tracker,
    states: &States,
    kind: impl FnMut(&str) -> Option<i32>,
) -> Vec<Clientbound> {
    let position = states.as_slice()[0].position;
    let chunks = [(
        (position[0].floor() as i32) >> 4,
        (position[2].floor() as i32) >> 4,
    )]
    .into_iter()
    .collect();
    let mut out = Vec::new();
    entities.tick(
        &states.as_slice().iter().collect::<Vec<_>>(),
        &Riding::default(),
        &mut [Viewer {
            entity_id: -1,
            spectator: false,
            permission: 4,
            horizon: false,
            moved: true,
            position,
            view_distance: 8,
            chunks: &chunks,
            tracker,
            out: &mut out,
            gone: None,
        }],
        kind,
    );
    out.iter()
        .map(|bytes| {
            let mut reader = Reader::new(bytes);
            let id = reader.varint().unwrap();
            Clientbound::decode(id, reader.rest()).unwrap()
        })
        .collect()
}

fn raw(game: &Game<'_>, id: u32) -> bool {
    macro_rules! value {
        ($list:expr) => {
            if let Some(body) = $list.iter().find(|body| body.id == id) {
                return body.no_gravity;
            }
        };
    }
    value!(game.mobs);
    value!(game.dropped);
    value!(game.orbs);
    value!(game.armor_stands);
    value!(game.boats);
    value!(game.carts);
    value!(game.creatures.projectiles);
    value!(game.tnts);
    value!(game.tridents);
    value!(game.falling);
    panic!("missing actual replica body {id}")
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn no_gravity_all_families_publish_wire_pairing_reset_and_real_client_projection() {
    let mut deliveries = Vec::new();
    let mut expected = Vec::new();
    super::native_guest_turn_tests::with_guests(|game| {
        game.options.dedicated = Some(Vec::new());
        game.build_registries();
        game.set_allow_commands(true);
        game.world.chunks.insert(
            (0, 0),
            std::sync::Arc::new(horizon_client::chunk::Chunk::empty(0, 0)),
        );
        let epoch = game.native_lifecycle.epoch().unwrap();
        let mut targets = Vec::new();
        for (n, (kind, data)) in [
            ("pig", "NoAI:1b"),
            ("item", "Item:{id:\"minecraft:stone\",count:1}"),
            ("experience_orb", "Value:1s"),
            ("armor_stand", "Marker:0b"),
            ("oak_boat", ""),
            ("minecart", ""),
            ("snowball", ""),
            ("tnt", "fuse:1000s"),
            ("trident", ""),
            (
                "falling_block",
                "BlockState:{Name:\"minecraft:stone\"},Time:1",
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let tail = if data.is_empty() {
                String::new()
            } else {
                format!(",{data}")
            };
            assert!(
                game.run_command(&format!(
                    "summon {kind} 4.5 100 4.5 {{UUID:[I;734,0,0,{n}]{tail}}}"
                )),
                "{kind}"
            );
            let id = *game
                .persist
                .uuids
                .iter()
                .find(|(_, uuid)| **uuid == [734, 0, 0, n as i32])
                .unwrap()
                .0;
            targets.push(EntityRef::new(epoch, id));
        }
        let mut states = States::default();
        let mut snapshots = Vec::new();
        for value in [true, false] {
            for target in &targets {
                assert_eq!(
                    target.set_no_gravity(game, value),
                    WriteReceipt::Applied(true)
                );
            }
            assert!(
                game.dropped[0].needs_sync,
                "idle item metadata must not be described bare"
            );
            game.lan_entities(&mut states, false, 0, None);
            for target in &targets {
                let state = states
                    .as_slice()
                    .iter()
                    .find(|state| state.id == target.id() as i32)
                    .unwrap();
                assert!(
                    state
                        .defaults
                        .contains(&(5, serializer::BOOLEAN, Value::Bool(false)))
                );
                assert_eq!(
                    state.meta.iter().find(|entry| entry.0 == 5),
                    value.then_some(&(5, serializer::BOOLEAN, Value::Bool(true)))
                );
            }
            snapshots.push(states.as_slice().to_vec());
        }
        // Source bodies are now false. Applying actual pairing bytes must turn
        // them true; applying the later tracker reset must turn them false.
        let sync = game.lan.as_ref().unwrap().hub.lan.shared.sync();
        let mut entities = Entities::default();
        let mut tracker = Tracker::default();
        for (index, value) in [true, false].into_iter().enumerate() {
            states.clear();
            for state in &snapshots[index] {
                states.push(state.clone());
            }
            let mut seen = std::collections::BTreeSet::new();
            let mut delivery = Vec::new();
            for _ in 0..21 {
                for packet in packets(&mut entities, &mut tracker, &states, |kind| {
                    sync.builtin
                        .get("minecraft:entity_type")
                        .and_then(|registry| registry.id(kind))
                }) {
                    if let Clientbound::SetEntityData(ref data) = packet
                        && data
                            .values
                            .contains(&(5, serializer::BOOLEAN, Value::Bool(value)))
                    {
                        seen.insert(data.id as u32);
                    }
                    delivery.push(packet);
                }
            }
            assert_eq!(
                seen,
                targets.iter().map(|target| target.id()).collect(),
                "wire value {value}"
            );
            deliveries.push(delivery);
        }
        expected = targets.iter().map(|target| target.id()).collect();
    });
    for split_item_metadata in [false, true] {
        super::native_entity_refs::tests::with_client_game(|client| {
            use horizon_protocol::packets::play;
            let (pipe, _server) = horizon_protocol::connection::Pipe::pair();
            client.connect_local(pipe, "GravityMirror").unwrap();
            client
                .handle_packet(Clientbound::Login(play::Login {
                    player_id: -1,
                    hardcore: false,
                    levels: vec!["minecraft:overworld".into()],
                    max_players: 20,
                    chunk_radius: 8,
                    simulation_distance: 8,
                    reduced_debug_info: false,
                    show_death_screen: true,
                    limited_crafting: false,
                    spawn: play::SpawnInfo {
                        dimension_type: 0,
                        dimension: "minecraft:overworld".into(),
                        seed: 0,
                        game_type: 1,
                        previous_game_type: None,
                        debug: false,
                        flat: true,
                        last_death: None,
                        portal_cooldown: 0,
                        sea_level: 63,
                    },
                    online_mode: false,
                    enforces_secure_chat: false,
                }))
                .unwrap();
            for (index, delivery) in deliveries.iter().enumerate() {
                for packet in delivery {
                    if split_item_metadata
                        && let Clientbound::SetEntityData(data) = packet
                        && data
                            .values
                            .iter()
                            .any(|(slot, _, value)| *slot == 8 && matches!(value, Value::Item(_)))
                    {
                        // Also cover flag metadata arriving before the stack
                        // that constructs the real Item body.
                        let (flags, rest): (Vec<_>, Vec<_>) =
                            data.values.iter().cloned().partition(|entry| entry.0 == 5);
                        if !flags.is_empty() {
                            client
                                .handle_packet(Clientbound::SetEntityData(play::SetEntityData {
                                    id: data.id,
                                    values: flags,
                                }))
                                .unwrap();
                        }
                        client
                            .handle_packet(Clientbound::SetEntityData(play::SetEntityData {
                                id: data.id,
                                values: rest,
                            }))
                            .unwrap();
                    } else {
                        client.handle_packet(packet.clone()).unwrap();
                    }
                }
                assert_eq!(
                    [
                        client.mobs[0].id,
                        client.dropped[0].id,
                        client.orbs[0].id,
                        client.armor_stands[0].id,
                        client.boats[0].id,
                        client.carts[0].id,
                        client.creatures.projectiles[0].id,
                        client.tnts[0].id,
                        client.tridents[0].id,
                        client.falling[0].id
                    ]
                    .as_slice(),
                    expected.as_slice(),
                    "real client family selection and runtime IDs"
                );
                for id in &expected {
                    assert_eq!(
                        raw(client, *id),
                        index == 0,
                        "replica {id}, split {split_item_metadata}"
                    );
                }
            }
        });
    }
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn no_gravity_clean_idle_item_and_orb_wake_existing_wire_viewers() {
    super::native_guest_turn_tests::with_guests(|game| {
        game.options.dedicated = Some(Vec::new());
        game.build_registries();
        game.set_allow_commands(true);
        game.world.chunks.insert(
            (0, 0),
            std::sync::Arc::new(horizon_client::chunk::Chunk::empty(0, 0)),
        );
        // A loaded chunk outside the actual guests' simulation tickets.
        game.world.chunks.insert(
            (40, 0),
            std::sync::Arc::new(horizon_client::chunk::Chunk::empty(40, 0)),
        );
        assert!(
            game.run_command("summon item 644.5 100 4.5 {Item:{id:\"minecraft:stone\",count:1}}")
        );
        assert!(game.run_command("summon experience_orb 644.5 100 4.5 {Value:1s}"));
        let epoch = game.native_lifecycle.epoch().unwrap();
        let targets = [
            EntityRef::new(epoch, game.dropped[0].id),
            EntityRef::new(epoch, game.orbs[0].id),
        ];
        // Headless fixtures default to ticking everywhere. Install the same
        // distance-manager inputs as the actual guests' chunks instead.
        game.tickets
            .update(Some(horizon_client::ticket_levels::Sources {
                players: vec![(0, 0)],
                forced: Vec::new(),
                portal: Vec::new(),
                dragon: None,
                view_distance: 8,
                simulation_distance: 2,
            }));
        // Normal packet/state publication boundary, not manual dirty clearing.
        game.tick_lan_packets();
        game.tick_lan(true);
        assert!(!game.dropped[0].needs_sync);
        assert!(!game.orbs[0].needs_sync);
        let sync = game.lan.as_ref().unwrap().hub.lan.shared.sync();
        let mut entities = Entities::default();
        let mut tracker = Tracker::default();
        let mut states = States::default();
        game.lan_entities(&mut states, false, 0, None);
        packets(&mut entities, &mut tracker, &states, |kind| {
            sync.builtin
                .get("minecraft:entity_type")
                .and_then(|registry| registry.id(kind))
        });
        for target in targets {
            assert!(tracker.is_tracking(target.id() as i32));
        }
        for value in [true, false] {
            for target in targets {
                assert_eq!(
                    target.set_no_gravity(game, value),
                    WriteReceipt::Applied(true)
                );
            }
            assert!(game.dropped[0].needs_sync);
            assert!(game.orbs[0].needs_sync);
            game.lan_entities(&mut states, false, 0, None);
            for state in states.as_slice() {
                assert!(
                    state.idle,
                    "fixture must remain outside entity-ticking range"
                );
                assert!(state.needs_sync);
            }
            let delivered = packets(&mut entities, &mut tracker, &states, |kind| {
                sync.builtin
                    .get("minecraft:entity_type")
                    .and_then(|registry| registry.id(kind))
            });
            let seen = delivered
                .iter()
                .filter_map(|packet| match packet {
                    Clientbound::SetEntityData(data)
                        if data
                            .values
                            .contains(&(5, serializer::BOOLEAN, Value::Bool(value))) =>
                    {
                        Some(data.id as u32)
                    }
                    _ => None,
                })
                .collect::<std::collections::BTreeSet<_>>();
            assert_eq!(seen, targets.iter().map(|target| target.id()).collect());
            game.tick_lan_packets();
            game.tick_lan(true);
            assert!(!game.dropped[0].needs_sync);
            assert!(!game.orbs[0].needs_sync);
            for target in targets {
                assert_eq!(
                    target.set_no_gravity(game, value),
                    WriteReceipt::Applied(true)
                );
            }
            assert!(
                !game.dropped[0].needs_sync,
                "a no-op must not dirty an idle item"
            );
            assert!(
                !game.orbs[0].needs_sync,
                "a no-op must not dirty an idle orb"
            );
        }
    });
}
