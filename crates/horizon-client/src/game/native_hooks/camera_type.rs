use super::*;

/// The hook points. Each does nothing in Horizon.
impl Game<'_> {
    /// `key.togglePerspective` (F5) cycles the camera type: `previous` is the type it
    /// leaves, and `next` comes in as `CameraType.cycle` and may be set to another.
    pub(in crate::game) fn native_camera_type_cycle(
        &mut self,
        previous: mod_api::CameraType,
        next: &mut mod_api::CameraType,
    ) {
        self.fire_camera_type_cycle(previous, next);
    }
    /// The window's camera type (`Options.getCameraType`).
    pub(in crate::game) fn native_camera_type(&self) -> CameraType {
        self.camera_type
    }
    /// Sets the window's camera type (`Options.setCameraType`) as F5 does, without
    /// cycling: a freelook mod switches to third person and back.
    pub(in crate::game) fn native_set_camera_type(&mut self, camera_type: CameraType) {
        self.set_camera_type(camera_type);
    }
}

#[cfg(test)]
mod camera_type_tests {
    use mod_api::{
        CameraType, CameraTypeCycleEvent, ClientHost, Hooks, ModEntry, testing::with_mods,
    };
    use std::sync::{Arc, Mutex};

    /// Skips the front view: F5 goes first person, behind, first person.
    struct NoFront(Arc<Mutex<Vec<(CameraType, CameraType)>>>);
    #[mod_api::__subscriptions]
    impl Hooks for NoFront {
        fn camera_type_cycle(&self, _: &mut dyn ClientHost, event: &mut CameraTypeCycleEvent<'_>) {
            self.0.lock().unwrap().push((event.previous, *event.next));
            if *event.next == CameraType::ThirdPersonFront {
                *event.next = CameraType::FirstPerson;
            }
        }
    }

    /// `key.togglePerspective`: the mod hears each cycle with vanilla's next type and its
    /// answer is the new type; setting the type does not cycle, and without a listener F5
    /// cycles as vanilla does.
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn a_client_mod_picks_the_camera_type_f5_cycles_to() {
        crate::game::native_entity_refs::tests::with_game(|game| {
            game.options.execution_role = super::ExecutionRole::Client;
            let mut seen = Vec::new();
            for _ in 0..3 {
                game.cycle_camera();
                seen.push(game.native_camera_type());
            }
            use CameraType::*;
            assert_eq!(seen, [ThirdPersonBack, ThirdPersonFront, FirstPerson]);
        });
        let heard = Arc::new(Mutex::new(Vec::new()));
        let mods: mod_api::Mods = vec![ModEntry::new(
            "no-front",
            Box::new(NoFront(Arc::clone(&heard))),
        )]
        .into();
        with_mods(mods, || {
            crate::game::native_entity_refs::tests::with_game(|game| {
                use CameraType::*;
                game.options.execution_role = super::ExecutionRole::Client;
                game.cycle_camera();
                assert_eq!(game.native_camera_type(), ThirdPersonBack);
                game.cycle_camera();
                assert_eq!(
                    game.native_camera_type(),
                    FirstPerson,
                    "the front view is skipped"
                );
                game.native_set_camera_type(ThirdPersonFront);
                assert_eq!(game.native_camera_type(), ThirdPersonFront);
            });
        });
        use CameraType::*;
        assert_eq!(
            *heard.lock().unwrap(),
            [
                (FirstPerson, ThirdPersonBack),
                (ThirdPersonBack, ThirdPersonFront)
            ],
            "setting the type is not a cycle"
        );
    }
}
