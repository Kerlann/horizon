#!/usr/bin/env python3
"""One-shot, behavior-preserving extraction before temporary actor loan wiring.

Run from the repository root. This deliberately leaves the original remove/insert
protocol unchanged; authoritative owning loans are a separate semantic change.
"""
from pathlib import Path
import textwrap


def extract(path, family, variable, actor_type, parameters, arguments, exits):
    source = Path(path).read_text()
    start = f"            let mut {variable} = self.{family}.remove(i);\n"
    end = f"            self.{family}.insert(i, {variable});\n"
    assert source.count(start) == source.count(end) == 1, path
    before, body = source.split(start)
    body, after = body.split(end)
    assert body.count("continue;") == exits, path
    # These are actor-retirement exits, not inner-loop continue statements.
    body = textwrap.indent(textwrap.dedent(body), "        ")
    body = body.replace("continue;", "return false;")
    singular = "rocket" if family == "rockets" else "trident"
    call = (
        start
        + f"            if !self.tick_{singular}_body(&mut {variable}, {arguments}) {{\n"
        + "                continue;\n"
        + "            }\n"
        + end
    )
    boundary = "        }\n    }\n"
    assert after.startswith("            i += 1;\n" + boundary), path
    after = after.replace(
        boundary,
        boundary
        + f"    fn tick_{singular}_body(&mut self, mut {variable}: &mut {actor_type}, {parameters}) -> bool {{\n"
        + body
        + "        true\n    }\n",
        1,
    )
    return Path(path), before + call + after


changes = [
    extract(
        "crates/horizon-client/src/game/entities/fireworks.rs",
        "rockets", "r", "Rocket", "targets: &[(Who, Aabb)]", "&targets", 1,
    ),
    extract(
        "crates/horizon-client/src/game/items/trident.rs",
        "tridents", "t", "ThrownTrident",
        "catchers: &[Catcher], targets: &[(Who, Aabb)]", "&catchers, &targets", 2,
    ),
]
for path, source in changes:
    path.write_text(source)
