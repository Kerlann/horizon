#!/usr/bin/env python3
"""Extract the existing pure breath-cloud step without changing its behavior.

The separate commit lets ownership changes be reviewed apart from the move.
Run from the repository root; an unexpected source shape fails before writing.
"""
from pathlib import Path

path = Path("crates/horizon-client/src/game/mobs/dragon.rs")
source = path.read_text()
start = source.index("    pub(super) fn tick_breath(&mut self) {")
end = source.index("    /// `EnderDragon.knockBack`", start)
method = source[start:end]
body_start = method.index("            c.age += 1;")
body_end = method.index("            true\n        });", body_start) + len("            true\n")
body = method[body_start:body_end]
body = "".join(line[4:] if line.startswith("    ") else line for line in body.splitlines(keepends=True))
call = "            self.tick_breath_body(c, &players, &mobs, &mut harmed, &mut harmed_mobs)\n"
method = method[:body_start] + call + method[body_end:]
helper = (
    "    /// The existing pure breath Cloud step, before deferred damage.\n"
    "    fn tick_breath_body(\n"
    "        &mut self,\n"
    "        c: &mut Breath,\n"
    "        players: &[FightPlayer],\n"
    "        mobs: &[(u32, Aabb, Vec3)],\n"
    "        harmed: &mut Vec<(FightWho, f32)>,\n"
    "        harmed_mobs: &mut Vec<(u32, i32, [f64; 3])>,\n"
    "    ) -> bool {\n"
    + body
    + "    }\n"
)
assert "fn tick_breath_body(" not in source
path.write_text(source[:start] + method + helper + source[end:])
