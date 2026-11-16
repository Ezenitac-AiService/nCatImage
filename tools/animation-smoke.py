#!/usr/bin/env python3
"""Opt-in actual GTK animation display smoke using Pillow-generated private fixtures."""
import hashlib
from pathlib import Path
import shutil
import subprocess
import zipfile

from PIL import Image, features
from smoke_support import arguments, image_command, work_directory


def main():
    args = arguments(__doc__)
    if not features.check("webp"):
        raise SystemExit("Pillow needs animated WebP encoder support for this test")
    work = work_directory("ncat-animation-")
    frames = [Image.new("RGBA", (64, 96), color) for color in ["red", "blue"]]
    sources = []
    for extension, format in [("gif", "GIF"), ("webp", "WEBP"), ("apng", "PNG")]:
        path = work / f"animation.{extension}"
        frames[0].save(path, format=format, save_all=True, append_images=frames[1:],
                       duration=[30, 70], loop=0, lossless=True)
        sources.append(path)
    agif = work / "animation.agif"
    shutil.copy2(sources[0], agif)
    sources.append(agif)
    png = work / "animation.png"
    shutil.copy2(sources[2], png)
    sources.append(png)
    archive = work / "animation.cbz"
    with zipfile.ZipFile(archive, "x") as output:
        output.write(sources[2], "page.apng")
    sources.append(archive)
    once = work / "once.gif"
    frames[0].save(once, format="GIF", save_all=True, append_images=frames[1:], duration=[30, 70])
    sources.append(once)
    for index, source in enumerate(sources):
        directory = work / f"run-{index}"
        directory.mkdir()
        before = hashlib.sha256(source.read_bytes()).digest()
        result = subprocess.run(image_command(args.image, "--smoke-animation", source),
                                cwd=directory, capture_output=True, text=True, timeout=20)
        assert result.returncode == 0 and "animation_smoke=ok" in result.stdout, (
            f"{source.suffix}: animation display failed (exit {result.returncode}); "
            "inspect synthetic fixtures/backend diagnostics in the reported test directory"
        )
        assert hashlib.sha256(source.read_bytes()).digest() == before
        print(f"PASS {source.name}: {result.stdout.strip()}")
    print(f"Synthetic fixtures/results retained in {work}")
    directory = work / "controls"
    directory.mkdir()
    result = subprocess.run(image_command(args.image, "--smoke-animation-controls", once),
                            cwd=directory, capture_output=True, text=True, timeout=20)
    assert result.returncode == 0 and "animation_controls_smoke=ok" in result.stdout, (
        f"animation controls failed (exit {result.returncode}); inspect {directory}")
    print(f"PASS controls: {result.stdout.strip()}")


if __name__ == "__main__":
    main()
