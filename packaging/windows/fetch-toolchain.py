#!/usr/bin/env python3
"""Fetch a checksum-pinned Linux llvm-mingw toolchain into an explicit external directory."""
import argparse
import hashlib
from pathlib import Path
import tarfile
import urllib.request

VERSION = "20261104"
NAME = f"llvm-mingw-{VERSION}-ucrt-ubuntu-22.04-x86_64"
SHA256 = "919380403de87dca3c581c2053230cd298af5e891286628ba0613b7fed370926"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    args = parser.parse_args()
    root = args.directory.resolve()
    root.mkdir(parents=True, exist_ok=True)
    archive = root / f"{NAME}.tar.xz"
    destination = root / NAME
    if destination.exists():
        raise SystemExit("Refusing to overwrite an existing extracted toolchain")
    if not archive.exists():
        url = f"https://github.com/mstorsjo/llvm-mingw/releases/download/{VERSION}/{archive.name}"
        urllib.request.urlretrieve(url, archive)
    with archive.open("rb") as file:
        actual = hashlib.file_digest(file, "sha256").hexdigest()
    if actual != SHA256:
        raise SystemExit("Toolchain SHA256 mismatch; review upstream before changing the pin")
    with tarfile.open(archive) as file:
        file.extractall(root, filter="data")
    print(destination)


if __name__ == "__main__":
    main()
