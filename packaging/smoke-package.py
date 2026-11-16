#!/usr/bin/env python3
"""Open a synthetic CBZ from an unpacked/mounted native package, not the build tree."""
import argparse
import os
from pathlib import Path
import plistlib
import subprocess
import sys
import tempfile
import zipfile

ROOT = Path(__file__).resolve().parent.parent
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("package", type=Path)
args = parser.parse_args()
package = args.package.resolve(strict=True)
with tempfile.TemporaryDirectory(prefix="ncat-package-") as temporary:
    work = Path(temporary)
    fixture = work / "fixtures"
    subprocess.run([sys.executable, str(ROOT / "tools/smoke-fixtures.py"), str(fixture)], check=True)
    environment = os.environ.copy()
    environment.pop("NCAT_WORKER", None)  # Must exercise the packaged sibling worker.
    environment.pop("GTK_PATH", None)
    environment.pop("GDK_PIXBUF_MODULE_FILE", None)
    if sys.platform == "win32":
        with zipfile.ZipFile(package) as archive:
            archive.extractall(work / "unpacked")
        app = work / "unpacked/nCatImage"
        environment["PATH"] = str(app) + os.pathsep + str(Path(environment["SystemRoot"]) / "System32")
        subprocess.run(["cmd", "/d", "/c", str(app / "nCatImage.cmd"),
                        "--smoke-test", str(fixture / "sample.cbz")],
                       cwd=work, env=environment, check=True, timeout=30)
    elif sys.platform == "darwin":
        mount = work / "mounted"
        result = subprocess.check_output(["hdiutil", "attach", "-readonly", "-nobrowse",
                                          "-mountpoint", str(mount), "-plist", str(package)])
        plistlib.loads(result)  # Reject malformed attach output before launching.
        try:
            environment["PATH"] = "/usr/bin:/bin:/usr/sbin:/sbin"
            environment.pop("DYLD_LIBRARY_PATH", None)
            subprocess.run([str(mount / "nCatImage.app/Contents/MacOS/nCatImage"),
                            "--smoke-test", str(fixture / "sample.cbz")],
                           cwd=work, env=environment, check=True, timeout=30)
        finally:
            subprocess.run(["hdiutil", "detach", str(mount)], check=True)
    else:
        raise SystemExit("Native Windows/macOS package required; Linux uses desktop-regression.py")
print("Packaged GUI + sibling worker synthetic-page smoke passed")
