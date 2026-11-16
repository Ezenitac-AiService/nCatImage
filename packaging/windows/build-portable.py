#!/usr/bin/env python3
"""MSYS2 UCRT64 native build. ZIP fallback: GTK DLLs + GUI + separate worker."""
import os
import json
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parents[2]


def run(*args):
    subprocess.run(args, cwd=ROOT, check=True)


def main():
    if json.loads((ROOT / "packaging/targets.json").read_text())["windows"]["format"] != "portable.zip":
        raise SystemExit("Legacy GTK ZIP packaging is disabled; the current Windows goal is native single EXE")
    run(sys.executable, str(ROOT / "packaging/preflight.py"), "windows")
    if sys.platform != "win32":
        raise SystemExit("Build on Windows in an MSYS2 UCRT64 environment")
    prefix = Path(os.environ["NCAT_MINGW_PREFIX"])
    version = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
    stage = ROOT / "target/portable-windows"
    stage.mkdir(exist_ok=False)
    run("cargo", "build", "-p", "ncat-desktop", "-p", "ncat-worker", "--release",
        "--locked", "--target", "x86_64-pc-windows-gnu")
    binaries = ROOT / "target/x86_64-pc-windows-gnu/release"
    shutil.copy2(binaries / "ncat-desktop.exe", stage / "nCatImage.exe")
    shutil.copy2(binaries / "ncat-worker.exe", stage / "ncat-worker.exe")
    shutil.copy2(prefix / "bin/gdk-pixbuf-query-loaders.exe", stage)
    for directory in ["share/glib-2.0/schemas", "share/icons/Adwaita", "share/icons/hicolor"]:
        source = prefix / directory
        if not source.is_dir():
            raise SystemExit(f"Missing GTK resource directory: {source}")
        shutil.copytree(source, stage / directory)
    loader_directory = subprocess.check_output(
        ["pkg-config", "--variable=gdk_pixbuf_moduledir", "gdk-pixbuf-2.0"], text=True).strip()
    if loader_directory.startswith("/"):
        loader_directory = subprocess.check_output(["cygpath", "-m", loader_directory], text=True).strip()
    loaders = Path(loader_directory)
    if not loaders.is_dir():
        raise SystemExit("gdk-pixbuf loader directory missing")
    shutil.copytree(loaders, stage / "loaders")
    # Walk PE imports, never copy Windows system libraries into the application.
    available = {p.name.lower(): p for p in (prefix / "bin").glob("*.dll")}
    system = Path(os.environ["SystemRoot"]) / "System32"
    pending = list(stage.glob("*.exe")) + list((stage / "loaders").glob("*.dll"))
    copied = set()
    while pending:
        binary = pending.pop()
        imports = subprocess.check_output(["objdump", "-p", str(binary)], text=True)
        for name in re.findall(r"DLL Name:\s*(\S+)", imports):
            key = name.lower()
            if key in copied:
                continue
            source = available.get(key)
            if source:
                destination = stage / source.name
                shutil.copy2(source, destination)
                copied.add(key)
                pending.append(destination)
            elif not key.startswith(("api-ms-win-", "ext-ms-win-")) and not (system / name).is_file():
                raise SystemExit(f"Unresolved PE import: {name}")
    # Per-user temporary loader cache, no writes to the portable installation.
    (stage / "nCatImage.cmd").write_text(
        '@echo off\nsetlocal\nset "ROOT=%~dp0"\n'
        'set "GSETTINGS_SCHEMA_DIR=%ROOT%share\\glib-2.0\\schemas"\n'
        'set "GDK_PIXBUF_MODULEDIR=%ROOT%loaders"\n'
        'set "GDK_PIXBUF_MODULE_FILE=%TEMP%\\ncat-loaders-%RANDOM%-%RANDOM%.cache"\n'
        '"%ROOT%gdk-pixbuf-query-loaders.exe" > "%GDK_PIXBUF_MODULE_FILE%"\n'
        'if errorlevel 1 exit /b 1\n'
        '"%ROOT%nCatImage.exe" %*\nset "RESULT=%ERRORLEVEL%"\n'
        'del "%GDK_PIXBUF_MODULE_FILE%"\nexit /b %RESULT%\n', encoding="ascii")
    shutil.copy2(ROOT / "README.md", stage)
    # Preserve upstream license records for every installed UCRT64 dependency.
    shutil.copytree(prefix / "share/licenses", stage / "licenses")
    run(sys.executable, str(ROOT / "packaging/collect-rust-notices.py"), str(stage / "licenses"))
    shutil.copy2(ROOT / "Cargo.lock", stage / "Cargo.lock")
    (stage / "BUILD-INFO.txt").write_text(
        "Unsigned development preview; launch nCatImage.cmd.\n"
        "GTK and worker are separate files, not a single-file portable EXE.\n"
        "See README for first-party license status; Cargo.lock identifies Rust dependencies.\n")
    dist = ROOT / "dist"
    dist.mkdir(exist_ok=True)
    output = dist / f"nCatImage-{version}-windows-x86_64-portable.zip"
    with zipfile.ZipFile(output, "x", compression=zipfile.ZIP_DEFLATED) as archive:
        for path in sorted(stage.rglob("*")):
            if path.is_file():
                archive.write(path, Path("nCatImage") / path.relative_to(stage))
    print(output)


if __name__ == "__main__":
    main()
