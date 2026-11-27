#!/usr/bin/env python3
"""Measure equivalent name-sorted directory scans and process startup.
No timing assertion: CI machines and filesystem caches vary.
"""
import argparse
import json
import os
from pathlib import Path
import statistics
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[2]
PY_SCAN = """import sys
sys.path.insert(0, sys.argv[1])
import tree
count = 0
for _ in range(int(sys.argv[3])):
    count += len(tree.scan(sys.argv[2], False, 'name', ''))
print(count)
"""


def timed(cmd, expected, rounds):
    samples = []
    for _ in range(rounds):
        start = time.perf_counter()
        result = subprocess.run(cmd, check=True, capture_output=True, text=True)
        samples.append((time.perf_counter() - start) * 1000)
        if result.stdout.strip() != str(expected):
            raise AssertionError(f"count mismatch: {cmd!r}: {result.stdout!r}")
    return round(statistics.median(samples), 3)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--entries", type=int, default=10000)
    parser.add_argument("--rounds", type=int, default=5)
    args = parser.parse_args()
    binary = str(args.binary.resolve())
    result = {}
    with tempfile.TemporaryDirectory(prefix="tt-native-benchmark-") as tmp:
        p = Path(tmp)
        empty = p / "empty"
        empty.mkdir()
        fixture = p / "fixture"
        fixture.mkdir()
        for i in range(args.entries):
            name = fixture / f"entry-{i:06}.txt"
            if i % 20 == 0:
                name.mkdir()
            else:
                name.touch()
        for label, directory, entries, repeats in [
            ("empty_scan_startup", empty, 0, 1),
            ("large_scan_startup", fixture, args.entries, 1),
            ("large_scan_10_repeats", fixture, args.entries, 10),
        ]:
            py = [sys.executable, "-c", PY_SCAN, str(ROOT/"legacy/python/src"), str(directory), str(repeats)]
            rust = [binary, "--scan-count", str(directory), str(repeats)]
            # Warm both implementations and validate counts, then alternate order.
            timed(py, entries*repeats, 1)
            timed(rust, entries*repeats, 1)
            p_samples, r_samples = [], []
            for i in range(args.rounds):
                pair = [(py, p_samples), (rust, r_samples)]
                if i % 2: pair.reverse()
                for command, samples in pair:
                    samples.append(timed(command, entries*repeats, 1))
            p_ms = round(statistics.median(p_samples), 3)
            r_ms = round(statistics.median(r_samples), 3)
            result[label] = {"python_ms": p_ms, "rust_ms": r_ms,
                             "python_over_rust": round(p_ms / r_ms, 2)}
    print("TT_NATIVE_BENCHMARK " + json.dumps(result, sort_keys=True))


if __name__ == "__main__":
    main()
