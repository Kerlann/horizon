"""Run the production metadata importer and published-lookup allocation tests in verify."""
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


class BlockMetadataLoadTests(unittest.TestCase):
    def test_production_import_and_allocation_free_lookups(self):
        subprocess.run(
            [sys.executable, str(ROOT / "tools/refactors/api-pr3b/test_block_key_load_boundary.py")],
            cwd=ROOT, check=True,
        )
