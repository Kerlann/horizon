// Included in villager_brain.rs to retain its private sensor scope.
/// `VillagerHostilesSensor.ACCEPTABLE_DISTANCE_FROM_HOSTILES`.
/// `VillagerHostilesSensor`, its distances from `neoforge:acceptable_villager_distances`.
fn sense_hostiles(m: &mut Mob, w: &MobWorld) {
    let hostile = find_closest(m, w, |m, who| {
        let (entity_type, position) = match who {
            Who::Mob(id) => {
                let Some(seen) = w.seen(id) else { return false };
                (seen.kind.id.as_str(), seen.position)
            }
            Who::Player => {
                let Some(player) = w.selected_player() else {
                    return false;
                };
                ("minecraft:player", player.position)
            }
        };
        w.mob_maps
            .villager_distance(entity_type)
            .is_some_and(|distance| m.distance_sqr(position) <= f64::from(distance * distance))
    });
    mem(m).nearest_hostile.set_opt(hostile);
}

#[cfg(test)]
mod mob_map_tests {
    use super::*;
    use crate::neoforge_mob_maps::NeoForgeMobMaps;
    use std::{collections::BTreeMap, sync::Arc};

    #[test]
    #[ignore = "daily"]
    fn hostile_sensor_matches_registered_ids_and_world_snapshot() {
        let root = std::env::var_os("HORIZON_TEST_RESOURCES").expect("explicit fixture resources");
        let mut villager_kind = crate::mobs::sensor_scan_tests::kind();
        Arc::make_mut(&mut villager_kind).id = "minecraft:villager".into();
        Arc::make_mut(&mut villager_kind).species = Species::Villager;
        let mut observer = Mob::new(1, villager_kind, Vec3::new(0.0, 1.0, 0.0), 0.0);
        let mut zombie_kind = crate::mobs::sensor_scan_tests::kind();
        Arc::make_mut(&mut zombie_kind).id = "minecraft:zombie".into();
        Arc::make_mut(&mut zombie_kind).species = Species::Zombie;
        let mut custom_kind = zombie_kind.clone();
        Arc::make_mut(&mut custom_kind).id = "test:wraith".into();
        let items = crate::items::Items::empty();
        let seen = [
            Mob::new(2, custom_kind, Vec3::new(12.0, 1.0, 0.0), 0.0).seen(None, &items),
            Mob::new(3, zombie_kind, Vec3::new(4.0, 1.0, 0.0), 0.0).seen(None, &items),
        ];
        let mut first = NeoForgeMobMaps::default();
        first.set_distances(BTreeMap::from([("test:wraith".into(), 20.0)]));
        let mut second = NeoForgeMobMaps::default();
        second.set_distances(BTreeMap::from([("minecraft:zombie".into(), 5.0)]));
        let mut removed = NeoForgeMobMaps::default();
        removed.set_distances(BTreeMap::new());
        assert!(crate::mobs::world::with_resource_test_world(
            std::path::Path::new(&root),
            &seen,
            true,
            true,
            |_, _| {},
            |world, _| {
                let mut view = world.clone();
                for (snapshot, expected) in [
                    (&first, Some(Who::Mob(2))),
                    (&second, Some(Who::Mob(3))),
                    (&removed, None),
                    (&first, Some(Who::Mob(2))),
                    (NeoForgeMobMaps::vanilla().as_ref(), Some(Who::Mob(3))),
                ] {
                    view.mob_maps = snapshot;
                    // Isolate the map's selection from visibility: both living
                    // candidates were visible at the preceding sensor scan.
                    mem(&mut observer).visible_mobs.set(Visible {
                        list: vec![Who::Mob(3), Who::Mob(2)],
                        cache: vec![(Who::Mob(3), true), (Who::Mob(2), true)],
                    });
                    sense_hostiles(&mut observer, &view);
                    assert_eq!(mem(&mut observer).nearest_hostile.get().copied(), expected);
                }
            },
        ));
    }
}
