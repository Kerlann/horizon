"""The mechanical Game port must not change a parked owner or repack paths."""
import importlib.util
from pathlib import Path
import unittest

SPEC = importlib.util.spec_from_file_location(
    "entity_main_packed_state",
    Path(__file__).resolve().parents[1] / "tools/refactors/entity_main_packed_state.py",
)
PORT = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PORT)


class PackedStatePortTests(unittest.TestCase):
    def test_game_and_parked_keep_distinct_knot_owners(self):
        source = "swap(&mut self.knots, &mut p.knots);"
        expected = "swap(&mut self.resident_entities.knots, &mut p.knots);"
        self.assertEqual(PORT.port_packed_paths(source), expected)

    def test_formatted_existing_paths_are_not_packed_again(self):
        source = (
            "self.resident_entities\n    .knots.retain(predicate);\n"
            "dragon.residency\n    .part_uuids.get(offset);\n"
        )
        self.assertEqual(PORT.port_packed_paths(source), source)

    def test_legacy_dragon_path_moves_once(self):
        expected = "dragon.residency.part_uuids.get(offset);"
        result = PORT.port_packed_paths("dragon.part_uuids.get(offset);")
        self.assertEqual(result, expected)
        self.assertEqual(PORT.port_packed_paths(result), expected)


if __name__ == "__main__":
    unittest.main()
