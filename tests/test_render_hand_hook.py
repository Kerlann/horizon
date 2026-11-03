"""Exercise the hand renderer's actual dispatch with a fake mod."""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


class RenderHandHookTests(unittest.TestCase):
    @unittest.skipUnless(shutil.which("rustc"), "requires Rust")
    def test_each_hand_can_be_cancelled_or_posed(self):
        source = (ROOT / "crates/horizon-client/src/game.rs").read_text()
        start = source.index("            let mut render =", source.index("fn hand_geometry("))
        end = source.index("            // No mod moved the hand", start)
        dispatch = source[start:end]
        pitch_start = source.index("        let interpolated_pitch =", source.index("fn hand_geometry("))
        pitch_end = source.index("        // FirstPersonHandsAndItemsRenderer", pitch_start)
        pitch = source[pitch_start:pitch_end]
        self.assertLess(source.index("if !shown[i]", source.index("fn hand_geometry(")), start)
        self.assertLess(end, source.index("let Some(stack) = item", end))
        self.assertLess(end, source.index("if let Some(id) = maps::map_id", end))
        fixture = r"""
mod mod_api {
    pub enum HookId { RenderHand }
    pub fn listened(_: HookId) -> bool { true }
}
mod horizon_client {
    pub mod mod_api {
        pub struct NativeHandRender<'a> {
            pub player: u128, pub player_entity_id: i32, pub hand: usize,
            pub stack: Option<&'a i32>, pub partial: f32, pub interpolated_pitch: f32,
            pub swing: f32, pub equip: f32, pub packed_light: u32,
            pub translation: [f32; 3], pub rotation: [f32; 3], pub scale: f32,
        }
    }
}
struct Model { previous_head_pitch: f32 }
struct Game { cancel: Option<usize>, calls: Vec<(usize, Option<i32>, f32, f32, f32)>, pitch: f32, player_model: Option<Model> }
impl Game {
    fn own_uuid(&self) -> u128 { 7 }
    fn own_entity_id(&self) -> i32 { 9 }
    fn native_render_hand(&mut self, render: &mut horizon_client::mod_api::NativeHandRender<'_>, canceled: &mut bool) {
        assert_eq!((render.player, render.player_entity_id, render.interpolated_pitch, render.packed_light), (7, 9, 2.0, 3 << 4 | 10 << 20));
        self.calls.push((render.hand, render.stack.copied(), render.partial, render.swing, render.equip));
        render.translation[0] += 2.0;
        render.rotation[1] += 15.0;
        render.scale *= 2.0;
        *canceled = self.cancel == Some(render.hand);
    }
    fn draw(&mut self) -> Vec<(usize, [f32; 3], [f32; 3], f32)> {
        let mut rendered = Vec::new();
        for (hand, item) in [Some(42), None].into_iter().enumerate() {
            let (a, attack, inverse_height, light) = (0.5, 0.25, 0.75, 0xA3u8);
            PITCH
            DISPATCH
            rendered.push((hand, render.translation, render.rotation, render.scale));
        }
        rendered
    }
}
#[test]
fn cancellation_and_pose_share_one_dispatch_for_each_hand() {
    for (cancel, hands) in [(None, vec![0, 1]), (Some(0), vec![1]), (Some(1), vec![0])] {
        let mut game = Game { cancel, calls: Vec::new(), pitch: 3.0, player_model: Some(Model { previous_head_pitch: 1.0 }) };
        let expected = hands.into_iter().map(|h| (h, [2.0, 0.0, 0.0], [0.0, 15.0, 0.0], 2.0)).collect::<Vec<_>>();
        assert_eq!(game.draw(), expected);
        assert_eq!(game.calls, vec![(0, Some(42), 0.5, 0.25, 0.75), (1, None, 0.5, 0.25, 0.75)]);
    }
}
""".replace("DISPATCH", dispatch).replace("PITCH", pitch)
        target = ROOT / "target"
        target.mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(dir=target) as folder:
            src, exe = Path(folder) / "hand.rs", Path(folder) / "hand.exe"
            src.write_text(fixture)
            subprocess.run(["rustc", "--edition=2024", "--test", str(src), "-o", str(exe)], check=True)
            subprocess.run([str(exe)], check=True)
