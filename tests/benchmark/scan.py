#!/usr/bin/env python3
"""Measure equivalent name-sorted directory scans and process startup.
No timing assertion: CI machines and filesystem caches vary.
"""
import argparse
import json
import math
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


def timed(cmd, expected, rounds, timeout=120):
    samples = []
    for _ in range(rounds):
        start = time.perf_counter()
        result = subprocess.run(
            cmd, check=True, capture_output=True, text=True, timeout=timeout,
        )
        samples.append((time.perf_counter() - start) * 1000)
        if result.stdout.strip() != str(expected):
            raise AssertionError(f"count mismatch: {cmd!r}: {result.stdout!r}")
    median = statistics.median(samples)
    if not math.isfinite(median) or median <= 0:
        raise ValueError("invalid elapsed time")
    return median


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--entries", type=int, default=10000)
    parser.add_argument("--rounds", type=int, default=5)
    parser.add_argument("--timeout", type=float, default=120)
    args = parser.parse_args()
    if not 1 <= args.entries <= 100000 or not 1 <= args.rounds <= 100:
        parser.error("entries must be 1..100000 and rounds 1..100")
    if not 0 < args.timeout <= 3600:
        parser.error("timeout must be greater than 0 and at most 3600 seconds")
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
            py = [sys.executable, "-c", PY_SCAN, os.environ.get("TT_PYTHON_REFERENCE", str(ROOT/"legacy/python/src")), str(directory), str(repeats)]
            rust = [binary, "--scan-count", str(directory), str(repeats)]
            # Warm both implementations and validate counts, then alternate order.
            timed(py, entries*repeats, 1, args.timeout)
            timed(rust, entries*repeats, 1, args.timeout)
            p_samples, r_samples = [], []
            for i in range(args.rounds):
                pair = [(py, p_samples), (rust, r_samples)]
                if i % 2: pair.reverse()
                for command, samples in pair:
                    samples.append(timed(command, entries*repeats, 1, args.timeout))
            p_ms = statistics.median(p_samples)
            r_ms = statistics.median(r_samples)
            # Do not quantize timings before computing a ratio. Very short
            # samples can round to zero; preserve raw finite samples for analysis.
            result[label] = {"python_ms": round(p_ms, 3), "rust_ms": round(r_ms, 3),
                             "python_over_rust": round(p_ms / r_ms, 2),
                             "python_samples_ms": p_samples,
                             "rust_samples_ms": r_samples}
    print("TT_NATIVE_BENCHMARK " + json.dumps(result, sort_keys=True))


if __name__ == "__main__":
    main()
