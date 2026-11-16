#!/usr/bin/env python3
"""Collect Rust dependency license files on any build host; not a legal audit."""
import json
from pathlib import Path
import shutil
import subprocess
import sys

destination = Path(sys.argv[1])
metadata = json.loads(subprocess.check_output(
    ["cargo", "metadata", "--locked", "--format-version", "1"], text=True))
packages = []
for package in metadata["packages"]:
    if package["source"] is None:
        continue
    name = f'{package["name"]}-{package["version"]}'
    packages.append(f'{name}: {package["license"] or "see package notices"}')
    source = Path(package["manifest_path"]).parent
    output = destination / "rust" / name
    output.mkdir(parents=True, exist_ok=True)
    shutil.copy2(source / "Cargo.toml", output / "Cargo.toml")
    for path in source.rglob("*"):
        if path.is_file() and path.name.upper().startswith(
                ("LICENSE", "LICENCE", "COPYING", "NOTICE", "AUTHORS", "COPYRIGHT")):
            target = output / path.relative_to(source)
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(path, target)
destination.mkdir(parents=True, exist_ok=True)
(destination / "RUST-DEPENDENCIES.txt").write_text("\n".join(sorted(packages)) + "\n")
