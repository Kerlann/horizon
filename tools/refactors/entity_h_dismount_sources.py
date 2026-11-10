#!/usr/bin/env python3
"""Make the existing dismount kernels take actual bodies and owning worlds.

Committed before application. No algorithm changes; handwritten H admission and
source-carrier integration follow in separate corrections.
"""
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
BASE = ROOT / 'crates/horizon-client/src/game'


def extract(text, name):
    start = text.rfind('\n', 0, text.index('fn '+name+'(')) + 1
    brace = text.index('{', text.index('fn '+name+'('))
    depth, end = 1, brace + 1
    while depth:
        if text[end] == '{': depth += 1
        elif text[end] == '}': depth -= 1
        end += 1
    return start, brace, end


def route(path, name, signature, wrapper, transform):
    path = BASE / path
    text = path.read_text()
    if 'fn '+name+'_at(' in text: return
    start, brace, end = extract(text, name)
    old = text[start:end]
    body = transform(text[brace + 1:end - 1])
    replacement = text[start:brace + 1] + '\n' + wrapper + '\n    }\n\n' + signature + ' {' + body + '}'
    path.write_text(text[:start] + replacement + text[end:])


WORLD = '&horizon_client::world::World'
BORDER = '&horizon_client::worldborder::WorldBorder'
MOB = '&horizon_client::mobs::Mob'


def apply():
    route('entities/boats/mod.rs', 'boat_dismount_target',
          f'    pub(super) fn boat_dismount_target_at(&self, world: {WORLD}, b: &Boat, width: f64, yaw: f32) -> Vec3',
          '        self.boat_dismount_target_at(&self.world, &self.boats[i], width, yaw)',
          lambda body: body.replace('        let b = &self.boats[i];\n', '').replace('self.world', 'world'))
    route('entities/minecarts.rs', 'cart_player_dismount_target',
          f'    pub(super) fn cart_player_dismount_target_at(&self, world: {WORLD}, c: &Minecart) -> Vec3',
          '        self.cart_player_dismount_target_at(&self.world, &self.carts[i])',
          lambda body: body.replace('        let c = &self.carts[i];\n', '').replace('self.world', 'world'))
    route('blocks/respawn_anchor.rs', 'can_dismount_to',
          f'    pub(super) fn can_dismount_to_at(&self, world: {WORLD}, border: {BORDER}, bounds: Aabb) -> bool',
          '        self.can_dismount_to_at(&self.world, &self.border, bounds)',
          lambda body: body.replace('world: &self.world', 'world').replace('self.border', 'border'))
    for name in ['cart_dismount_mob', 'horse_dismount_mobs']:
        cart = name == 'cart_dismount_mob'
        source = 'cart: &super::minecarts::Minecart' if cart else f'mount: {MOB}'
        wrapper = ('        let cart = self.carts.iter().find(|cart| cart.id == vehicle)?;\n' if cart else '')
        wrapper += f'        self.{name}_at(&self.world, &self.border, '+('cart' if cart else 'mount')+', rider)'
        def transform(body):
            body = body.replace('        let cart = self.carts.iter().find(|c| c.id == vehicle)?;\n', '')
            return body.replace('self.block_collision_shape(', 'self.block_collision_shape_in(world, ').replace('self.can_dismount_to(', 'self.can_dismount_to_at(world, border, ')
        route('entities/vehicle_teleport.rs', name,
              f'    pub(super) fn {name}_at(&self, world: {WORLD}, border: {BORDER}, {source}, rider: {MOB}) -> Option<Vec3>', wrapper, transform)
    route('mobs/mounts.rs', 'mount_player_dismount',
          f'    fn mount_player_dismount_at(&self, world: {WORLD}, border: {BORDER}, mount: &Mount, yaw: f32, p: &PlayerMotion) -> (Vec3, Option<Pose>)',
          '        self.mount_player_dismount_at(&self.world, &self.border, mount, yaw, p)',
          lambda body: body.replace('GameDismountWorld(self)', 'GameDismountWorld(self, world, border)'))
    path = BASE / 'mobs/mounts.rs'
    text = path.read_text()
    text = text.replace("struct GameDismountWorld<'a, 'g>(&'a Game<'g>);", f"struct GameDismountWorld<'a, 'g>(&'a Game<'g>, &'a horizon_client::world::World, &'a horizon_client::worldborder::WorldBorder);")
    start = text.index('impl DismountWorld for GameDismountWorld')
    end = text.index('/// `getDismountLocationForPassenger`', start)
    block = text[start:end].replace('g.world.block(', 'self.1.block(')
    block = block.replace('self.0.block_collision_shape(', 'self.0.block_collision_shape_in(self.1, ')
    block = block.replace('self.0.can_dismount_to(bounds)', 'self.0.can_dismount_to_at(self.1, self.2, bounds)')
    text = text[:start] + block + text[end:]
    path.write_text(text)


if __name__ == '__main__': apply()
