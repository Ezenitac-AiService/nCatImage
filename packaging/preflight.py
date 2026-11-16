#!/usr/bin/env python3
"""Fail closed before a not-yet-ported OS can produce a misleading release asset."""
import argparse
import json
from pathlib import Path
import sys
import tomllib

ROOT = Path(__file__).resolve().parent.parent


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("platform", choices=["linux", "windows", "macos"])
    parser.add_argument("--tag", help="release tag, which must match Cargo version")
    args = parser.parse_args()
    version = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
    if args.tag and args.tag != f"v{version}":
        raise SystemExit(f"Tag {args.tag} does not match Cargo version v{version}")
    target = json.loads((ROOT / "packaging/targets.json").read_text())[args.platform]
    print(f"{args.platform}: nCatImage {version}, {target['architecture']}, {target['format']}", flush=True)
    if not target["ready"]:
        print("BLOCKED: native port is not release-ready. No placeholder asset will be uploaded.", file=sys.stderr)
        for blocker in target["blockers"]:
            print(f"  - {blocker}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main())
