#!/usr/bin/env python3
"""Generate our own PNG/CBZ test pages; no downloads or user images."""
import argparse
import binascii
from pathlib import Path
import struct
import zipfile
import zlib


def chunk(kind, data):
    return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", binascii.crc32(kind + data) & 0xFFFFFFFF)


def page(width, height, shade):
    rows = bytearray()
    for y in range(height):
        rows.append(0)
        for x in range(width):
            # Simple comic-like panels, gutters and deterministic gray shading.
            gutter = x < 24 or y < 24 or x > width - 24 or y > height - 24 or abs(y - height // 2) < 16
            value = 245 if gutter else (shade + (x // 80 + y // 80) % 2 * 24)
            rows.extend((value, value, value))
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(rows, 3)) + chunk(b"IEND", b"")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path, help="new output directory (must not exist)")
    args = parser.parse_args()
    args.directory.mkdir(parents=True, exist_ok=False)
    pages = [("page1.png", 800, 1200, 180), ("page2.png", 800, 1200, 150), ("page10.png", 1200, 800, 120)]
    with zipfile.ZipFile(args.directory / "sample.cbz", "x", compression=zipfile.ZIP_DEFLATED) as archive:
        for name, width, height, shade in pages:
            data = page(width, height, shade)
            (args.directory / name).write_bytes(data)
            entry = zipfile.ZipInfo(name, date_time=(2000, 1, 1, 0, 0, 0))
            entry.compress_type = zipfile.ZIP_DEFLATED
            entry.external_attr = 0o100600 << 16
            archive.writestr(entry, data)
    print(args.directory)


if __name__ == "__main__":
    main()
