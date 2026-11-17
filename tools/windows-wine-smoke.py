#!/usr/bin/env python3
"""Isolated Wine platform/UI checks; not protected decoding or native security certification."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess

from smoke_support import arguments, work_directory


def main():
    args = arguments(__doc__)
    wine = shutil.which("wine")
    if not wine:
        raise SystemExit("Wine is required; the system/default prefix will not be installed or changed")
    work = work_directory("ncat-wine-")
    deployed = work / "deployment 한글 space"
    deployed.mkdir()
    image = deployed / "nCatImage.exe"
    shutil.copy2(args.image, image)
    before = hashlib.sha256(image.read_bytes()).hexdigest()
    environment = os.environ.copy()
    for variable in ["WINESERVER", "WINELOADER", "WINEDLLPATH", "WINEPATH"]:
        environment.pop(variable, None)
    environment["WINEPREFIX"] = str(work / "prefix")
    environment["WINEARCH"] = "win64"
    environment["WINEDEBUG"] = "-all"
    environment["WINEDLLOVERRIDES"] = "mscoree,mshtml="
    results = []
    print(f"Isolated Wine prefix and results: {work}", flush=True)
    for option, expected in [("--self-test-platform", 0), ("--smoke-window", 0),
                             ("--smoke-resize", 0), ("--worker", 2)]:
        # Wine creates its own OS files in the private prefix, never in deployment.
        with (work / (option.removeprefix("--") + ".log")).open("wb") as log:
            result = subprocess.run([wine, str(image), option], cwd=deployed, env=environment,
                                    stdout=log, stderr=subprocess.STDOUT, timeout=90)
        assert result.returncode == expected, f"{option} exit {result.returncode}; inspect {work}"
        assert sorted(path.name for path in deployed.iterdir()) == ["nCatImage.exe"], "runtime payload was extracted"
        assert hashlib.sha256(image.read_bytes()).hexdigest() == before
        results.append({"test": option, "exit": result.returncode})
        print(f"PASS {option}: exit={expected}; no extra EXE/DLL/files in deployment", flush=True)
    report = {"stage": "platform-and-empty-UI", "wine": subprocess.check_output([wine, "--version"], text=True).strip(),
              "sha256": before, "tests": results,
               "not_tested": ["protected image decoding", "native ACL/reparse security", "Job enforcement", "native Windows"]}
    (work / "result.json").write_text(json.dumps(report, indent=2) + "\n")
    print("Platform/UI checks passed. Protected decoding requires --self-test on native Windows; readiness stays false.")


if __name__ == "__main__":
    main()
