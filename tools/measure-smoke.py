#!/usr/bin/env python3
"""Linux smoke-only process-tree RSS/PSS sampling. Not the release benchmark gate."""
import argparse
import json
from pathlib import Path
import subprocess
import time


def tree(root):
    result, todo = set(), [root]
    while todo:
        pid = todo.pop()
        if pid in result:
            continue
        result.add(pid)
        try:
            tasks = list(Path(f"/proc/{pid}/task").iterdir())
        except OSError:
            continue
        for task in tasks:
            try:
                todo.extend(map(int, (task / "children").read_text().split()))
            except OSError:
                pass
    return result


def memory(pid):
    try:
        fields = {}
        for line in Path(f"/proc/{pid}/smaps_rollup").read_text().splitlines():
            if ":" in line:
                key, value = line.split(":", 1)
                fields[key] = int(value.split()[0])
        return fields.get("Rss", 0), fields.get("Pss", 0)
    except (OSError, ValueError):
        return 0, 0


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    if not command:
        parser.error("supply a command after --")
    start = time.monotonic()
    process = subprocess.Popen(command)
    peak_rss, peak_pss, peak_count, samples = 0, 0, 0, 0
    while process.poll() is None:
        pids = tree(process.pid)
        sizes = [memory(pid) for pid in pids]
        peak_rss = max(peak_rss, sum(size[0] for size in sizes))
        peak_pss = max(peak_pss, sum(size[1] for size in sizes))
        peak_count = max(peak_count, len(pids))
        samples += 1
        time.sleep(0.01)
    print(json.dumps({"exit_code": process.returncode, "elapsed_ms": round((time.monotonic() - start) * 1000, 2),
                      "sampled_tree_peak_rss_kib": peak_rss, "sampled_tree_peak_pss_kib": peak_pss,
                      "max_processes": peak_count, "samples": samples,
                      "note": "10ms sampled smoke run, includes auto-close delay, not first-presentation latency/p95"}, indent=2))
    raise SystemExit(process.returncode)


if __name__ == "__main__":
    main()
