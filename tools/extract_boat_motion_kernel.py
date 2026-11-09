#!/usr/bin/env python3
"""Extract the shared pure Boat motion phase, leaving the existing call order.

Mechanical first step before an owning server actor driver supplies canonical
leash callback handoffs. Run once on the existing Boat tick implementation.
"""
from pathlib import Path

root = Path(__file__).resolve().parents[1]
path = root / "crates/horizon-client/src/game/entities/boats/mod.rs"
source = path.read_text()
start = source.index("    pub(super) fn tick_boat(&mut self, b: &mut Boat, controlled: bool) {")
split = source.index("        let had_status =", start)
end = source.index("    /// `AbstractBoat.tick` advances", split)
assert "fn tick_boat_motion" not in source
# The existing final closing brace is kept with the moved motion body.
source = (
    source[:split]
    + "        self.tick_boat_motion(b, controlled);\n    }\n"
    + "    /// Shared motion after the existing previous-pose and leash phase.\n"
    + "    fn tick_boat_motion(&mut self, b: &mut Boat, controlled: bool) {\n"
    + source[split:end]
    + source[end:]
)
path.write_text(source)
