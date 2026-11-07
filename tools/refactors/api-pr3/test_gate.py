import json
from pathlib import Path
import subprocess
import tempfile
import unittest

import compiler_rewrite
import gate
import text_replay


class GateTests(unittest.TestCase):
    def test_manifest_checks_signatures_and_all_metadata(self):
        old = 'Host\tf\tfn f(x: ParticleType)\tdoc\tfalse\n'
        new = old.replace('ParticleType', 'ParticleKey')
        self.assertTrue(gate.host_gate(old, new, {}))
        plan = {'host_substitutions': {'ParticleType': 'ParticleKey'}}
        self.assertEqual(gate.host_gate(old, new, plan), [])
        for changed in [new.replace('doc', 'other'), new.replace('fn f', 'fn g'), new.replace('false', 'true')]:
            self.assertTrue(gate.host_gate(old, changed, plan))

    def test_wit_checks_world_hooks_and_variant_order(self):
        old = 'package horizon:mod-api;\ninterface types {\n  enum side {\n    client,\n    server,\n  }\n}\nworld mod {\n  export run: func();\n}\n'
        rows = gate.wit_rows(old)
        self.assertEqual(gate.exact_rows(rows, rows, {}, {})[0], [])
        for changed in [old.replace('export run', 'export other'), old.replace('client,\n    server', 'server,\n    client')]:
            self.assertTrue(gate.exact_rows(rows, gate.wit_rows(changed), {}, {})[0])

    def test_exact_exceptions_are_not_blanket_signature_waivers(self):
        self.assertTrue(gate.exact_rows({'x': 'old'}, {'x': 'new'}, {'x': {'old': 'old', 'new': 'wrong'}}, {})[0])
        self.assertTrue(gate.exact_rows({'x': 'same'}, {'x': 'same'}, {'x': {'old': 'same', 'new': 'same'}}, {})[0])

    def test_text_replay_is_idempotent(self):
        path = 'crates/horizon-mod-api/src/exports/text.rs'
        once, edits = text_replay.transform(path, 'pub mod text;\n')
        self.assertTrue(edits)
        self.assertEqual(once, 'pub mod text;\npub use text::Text;\n')
        self.assertEqual(text_replay.transform(path, once), (once, []))

    def test_compiler_spans_rewrite_only_explicit_primary_tokens(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            path = root / 'crates/test.rs'
            path.parent.mkdir()
            path.write_text('Old Old')
            message = {'reason': 'compiler-message', 'message': {'level': 'error', 'message': 'missing', 'spans': [
                {'is_primary': True, 'file_name': 'crates/test.rs', 'byte_start': 0, 'byte_end': 3},
                {'is_primary': False, 'file_name': 'crates/test.rs', 'byte_start': 4, 'byte_end': 7}]}}
            result = compiler_rewrite.rewrite(root, json.dumps(message), {'Old': 'New'}, True)
            self.assertEqual(path.read_text(), 'New Old')
            self.assertEqual(len(result['records']), 1)
            # Stale diagnostics must not change an unrelated token.
            self.assertEqual(compiler_rewrite.rewrite(root, json.dumps(message), {'Old': 'New'}, True)['records'], [])

    def test_source_gate_rejects_balanced_but_reordered_lines(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            subprocess.run(['git', 'init', '-q', tmp], check=True)
            path = root / 'docs/modding/a.md'
            path.parent.mkdir(parents=True)
            path.write_text('a\nb\n')
            subprocess.run(['git', '-C', tmp, 'add', '.'], check=True)
            subprocess.run(['git', '-C', tmp, '-c', 'user.name=Test', '-c', 'user.email=test@example.invalid', 'commit', '-qm', 'base'], check=True)
            path.write_text('b\na\n')
            self.assertTrue(gate.source_gate(root, 'HEAD', {})[0])
            plan = {'source': {'docs/modding/a.md': {'rewrites': [{'old': 'a\nb\n', 'new': 'b\na\n', 'count': 1}]}}}
            self.assertEqual(gate.source_gate(root, 'HEAD', plan)[0], [])
            path.write_text('b\nc\n')
            self.assertTrue(gate.source_gate(root, 'HEAD', plan)[0])

    def test_declared_move_compares_against_original_source(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            subprocess.run(['git', 'init', '-q', tmp], check=True)
            old = root / 'docs/modding/old.md'
            old.parent.mkdir(parents=True)
            old.write_text('the original body\n')
            subprocess.run(['git', '-C', tmp, 'add', '.'], check=True)
            subprocess.run(['git', '-C', tmp, '-c', 'user.name=Test', '-c', 'user.email=test@example.invalid', 'commit', '-qm', 'base'], check=True)
            new = root / 'docs/modding/new.md'
            old.unlink()
            new.write_text('the original body\n')
            plan = {'source': {
                'docs/modding/old.md': {'rewrites': [{'old': 'the original body\n', 'new': '', 'count': 1}]},
                'docs/modding/new.md': {'move_from': 'docs/modding/old.md'}}}
            self.assertEqual(gate.source_gate(root, 'HEAD', plan)[0], [])
            new.write_text('a different body\n')
            self.assertTrue(gate.source_gate(root, 'HEAD', plan)[0])


if __name__ == '__main__':
    unittest.main()
