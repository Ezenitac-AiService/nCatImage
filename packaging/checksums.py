#!/usr/bin/env python3
"""Write SHA256SUMS for release packages, refusing empty/missing-platform sets."""
import argparse
import hashlib
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("directory", type=Path)
parser.add_argument("--require-all", action="store_true")
args = parser.parse_args()
paths = sorted(path for path in args.directory.iterdir()
               if path.is_file() and path.name.startswith("nCatImage-")
               and path.name.endswith((".AppImage", "-portable.zip", ".dmg")))
if not paths:
    raise SystemExit("No release packages found")
if args.require_all:
    for suffix in [".AppImage", "-portable.zip", ".dmg"]:
        if sum(path.name.endswith(suffix) for path in paths) != 1:
            raise SystemExit(f"Expected exactly one {suffix} package")
lines = []
for path in paths:
    with path.open("rb") as file:
        digest = hashlib.file_digest(file, "sha256").hexdigest()
    lines.append(f"{digest}  {path.name}\n")
(args.directory / "SHA256SUMS").write_text("".join(lines), encoding="ascii")
