#!/usr/bin/env python3
"""Mechanical generalization of the owning projectile loan for other actors.

Run once from the repository root, before hand-written vehicle/cloud changes.
This changes names only, not ownership, callback, physics or ordering behavior.
"""
from pathlib import Path

root = Path(__file__).resolve().parents[1]
area = root / "crates/horizon-client/src/game"
old = area / "entities/projectile_actor_loan.rs"
new = area / "entities/entity_actor_loan.rs"
assert old.is_file() and not new.exists(), "run against the projectile-loan checkpoint"
paths = [old, area / "entities/fireworks.rs", area / "items/trident.rs"]
for path in paths:
    source = path.read_text()
    assert "ProjectileLoan" in source
    source = source.replace("ProjectileLoan", "EntityActorLoan")
    source = source.replace("ProjectileActor", "EntityActor")
    source = source.replace("projectile_actor_loan", "entity_actor_loan")
    if path == old:
        source = source.replace("An owning temporary projectile loan", "An owning temporary entity actor loan")
        new.write_text(source)
        old.unlink()
    else:
        path.write_text(source)
