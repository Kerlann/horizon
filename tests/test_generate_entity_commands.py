"""Behavior checks for per-operation command discovery and stale output rejection."""
import json
from pathlib import Path
import tempfile
import unittest

from tools.generate_entity_commands import declarations, outputs, run


class CommandGenerationTests(unittest.TestCase):
    def test_new_operation_is_discovered_without_a_shared_list(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            directory = root / 'crates/horizon-mod-api/entity_commands'
            directory.mkdir(parents=True)
            first = dict(operation='SetSilent', completion='EntitySetSilent',
                         payload='bool', result='bool', error='EntityWriteError')
            (directory / 'silent.json').write_text(json.dumps(first))
            run(root)
            run(root, check=True)
            second = dict(operation='SetGlowing', completion='EntitySetGlowing',
                          payload='bool', result='bool', error='EntityWriteError')
            (directory / 'glowing.json').write_text(json.dumps(second))
            with self.assertRaisesRegex(ValueError, 'stale command declarations'):
                run(root, check=True)
            run(root)
            generated = list(outputs(root).values())
            self.assertIn('SetGlowing(bool)', generated[0])
            self.assertIn('EntitySetGlowing(CommandCompleted<bool, EntityWriteError>)', generated[1])
            run(root, check=True)
            (directory / 'glowing.json').unlink()
            with self.assertRaisesRegex(ValueError, 'stale command declarations'):
                run(root, check=True)

    def test_duplicate_names_and_invalid_shapes_refuse_at_declaration_boundary(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            valid = dict(operation='SetSilent', completion='EntitySetSilent',
                         payload='bool', result='bool', error='EntityWriteError')
            (directory / 'one.json').write_text(json.dumps(valid))
            (directory / 'two.json').write_text(json.dumps(valid))
            with self.assertRaisesRegex(ValueError, 'duplicate operation'):
                declarations(directory)
            valid['operation'] = 'Not an identifier'
            (directory / 'two.json').write_text(json.dumps(valid))
            with self.assertRaisesRegex(ValueError, 'invalid operation'):
                declarations(directory)
            (directory / 'two.json').write_text('{}')
            with self.assertRaisesRegex(ValueError, 'expected operation'):
                declarations(directory)

    def test_checked_in_command_outputs_are_current(self):
        run(Path(__file__).resolve().parents[1], check=True)

    def test_empty_catalog_refuses_instead_of_erasing_operations(self):
        with tempfile.TemporaryDirectory() as temporary:
            with self.assertRaisesRegex(ValueError, 'no command declarations'):
                declarations(Path(temporary))
