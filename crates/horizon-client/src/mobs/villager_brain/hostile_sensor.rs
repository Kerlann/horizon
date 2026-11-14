// Included in villager_brain.rs to retain its private sensor scope.
/// `VillagerHostilesSensor.ACCEPTABLE_DISTANCE_FROM_HOSTILES`.
/// `VillagerHostilesSensor`, its distances from `neoforge:acceptable_villager_distances`.
fn sense_hostiles(m: &mut Mob, w: &MobWorld) {
    let maps = crate::neoforge_mob_maps::current();
    let hostile = find_closest(m, w, |m, who| {
        let Who::Mob(id) = who else { return false };
        let Some(s) = w.seen(id) else { return false };
        let close = |d: f32| m.distance_sqr(s.position) <= f64::from(d * d);
        maps.villager_distance(s.species).is_some_and(close)
    });
    mem(m).nearest_hostile.set_opt(hostile);
}
