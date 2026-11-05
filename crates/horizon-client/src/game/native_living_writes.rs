//! Writes to a living entity for native server mods, through a `LivingRef`
//! (docs/modding/src/how-to/native-mods.md, "Entity and level handles"): `LivingEntity.swing`,
//! the arrows and bee stingers stuck in a player (`setArrowCount`,
//! `setStingerCount`, `removeArrowTime`, `removeStingerTime`),
//! `Mob.setLeftHanded`, `Mob.setCanPickUpLoot` and `useItemRemaining`. Reads
//! return None and writes false for a stale handle, on a client, and for a
//! kind Horizon keeps no such state for (each doc says which).
use super::{Game, combat::Stuck, commands::Target};
use horizon_client::first_person::{Hand, SwingAnimation};
use mod_api::LivingRef;

impl Game<'_> {
    /// The UUID of the player a living handle names.
    fn native_living_player(&mut self, living: LivingRef) -> Option<u128> {
        match self.native_living_target(living)? {
            Target::Player => Some(self.native_acting_id()),
            Target::Guest(conn) => self.command_guest(conn).map(|g| g.uuid),
            _ => None,
        }
    }
    /// What is stuck in the player a handle names.
    fn native_living_stuck(&mut self, living: LivingRef) -> Option<&mut Stuck> {
        match self.native_living_target(living)? {
            Target::Player => Some(&mut self.combat.stuck),
            Target::Guest(conn) => Some(&mut self.guest_combat.entry(conn).or_default().stuck),
            _ => None,
        }
    }
    /// The mob a living handle names.
    fn native_living_mob(&mut self, living: LivingRef) -> Option<&mut horizon_client::mobs::Mob> {
        let Target::Mob(id) = self.native_living_target(living)? else {
            return None;
        };
        self.mobs.iter_mut().find(|m| m.id == id)
    }

    /// `LivingEntity.swing(hand, DEFAULT, sendToSwingingEntity)`: starts the
    /// arm swing (shortened by haste, lengthened by mining fatigue) unless
    /// one is under way and past its first half, and tells the players
    /// around at the end of the tick, and the swinger too when
    /// `send_to_self`. True when a swing started. Mobs and players; false
    /// for armor stands and the dragon, and for a player other than the
    /// acting one while another player acts.
    pub(super) fn native_living_swing(
        &mut self,
        living: LivingRef,
        hand: Hand,
        send_to_self: bool,
    ) -> bool {
        match self.native_living_target(living) {
            Some(Target::Mob(id)) => {
                self.mobs.iter_mut().find(|m| m.id == id).is_some_and(|m| {
                    m.swing_hand_modified(hand == Hand::Off, SwingAnimation::DEFAULT)
                })
            }
            Some(Target::Player | Target::Guest(_)) => {
                let Some(uuid) = self.native_living_player(living) else {
                    return false;
                };
                let mut started = false;
                self.native_as_player(uuid, |game| {
                    let before = game.swings_started;
                    game.swing_hand(hand, SwingAnimation::DEFAULT);
                    started = game.swings_started != before;
                    let current = game.swing.current;
                    if let (true, true, Some(c), Some(id)) =
                        (started, send_to_self, current, game.lan_acting_entity)
                    {
                        game.guest_packets.push(super::lan_actions::swing_packet(
                            id as i32,
                            c.hand,
                            c.animation,
                        ));
                    }
                });
                started
            }
            _ => false,
        }
    }

    /// `LivingEntity.getArrowCount`: the arrows stuck in a player. None for
    /// the other kinds, which Horizon keeps no count for.
    pub(super) fn native_living_arrow_count(&mut self, living: LivingRef) -> Option<i32> {
        Some(self.native_living_stuck(living)?.arrows)
    }
    /// `LivingEntity.setArrowCount`: players around see the arrows (vanilla
    /// does not limit the count). False for the other kinds.
    pub(super) fn native_living_set_arrow_count(&mut self, living: LivingRef, count: i32) -> bool {
        self.native_living_stuck(living)
            .map(|s| s.arrows = count)
            .is_some()
    }
    /// `LivingEntity.getStingerCount`: the bee stingers stuck in a player.
    /// None for the other kinds.
    pub(super) fn native_living_stinger_count(&mut self, living: LivingRef) -> Option<i32> {
        Some(self.native_living_stuck(living)?.stingers)
    }
    /// `LivingEntity.setStingerCount`: players around see the stingers. False
    /// for the other kinds.
    pub(super) fn native_living_set_stinger_count(
        &mut self,
        living: LivingRef,
        count: i32,
    ) -> bool {
        self.native_living_stuck(living)
            .map(|s| s.stingers = count)
            .is_some()
    }
    /// `LivingEntity.removeArrowTime`: the ticks until the next arrow falls
    /// out of a player; at 0 or less it restarts at `20 * (30 - arrows)`.
    /// False for the other kinds.
    pub(super) fn native_living_set_arrow_removal_ticks(
        &mut self,
        living: LivingRef,
        ticks: i32,
    ) -> bool {
        self.native_living_stuck(living)
            .map(|s| s.remove_arrow_time = ticks)
            .is_some()
    }
    /// `LivingEntity.removeStingerTime`, as `set_arrow_removal_ticks` for the
    /// stingers.
    pub(super) fn native_living_set_stinger_removal_ticks(
        &mut self,
        living: LivingRef,
        ticks: i32,
    ) -> bool {
        self.native_living_stuck(living)
            .map(|s| s.remove_stinger_time = ticks)
            .is_some()
    }

    /// `Mob.isLeftHanded`: whether the mob holds things in its left hand (the
    /// mob flags in its entity data). None for players and the other kinds.
    pub(super) fn native_living_left_handed(&mut self, living: LivingRef) -> Option<bool> {
        Some(self.native_living_mob(living)?.extra.left_handed)
    }
    /// `Mob.setLeftHanded`: sets bit 2 of the mob flags, which the players
    /// around get with the mob's next entity data and see in its swing and
    /// held items. Saved with the mob. False for players and the other kinds.
    pub(super) fn native_living_set_left_handed(
        &mut self,
        living: LivingRef,
        left_handed: bool,
    ) -> bool {
        self.native_living_mob(living)
            .map(|m| m.extra.left_handed = left_handed)
            .is_some()
    }
    /// `Mob.canPickUpLoot`, with the overrides of an allay (holding
    /// something) and a sulfur cube (with no block). None for players and
    /// the other kinds.
    pub(super) fn native_living_can_pick_up_loot(&mut self, living: LivingRef) -> Option<bool> {
        Some(self.native_living_mob(living)?.can_pick_up_loot())
    }
    /// `Mob.setCanPickUpLoot`: whether the mob picks up the items it sees
    /// (saved with it; clients are not told, as in vanilla). An allay and a
    /// sulfur cube decide for themselves. False for players and the other
    /// kinds.
    pub(super) fn native_living_set_can_pick_up_loot(
        &mut self,
        living: LivingRef,
        can_pick_up: bool,
    ) -> bool {
        self.native_living_mob(living)
            .map(|m| m.extra.pick_up_loot = can_pick_up)
            .is_some()
    }

    /// `LivingEntity.getUseItemRemainingTicks`: the ticks left of the item a
    /// player uses, 0 when they use none. None for the other kinds (a mob's
    /// item use is a counter of its goal, not this timer).
    pub(super) fn native_living_item_use_remaining(&self, living: LivingRef) -> Option<i32> {
        match self.native_living_target(living)? {
            Target::Player => Some(super::item_use::remaining_of(self.using, self.use_duration)),
            Target::Guest(conn) => Some(self.lan.as_ref()?.play.get(&conn)?.use_remaining()),
            _ => None,
        }
    }
    /// Sets `LivingEntity.useItemRemaining` of the item a player uses (the
    /// way a mod's tick moves the end of the use): at 1 the use completes on
    /// the next tick, at 0 or less it runs until the player lets go, as in
    /// vanilla. It moves the deadline the use-item Tick hook moves, so only
    /// the uses that count it (food, drink, mod items) follow it: shield, bow,
    /// trident, spear, instrument and bundle uses ignore it, and a call from
    /// inside the Tick hook itself is overwritten by that hook's result. False
    /// when the player uses nothing, and for the other kinds.
    pub(super) fn native_living_set_item_use_remaining(
        &mut self,
        living: LivingRef,
        ticks: i32,
    ) -> bool {
        match self.native_living_target(living) {
            Some(Target::Player) => {
                super::item_use::set_remaining(self.using, &mut self.use_duration, ticks)
            }
            Some(Target::Guest(conn)) => self
                .lan
                .as_mut()
                .and_then(|h| h.play.get_mut(&conn))
                .is_some_and(|p| p.set_use_remaining(ticks)),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::native_entity_refs::tests::with_game;
    use super::Stuck;
    use horizon_client::mobs::Mob;
    use horizon_core::geometry::Vec3;
    use mod_api::first_person::Hand;
    use mod_api::{EntityRef, NativeWorldEpoch};

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn a_mob_swings_and_keeps_its_hand_and_loot_flags() {
        with_game(|game| {
            for (id, kind) in [(12, "minecraft:zombie"), (13, "minecraft:cow")] {
                let kind = game.res.mob_types().get(kind).unwrap().clone();
                game.mobs.push(Mob::new(id, kind, Vec3::ZERO, 0.0));
            }
            let zombie = game.native_living(game.native_entity(12).unwrap()).unwrap();
            assert_eq!(game.native_living_left_handed(zombie), Some(false));
            assert!(game.native_living_set_left_handed(zombie, true));
            assert!(game.mobs[0].extra.left_handed && game.mobs[0].main_arm_left());
            assert_eq!(game.native_living_left_handed(zombie), Some(true));
            assert!(game.native_living_set_left_handed(zombie, false));
            assert_eq!(game.native_living_left_handed(zombie), Some(false));
            assert!(game.native_living_set_can_pick_up_loot(zombie, true));
            assert!(game.mobs[0].extra.pick_up_loot);
            assert_eq!(game.native_living_can_pick_up_loot(zombie), Some(true));
            assert!(game.native_living_set_can_pick_up_loot(zombie, false));
            assert_eq!(game.native_living_can_pick_up_loot(zombie), Some(false));

            assert!(game.native_living_swing(zombie, Hand::Off, false));
            assert!(game.mobs[0].ai.swinging);
            assert_eq!(game.mobs[0].extra.swing_right_arm, Some(false));
            // In the first half of a swing it doesn't restart, past it does.
            game.mobs[0].ai.swing_time = 1;
            assert!(!game.native_living_swing(zombie, Hand::Main, false));
            game.mobs[0].ai.swing_time = 3;
            assert!(game.native_living_swing(zombie, Hand::Main, false));
            assert_eq!(game.mobs[0].ai.swing_time, -1);

            // A mob keeps no stuck arrows, item use timer or player state.
            assert_eq!(game.native_living_arrow_count(zombie), None);
            assert!(!game.native_living_set_arrow_count(zombie, 3));
            assert!(!game.native_living_set_stinger_count(zombie, 3));
            assert!(!game.native_living_set_arrow_removal_ticks(zombie, 3));
            assert_eq!(game.native_living_item_use_remaining(zombie), None);
            assert!(!game.native_living_set_item_use_remaining(zombie, 3));

            let stale =
                EntityRef::new(NativeWorldEpoch::new(zombie.entity().epoch().get() + 1), 12);
            let stale = mod_api::LivingRef::new(stale);
            assert!(!game.native_living_swing(stale, Hand::Main, true));
            assert!(!game.native_living_set_left_handed(stale, true));
            assert_eq!(game.native_living_can_pick_up_loot(stale), None);
        });
    }

    #[test]
    fn a_huge_stuck_count_ticks_without_overflow() {
        let mut stuck = Stuck {
            arrows: i32::MAX,
            stingers: i32::MIN + 1,
            ..Stuck::default()
        };
        stuck.tick();
        assert!(stuck.arrows >= i32::MAX - 1);
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn a_player_keeps_its_stuck_arrows_and_item_use_timer() {
        use super::super::lan::LanSettings;
        use horizon_client::lan::{Joined, NetConfig, PlayerSender, guest::Guest, hub::HubConfig};
        use horizon_protocol::packets::configuration::ClientInformation;
        use std::sync::{
            Arc,
            atomic::{AtomicI32, AtomicUsize},
        };
        with_game(|game| {
            let settings = LanSettings {
                port: 0,
                guest_commands: false,
                force_game_mode: false,
            };
            let net = NetConfig {
                listen: false,
                announce: false,
                lod_distance: 0,
                ..NetConfig::lan(0, String::new(), 2)
            };
            let config = HubConfig {
                default_game_mode: 0,
                guest_commands: false,
                force_game_mode: false,
                hardcore: false,
                view_distance: 2,
                simulation_distance: 2,
                dimension: game.dim.id().to_owned(),
                allow_flight: true,
                seed: 0,
                max_players: 2,
                integrated: true,
                levels: game.level_shapes,
            };
            game.lan_host(settings, net, config).unwrap();
            let (tx, _rx) = std::sync::mpsc::channel();
            let mut guest = Guest::new(
                Joined {
                    conn: 12,
                    uuid: 12,
                    name: "guest12".into(),
                    properties: Vec::new(),
                    info: ClientInformation::new(8),
                    brand: String::new(),
                    sender: PlayerSender::new(tx, Arc::new(AtomicUsize::new(0))),
                    latency: Arc::new(AtomicI32::new(0)),
                    address: "127.0.0.1:1".parse().unwrap(),
                    expected: false,
                },
                12,
                [3.0, 64.0, 5.0],
                10,
            );
            guest.placed = true;
            game.lan.as_mut().unwrap().hub.guests.push(guest);
            // A first turn starts a fresh combat record: take it first.
            game.as_guest(12, |_| {}).unwrap();
            let player = game.native_living(game.native_entity(12).unwrap()).unwrap();

            // Between its turns the guest's counts are in its combat record.
            assert_eq!(game.native_living_arrow_count(player), Some(0));
            assert!(game.native_living_set_arrow_count(player, 4));
            assert!(game.native_living_set_stinger_count(player, 2));
            assert!(game.native_living_set_arrow_removal_ticks(player, 7));
            assert!(game.native_living_set_stinger_removal_ticks(player, 9));
            let stuck = &game.guest_combat[&12].stuck;
            assert_eq!((stuck.arrows, stuck.stingers), (4, 2));
            assert_eq!((stuck.remove_arrow_time, stuck.remove_stinger_time), (7, 9));
            assert_eq!(game.native_living_arrow_count(player), Some(4));
            assert_eq!(game.native_living_stinger_count(player), Some(2));
            assert!(game.native_living_swing(player, Hand::Off, false));
            // No item in use: nothing to move.
            assert_eq!(game.native_living_item_use_remaining(player), Some(0));
            assert!(!game.native_living_set_item_use_remaining(player, 5));

            // The acting player's own state: an item in use for 3 of 32 ticks.
            game.as_guest(12, |game| {
                let player = game.native_living(game.native_entity(12).unwrap()).unwrap();
                assert_eq!(game.native_living_arrow_count(player), Some(4));
                game.using = Some((0, 3));
                game.use_duration = 32;
                assert_eq!(game.native_living_item_use_remaining(player), Some(29));
                assert!(game.native_living_set_item_use_remaining(player, 1));
                assert_eq!(game.use_duration, 4);
                assert_eq!(game.native_living_item_use_remaining(player), Some(1));
                game.using = None;
                // Its swing is queued for it when asked: not in the first half.
                game.swing.ticks = 0;
                game.guest_packets.clear();
                assert!(game.native_living_swing(player, Hand::Main, true));
                assert_eq!(game.guest_packets.len(), 1);
                game.swing.ticks = 1;
                assert!(!game.native_living_swing(player, Hand::Main, true));
                assert_eq!(game.guest_packets.len(), 1);
            })
            .unwrap();
        });
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn a_mobs_swing_reaches_clients_with_the_hand_asked_for() {
        use super::super::lan::LanSettings;
        use horizon_client::lan::{
            Joined, NetConfig, Out, PlayerSender, guest::Guest, hub::HubConfig,
        };
        use horizon_protocol::packets::{configuration::ClientInformation, play::Clientbound};
        use std::sync::{
            Arc,
            atomic::{AtomicI32, AtomicUsize},
        };
        with_game(|game| {
            let settings = LanSettings {
                port: 0,
                guest_commands: false,
                force_game_mode: false,
            };
            let net = NetConfig {
                listen: false,
                announce: false,
                lod_distance: 0,
                ..NetConfig::lan(0, String::new(), 2)
            };
            let config = HubConfig {
                default_game_mode: 0,
                guest_commands: false,
                force_game_mode: false,
                hardcore: false,
                view_distance: 2,
                simulation_distance: 2,
                dimension: game.dim.id().to_owned(),
                allow_flight: true,
                seed: 0,
                max_players: 2,
                integrated: true,
                levels: game.level_shapes,
            };
            game.lan_host(settings, net, config).unwrap();
            let (tx, rx) = std::sync::mpsc::channel();
            let mut guest = Guest::new(
                Joined {
                    conn: 12,
                    uuid: 12,
                    name: "guest12".into(),
                    properties: Vec::new(),
                    info: ClientInformation::new(8),
                    brand: String::new(),
                    sender: PlayerSender::new(tx, Arc::new(AtomicUsize::new(0))),
                    latency: Arc::new(AtomicI32::new(0)),
                    address: "127.0.0.1:1".parse().unwrap(),
                    expected: false,
                },
                12,
                [3.0, 64.0, 5.0],
                10,
            );
            guest.placed = true;
            game.lan.as_mut().unwrap().hub.guests.push(guest);
            let kind = game
                .res
                .mob_types()
                .get("minecraft:zombie")
                .unwrap()
                .clone();
            game.mobs
                .push(Mob::new(40, kind, Vec3::new(3.0, 64.0, 5.0), 0.0));
            let zombie = game.native_living(game.native_entity(40).unwrap()).unwrap();
            // The hand in the swing packet the flush sends after each swing.
            let mut sent = Vec::new();
            for natural in [false, true] {
                for hand in [Hand::Off, Hand::Main] {
                    game.mobs[0].ai.swinging = false;
                    game.mobs[0].ai.swing_time = 0;
                    if natural {
                        // A swing no hand was named for (a melee attack).
                        game.mobs[0].swing();
                    } else {
                        assert!(game.native_living_swing(zombie, hand, false));
                    }
                    game.lan_flush_effects();
                    while let Ok(Out::Packet(body)) = rx.try_recv() {
                        let mut reader = horizon_protocol::buf::Reader::new(&body);
                        let id = reader.varint().unwrap();
                        if let Ok(Clientbound::SwingAnimation(p)) =
                            Clientbound::decode(id, reader.rest())
                        {
                            sent.push(p.hand);
                        }
                    }
                    // The swing runs on, so the next one is a new start.
                    game.mobs[0].ai.swing_time = 3;
                    game.lan_flush_effects();
                    if natural {
                        break;
                    }
                }
            }
            assert_eq!(sent, [1, 0, 0]);
        });
    }
}
