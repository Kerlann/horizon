#!/usr/bin/env python3
"""Share the owning-world removed-vehicle free-position search."""
from pathlib import Path
path = Path(__file__).resolve().parents[2] / 'crates/horizon-client/src/game/entities/cushions.rs'
s = path.read_text()
if 'fn removed_vehicle_dismount_at(' not in s:
    start = s.index('    pub(super) fn dismount_removed_vehicle(')
    brace = s.index('{', start)
    depth, end = 1, brace + 1
    while depth:
        if s[end] == '{': depth += 1
        elif s[end] == '}': depth -= 1
        end += 1
    old = s[start:end]
    calc_start = old.index('        let mut at =')
    calc_end = old.index('        if let Some(p) = self.player.as_mut()')
    calc = old[calc_start:calc_end].replace('p.position', 'position').replace('self.world.block', 'world.block')
    finish = old[calc_end:-1]
    wrapper = '''    pub(super) fn dismount_removed_vehicle(&mut self, vehicle_y: f64) {
        self.riding = None;
        let Some(p) = self.player.as_ref() else { return; };
        let at = self.removed_vehicle_dismount_at(&self.world, &self.border, p.position,
            f64::from(p.dimensions.width), f64::from(p.dimensions.height), vehicle_y);
''' + finish + '}'
    helper = '''
    /// `LivingEntity.dismountVehicle` for a removed vehicle, in its owning world.
    pub(super) fn removed_vehicle_dismount_at(&self,
        world: &horizon_client::world::World,
        world_border: &horizon_client::worldborder::WorldBorder,
        position: Vec3, w: f64, h: f64, vehicle_y: f64) -> Vec3 {
        let [min_x, min_z, max_x, max_z] = world_border.bounds();
        let border = horizon_world::level::portal_travel::BorderBounds { min_x, min_z, max_x, max_z };
        let b = self.res.behaviours();
''' + calc + '        at\n    }'
    path.write_text(s[:start] + wrapper + helper + s[end:])
