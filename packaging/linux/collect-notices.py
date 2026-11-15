#!/usr/bin/env python3
"""Collect dependency notices; do not assign a license to the project itself."""
import json
import pathlib
import shutil
import subprocess
import sys

destination = pathlib.Path(sys.argv[1])
shutil.copytree("/usr/share/common-licenses", destination / "common-licenses")
metadata = json.loads(subprocess.check_output([
    "cargo", "metadata", "--locked", "--offline", "--format-version", "1",
]))
packages = []
for package in metadata["packages"]:
    if package["source"] is None:
        continue
    name = f'{package["name"]}-{package["version"]}'
    packages.append(f'{name}: {package["license"] or "see package notices"}')
    source = pathlib.Path(package["manifest_path"]).parent
    output = destination / "rust" / name
    output.mkdir(parents=True, exist_ok=True)
    shutil.copy2(source / "Cargo.toml", output / "Cargo.toml")
    for path in source.rglob("*"):
        if path.is_file() and path.name.upper().startswith(("LICENSE", "LICENCE", "COPYING", "NOTICE", "AUTHORS", "COPYRIGHT")):
            target = output / path.relative_to(source)
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(path, target)
(destination / "RUST-DEPENDENCIES.txt").write_text("\n".join(sorted(packages)) + "\n")
# Retain installed distro package notices, including GTK/GLib and codec libs.
for copyright_file in pathlib.Path("/usr/share/doc").glob("*/copyright"):
    target = destination / "system" / copyright_file.parent.name
    target.mkdir(parents=True, exist_ok=True)
    shutil.copy2(copyright_file, target / "copyright")
