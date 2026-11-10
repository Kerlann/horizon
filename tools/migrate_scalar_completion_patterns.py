"""Make single-operation completion assertions explicit before adding NoGravity."""
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1]


def run():
    directory = ROOT / "crates/horizon-client/src/game/modding"
    pattern = re.compile(r"(?m)^(\s*)let ((?:mod_api::)?EntityWriteCompletion::EntitySetSilent\(done\)) = event.completion;$")
    changed = 0
    for path in sorted(directory.glob("*.rs")):
        source = path.read_text()
        result, count = pattern.subn(lambda m: f'{m[1]}let {m[2]} = event.completion else {{ panic!("unexpected operation completion in SetSilent test") }};', source)
        if count:
            path.write_text(result)
            changed += count
    if changed != 6:
        raise ValueError(f"expected six single-operation callback assertions, found {changed}")
    path = ROOT / "crates/horizon-mod-api/src/entity_write_receipts.rs"
    source = path.read_text()
    old = "&[0, 0, 0, 0][..], // EntitySetSilent, independently pinned"
    if source.count(old) != 1:
        raise ValueError("expected one independent SetSilent wire tag")
    source = source.replace(old, "&[1, 0, 0, 0][..], // EntitySetSilent after adding NoGravity, independently pinned")
    source = source.replace("fn discovered_silent_completion_keeps_its_wire_tag_and_actual_false()", "fn discovered_silent_completion_pins_its_wire_tag_and_actual_false()")
    path.write_text(source)


if __name__ == "__main__":
    run()
