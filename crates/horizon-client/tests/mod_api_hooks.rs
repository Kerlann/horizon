//! A mod crate's view of the mod API: `Hooks` is implemented with the names `mod_api` exports.
//! The test is that this file builds with the exported event types.
use mod_api::{
    self, ClientHost, Hooks, InputKeyEvent, KeyAction, ModEntry, ScreenExclusionAreasEvent,
};

#[derive(Default)]
struct Keys;

#[mod_api::horizon_mod]
impl Hooks for Keys {
    fn input_key(&self, host: &mut dyn ClientHost, event: &mut InputKeyEvent<'_>) {
        let _: (&str, KeyAction) = (event.key, event.action);
        if let Some(menu) = host
            .local_player()
            .and_then(|player| player.container_menu(host))
        {
            let _ = menu.slot_count(host);
            let _ = menu.slot(host, 0);
            let _ = menu.carried(host);
        }
    }
}

#[test]
fn a_mod_implements_the_key_hooks() {
    let _: ModEntry = ModEntry::new("keys", hooks());
}

mod sidebar {
    use super::*;

    #[derive(Default)]
    struct Sidebar;

    #[mod_api::horizon_mod]
    impl Hooks for Sidebar {
        fn screen_exclusion_areas(
            &self,
            _host: &dyn ClientHost,
            event: &mut ScreenExclusionAreasEvent<'_>,
        ) {
            if event.screen.id == "inventory" {
                event.areas.push([0.0, 0.0, 40.0, event.screen.rect[3]]);
            }
        }
    }

    #[test]
    fn a_mod_adds_screen_exclusion_areas() {
        let _: ModEntry = ModEntry::new("sidebar", hooks());
    }
}

#[test]
fn client_packet_hands_a_mod_the_type_and_custom_payload() {
    use mod_api::{
        ClientPacket, ClientPacketEvent, ClientPayload, Host, Identifier, dispatch_client_packet,
    };
    use std::sync::Mutex;
    static HEARD: Mutex<Vec<(String, Vec<u8>)>> = Mutex::new(Vec::new());
    struct TestHost;
    impl Host for TestHost {}
    impl ClientHost for TestHost {}
    // A plugin-channel listener using only the mod API's plain data.
    #[derive(Default)]
    struct Channels;
    #[mod_api::horizon_mod]
    impl Hooks for Channels {
        fn client_packet(&self, _host: &mut dyn ClientHost, event: &mut ClientPacketEvent<'_>) {
            if let Some(p) = &event.packet.custom_payload {
                HEARD
                    .lock()
                    .unwrap()
                    .push((p.channel.clone(), p.data.clone()));
            }
        }
    }
    mod_api::testing::with_mods(vec![ModEntry::new("channels", hooks())].into(), || {
        dispatch_client_packet(
            &mut TestHost,
            &ClientPacket::new(Identifier::parse("minecraft:ping").unwrap(), None),
        );
        let payload = ClientPayload::new("voicechat:secret".into(), vec![1, 2, 3]);
        dispatch_client_packet(
            &mut TestHost,
            &ClientPacket::new(
                Identifier::parse("minecraft:custom_payload").unwrap(),
                Some(payload),
            ),
        );
    });
    assert_eq!(
        *HEARD.lock().unwrap(),
        [("voicechat:secret".to_string(), vec![1, 2, 3])]
    );
}

// Fluid acceptance is scoped to a FLUIDS provider, not a global Hooks method.
// Its default/refused/accepted writes are covered with a real Game backend in
// game/machines/native_fluids.rs::tests::a_scoped_tank_can_refuse_a_fluid.
