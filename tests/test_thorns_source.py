"""Exercise the real thorns method and attack scopes without prepared assets."""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

from test_world_input_capture import method

ROOT = Path(__file__).resolve().parents[1]


class ThornsSourceTests(unittest.TestCase):
    @unittest.skipUnless(shutil.which("rustc"), "requires Rust")
    def test_thorns_names_the_wearer_not_the_incoming_attacker(self):
        hooks = (ROOT / "crates/horizon-client/src/game/native_hooks.rs").read_text()
        effects = (ROOT / "crates/horizon-client/src/game/enchant_effects.rs").read_text()
        health = (ROOT / "crates/horizon-client/src/health.rs").read_text()
        start = health.index("thread_local! {")
        context = health[start:health.index("/// Whether [", start)]
        fixture = r"""
use std::cell::RefCell;
mod items { #[derive(Clone, Debug, PartialEq)] pub struct ItemStack(pub &'static str); }
use items::ItemStack;
mod horizon_client { pub mod health {
    use std::cell::Cell;
    CONTEXT
} }
mod mod_api { pub fn mods() -> &'static [()] { &[()] } }
#[derive(Clone, Copy, Debug, PartialEq)]
enum Who { Player, Mob(u32) }
#[derive(Clone, Copy, Debug, PartialEq)]
enum DamageKind { Generic, Thorns }
#[derive(Clone, Copy, Debug, PartialEq)]
struct Damage { kind: DamageKind, attacker: Option<Who>, from: Option<[f64; 3]> }
struct Position { x: f64, y: f64, z: f64 }
struct Player { entity_id: i32, position: Position }
struct Extra { hand: Option<ItemStack> }
impl Extra { fn main_hand_stack(&self) -> Option<&ItemStack> { self.hand.as_ref() } }
struct Mob { id: u32, position: Position, extra: Extra }
thread_local! {
    static HITS: RefCell<Vec<(f32, Damage, i32, [Option<ItemStack>; 2])>> = const { RefCell::new(Vec::new()) };
}
impl Mob {
    fn alive(&self) -> bool { true }
    fn damage(&mut self, amount: f32, source: Damage) {
        use horizon_client::health::{knockback_attacker, knockback_held};
        HITS.with_borrow_mut(|hits| hits.push((amount, source, knockback_attacker(), knockback_held())));
    }
    fn hurt(&mut self, amount: f32, _: Option<[f64; 3]>, _: &mut ()) {
        self.damage(amount, Damage { kind: DamageKind::Generic, attacker: None, from: None });
    }
}
struct Audio;
impl Audio { fn play(&self, _: &str, _: Option<[f64; 3]>, _: f32, _: f32) {} }
struct World { random: () }
struct Game {
    player: Option<Player>, mobs: Vec<Mob>, inventory: Vec<Option<ItemStack>>,
    selected: usize, offhand: Option<ItemStack>, audio: Audio, world: World,
}
impl Game {
    fn stack_in(&self, slot: usize) -> Option<&ItemStack> { self.inventory[slot].as_ref() }
    fn thorns_damage(&mut self) -> Vec<f32> { vec![3.0] }
    SCOPE
    SCOPE_FOR
    THORNS
}
#[test]
fn a_nested_thorns_hit_uses_the_enchanted_armors_owner() {
    use horizon_client::health::{knockback_from_holding, knockback_attacker, knockback_held};
    let _incoming = knockback_from_holding(42, [Some(ItemStack("mob_axe")), None]);
    let mut game = Game {
        player: Some(Player { entity_id: 7, position: Position { x: 1.0, y: 80.0, z: 2.0 } }),
        mobs: vec![Mob { id: 42, position: Position { x: 1.0, y: 80.0, z: 4.0 },
                         extra: Extra { hand: Some(ItemStack("mob_axe")) } }],
        inventory: vec![Some(ItemStack("player_sword"))], selected: 0,
        offhand: Some(ItemStack("player_shield")), audio: Audio, world: World { random: () },
    };
    game.thorns(42);
    assert_eq!(HITS.take(), [(3.0, Damage {
        kind: DamageKind::Thorns, attacker: Some(Who::Player), from: Some([1.0, 80.0, 2.0]),
    }, 7, [Some(ItemStack("player_sword")), Some(ItemStack("player_shield"))])]);
    assert_eq!(knockback_attacker(), 42);
    assert_eq!(knockback_held(), [Some(ItemStack("mob_axe")), None]);
}
"""
        for placeholder, source, name in [
            ("SCOPE_FOR", hooks, "knockback_scope_for"),
            ("SCOPE", hooks, "knockback_scope"),
            ("THORNS", effects, "thorns"),
        ]:
            fixture = fixture.replace(placeholder, method(source.replace("pub(super) fn", "fn"), name))
        fixture = fixture.replace("CONTEXT", context)
        target = ROOT / "target"
        target.mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(dir=target) as folder:
            source, executable = Path(folder) / "thorns.rs", Path(folder) / "thorns-test"
            source.write_text(fixture)
            subprocess.run(["rustc", "--edition=2024", "--test", str(source), "-o", str(executable)], check=True)
            subprocess.run([str(executable), "--test-threads=4"], check=True)
