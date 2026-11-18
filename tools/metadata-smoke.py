#!/usr/bin/env python3
"""Generated embedded-metadata fixtures and actual GTK overlay/key-signal regression."""
import binascii
import hashlib
from pathlib import Path
import struct
import subprocess
import zipfile
import zlib

from smoke_support import arguments, image_command, work_directory


def chunk(kind, value):
    return (struct.pack(">I", len(value)) + kind + value
            + struct.pack(">I", binascii.crc32(kind + value) & 0xFFFFFFFF))


def main():
    args = arguments(__doc__)
    work = work_directory("ncat-metadata-")
    plain = (b"\x89PNG\r\n\x1a\n"
             + chunk(b"IHDR", struct.pack(">IIBBBBB", 2, 2, 8, 2, 0, 0, 0))
             + chunk(b"IDAT", zlib.compress(b"\x00\xff\x00\x00\x00\xff\x00" * 2))
             + chunk(b"IEND", b""))
    text = chunk(b"tEXt", b"Title\x00<markup>literal fixture</markup>")
    text += chunk(b"iTXt", "parameters\0\0\0\0\0한글 prompt\nseed=42".encode())
    metadata = plain[:33] + text + plain[33:]
    paths = []
    for name, data, expected in [("plain.png", plain, False), ("metadata.png", metadata, True)]:
        path = work / name
        path.write_bytes(data)
        paths.append((path, expected))
    archive = work / "metadata.cbz"
    with zipfile.ZipFile(archive, "x") as output:
        output.writestr("page.png", metadata)
    paths.append((archive, True))
    for index, (path, expected) in enumerate(paths):
        run = work / f"run-{index}"
        run.mkdir()
        before = hashlib.sha256(path.read_bytes()).digest()
        result = subprocess.run(image_command(args.image, "--smoke-metadata", path),
                                cwd=run, capture_output=True, text=True, timeout=25)
        (run / "gui.log").write_text(result.stdout + result.stderr)
        assert result.returncode == 0 and "metadata_overlay_smoke=ok" in result.stdout, f"inspect {run}"
        assert f"has_metadata={str(expected).lower()}" in result.stdout, f"inspect {run}"
        assert hashlib.sha256(path.read_bytes()).digest() == before
        print(f"PASS {path.name}: default-off, Tab/Shift+Tab, no focus traversal, metadata={expected}")
    print(f"Synthetic fixtures/results retained in {work}")


if __name__ == "__main__":
    main()
