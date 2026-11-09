"""Rename the shared scalar overlay before extending it beyond SetSilent."""
from pathlib import Path
ROOT = Path(__file__).resolve().parents[1]


def run():
    directory = ROOT / "crates/horizon-client/src/game/modding"
    old = directory / "native_silent_overlay.rs"
    new = directory / "native_scalar_overlay.rs"
    if not old.exists() or new.exists():
        raise ValueError("expected original overlay module only")
    old.rename(new)
    replacements = (("native_silent_overlay", "native_scalar_overlay"),
                    ("SilentOverlay", "ScalarOverlay"),
                    ("silent_overlay:", "scalar_overlay:"),
                    (".silent_overlay", ".scalar_overlay"),
                    ("merge_pending_silent", "merge_pending_scalars"))
    for path in sorted(directory.rglob("*.rs")):
        source = path.read_text()
        result = source
        for old, new in replacements:
            result = result.replace(old, new)
        if result != source:
            path.write_text(result)


if __name__ == "__main__":
    run()
