#!/usr/bin/env python3
"""Package an explicitly UNTESTED cross-built Windows EXE without changing readiness."""
import argparse
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parents[2]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--toolchain", required=True, type=Path)
    parser.add_argument("--output", type=Path, default=ROOT / "dist-windows-experimental")
    args = parser.parse_args()
    version = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    subprocess.run([sys.executable, str(ROOT / "packaging/windows/build-single.py"),
                    "--toolchain", str(args.toolchain)], check=True)
    name = f"nCatImage-{version}"
    shutil.copy2(ROOT / "target/windows-single/nCatImage.exe",
                 output / f"{name}-UNTESTED-windows-x86_64.exe")
    warning = (
        "WARNING: UNTESTED / Windows 정상 동작 및 실사용 검증이 완료되지 않았습니다.\n"
        "Experimental, unsigned development build. Image display/security/input are not verified.\n"
        "Initial native CI failed: private owner/DACL required. This resize fix does not fix that error.\n"
        "Wine Job Object readback fails; protected decoding is refused. No protection bypass.\n"
        "FTP/SFTP, DPI/accessibility and Linux feature parity remain incomplete.\n"
        "Only Windows system DLLs are imported; no GTK DLL or worker EXE extraction.\n"
        "Private temporary pixel data files may be created.\n"
        "See the release notes and doc/11-windows-single-exe-wine.md.\n"
    )
    (output / f"{name}-windows-UNTESTED.txt").write_text(warning, encoding="utf-8")
    notices = ROOT / "target" / f"windows-notices-{version}"
    subprocess.run([sys.executable, str(ROOT / "packaging/collect-rust-notices.py"),
                    str(notices)], check=True)
    shutil.copy2(ROOT / "packaging/releases" / f"v{version}.md", notices / "RELEASE-NOTES.md")
    (notices / "WINDOWS-UNTESTED.txt").write_text(warning, encoding="utf-8")
    report = json.loads((ROOT / "target/windows-single-build.json").read_text())
    report["native_windows_tested"] = False
    report["native_windows_validation_passed"] = False
    report["known_native_failure"] = "private owner/DACL required (initial native CI)"
    report["release_ready"] = False
    (notices / "BUILD-INFO.json").write_text(json.dumps(report, indent=2) + "\n")
    # Cargo registry license files may carry reproducible pre-1980 timestamps.
    with zipfile.ZipFile(output / f"{name}-windows-NOTICES.zip", "x",
                         compression=zipfile.ZIP_DEFLATED, strict_timestamps=False) as archive:
        for path in sorted(notices.rglob("*")):
            if path.is_file():
                archive.write(path, path.relative_to(notices))
    subprocess.run([sys.executable, str(ROOT / "packaging/checksums.py"), str(output)], check=True)
    print(f"UNTESTED Windows experimental package: {output}")


if __name__ == "__main__":
    main()
