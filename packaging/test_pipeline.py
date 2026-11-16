"""Portable tests for release guards and checksum generation; no GUI dependencies."""
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import tomllib
import unittest

ROOT = Path(__file__).resolve().parent.parent


def run(script, *args):
    return subprocess.run([sys.executable, str(ROOT / "packaging" / script), *map(str, args)],
                          capture_output=True, text=True, cwd=ROOT)


class PipelineTests(unittest.TestCase):
    def test_supported_linux_and_matching_version(self):
        version = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
        self.assertEqual(run("preflight.py", "linux", "--tag", f"v{version}").returncode, 0)

    def test_wrong_tag_is_rejected(self):
        self.assertNotEqual(run("preflight.py", "linux", "--tag", "v0.0.0").returncode, 0)

    def test_unported_platforms_are_blocked(self):
        targets = json.loads((ROOT / "packaging/targets.json").read_text())
        for platform in ["windows", "macos"]:
            if not targets[platform]["ready"]:
                result = run("preflight.py", platform)
                self.assertEqual(result.returncode, 2)
                self.assertIn("BLOCKED", result.stderr)

    def test_no_partial_release_hashes(self):
        with tempfile.TemporaryDirectory() as directory:
            self.assertNotEqual(run("checksums.py", directory).returncode, 0)
            (Path(directory) / "nCatImage-test.AppImage").write_bytes(b"synthetic")
            self.assertNotEqual(run("checksums.py", directory, "--require-all").returncode, 0)
            self.assertFalse((Path(directory) / "SHA256SUMS").exists())

    def test_full_set_is_hashed(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            names = ["nCatImage-test.AppImage", "nCatImage-test-portable.zip", "nCatImage-test.dmg"]
            for name in names:
                (root / name).write_bytes(b"synthetic")
            self.assertEqual(run("checksums.py", directory, "--require-all").returncode, 0)
            text = (root / "SHA256SUMS").read_text()
            for name in names:
                self.assertIn(f"{hashlib.sha256(b'synthetic').hexdigest()}  {name}\n", text)


if __name__ == "__main__":
    unittest.main()
