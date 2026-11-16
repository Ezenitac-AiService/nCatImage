#!/usr/bin/env python3
"""Native Apple Silicon GTK .app + DMG; ad-hoc signed development preview only."""
from pathlib import Path
import platform
import plistlib
import shutil
import subprocess
import sys
import tomllib

ROOT = Path(__file__).resolve().parents[2]


def run(*args):
    subprocess.run(args, cwd=ROOT, check=True)


def output(*args):
    return subprocess.check_output(args, text=True).strip()


def main():
    run(sys.executable, str(ROOT / "packaging/preflight.py"), "macos")
    if sys.platform != "darwin" or platform.machine() != "arm64":
        raise SystemExit("Build natively on macOS Apple Silicon; this is not a universal binary")
    version = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
    run("cargo", "build", "-p", "ncat-desktop", "-p", "ncat-worker", "--release", "--locked")
    stage = ROOT / "target/dmg"
    stage.mkdir(exist_ok=False)
    app = stage / "nCatImage.app"
    contents = app / "Contents"
    executables = contents / "MacOS"
    resources = contents / "Resources"
    frameworks = contents / "Frameworks"
    for directory in [executables, resources, frameworks]:
        directory.mkdir(parents=True)
    for source, name in [("ncat-desktop", "nCatImage-bin"), ("ncat-worker", "ncat-worker")]:
        shutil.copy2(ROOT / "target/release" / source, executables / name)
    prefix = Path(output("brew", "--prefix"))
    shutil.copy2(prefix / "bin/gdk-pixbuf-query-loaders", executables)
    for directory in ["share/glib-2.0/schemas", "share/icons/Adwaita", "share/icons/hicolor"]:
        shutil.copytree(prefix / directory, resources / directory)
    loaders = Path(output("pkg-config", "--variable=gdk_pixbuf_moduledir", "gdk-pixbuf-2.0"))
    shutil.copytree(loaders, resources / "loaders")
    machos = list(executables.iterdir()) + list((resources / "loaders").glob("*.so"))
    # Rewrite non-system dylibs into the bundle, including dynamically loaded pixbuf modules.
    for binary in machos:
        run("dylibbundler", "-od", "-b", "-x", str(binary), "-d", str(frameworks),
            "-p", "@executable_path/../Frameworks", "-s", str(prefix / "lib"))
    # Do not silently ship binaries that only work on a Homebrew development machine.
    for binary in machos + list(frameworks.iterdir()):
        for line in output("otool", "-L", str(binary)).splitlines()[1:]:
            dependency = line.strip().split(" (", 1)[0]
            if dependency.startswith("/") and not dependency.startswith(("/usr/lib/", "/System/Library/")):
                raise SystemExit(f"Unbundled dylib: {dependency}")
    launcher = executables / "nCatImage"
    launcher.write_text(
        '#!/bin/sh\nset -eu\nHERE="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"\n'
        'export GSETTINGS_SCHEMA_DIR="$HERE/../Resources/share/glib-2.0/schemas"\n'
        'export XDG_DATA_DIRS="$HERE/../Resources/share"\n'
        'export GDK_PIXBUF_MODULEDIR="$HERE/../Resources/loaders"\n'
        'CACHE="$(mktemp -d "${TMPDIR:-/tmp}/ncat-loaders.XXXXXXXX")"\n'
        'trap \'rm -f "$CACHE/loaders.cache"; rmdir "$CACHE"\' EXIT\n'
        'export GDK_PIXBUF_MODULE_FILE="$CACHE/loaders.cache"\n'
        '"$HERE/gdk-pixbuf-query-loaders" > "$GDK_PIXBUF_MODULE_FILE"\n'
        '"$HERE/nCatImage-bin" "$@"\n')
    launcher.chmod(0o755)
    with (contents / "Info.plist").open("wb") as file:
        plistlib.dump({
            "CFBundleName": "nCatImage",
            "CFBundleDisplayName": "nCatImage",
            "CFBundleIdentifier": "io.github.ncatimage.Development",
            "CFBundleExecutable": "nCatImage",
            "CFBundlePackageType": "APPL",
            "CFBundleShortVersionString": version,
            "CFBundleVersion": version,
            "NSHighResolutionCapable": True,
        }, file)
    shutil.copy2(ROOT / "README.md", resources)
    notices = resources / "licenses"
    run(sys.executable, str(ROOT / "packaging/collect-rust-notices.py"), str(notices))
    # Retain installed formula notices alongside dependency versions.
    for formula in output("brew", "list", "--formula").splitlines():
        cellar = Path(output("brew", "--cellar", formula))
        for source in cellar.rglob("*"):
            if source.is_file() and source.name.upper().startswith(("LICENSE", "COPYING", "NOTICE", "COPYRIGHT")):
                destination = notices / "homebrew" / formula / source.relative_to(cellar)
                destination.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(source, destination)
    (resources / "BUILD-INFO.txt").write_text(
        "Unsigned/not notarized development preview (ad-hoc signature only).\n"
        + output("sw_vers") + "\n" + output("brew", "list", "--versions") + "\n")
    run("codesign", "--force", "--deep", "--sign", "-", str(app))
    run("codesign", "--verify", "--deep", "--strict", str(app))
    (stage / "Applications").symlink_to("/Applications")
    dist = ROOT / "dist"
    dist.mkdir(exist_ok=True)
    image = dist / f"nCatImage-{version}-macos-arm64.dmg"
    if image.exists():
        raise SystemExit("Refusing to overwrite existing DMG")
    run("hdiutil", "create", "-volname", f"nCatImage {version}", "-srcfolder", str(stage),
        "-format", "UDZO", str(image))
    run("hdiutil", "verify", str(image))
    print(image)


if __name__ == "__main__":
    main()
