#!/usr/bin/env python3
"""Linux resource and latency measurements on bounded synthetic fixtures.

No performance threshold: results depend on runner, filesystem and cache state.
GNU time reports filesystem I/O operations, not bytes or device throughput.
"""
import fcntl
import json
import math
import os
from pathlib import Path
import pty
import statistics
import struct
import subprocess
import sys
import tempfile
import termios
import time

from parity import wait_output


def measured(command, metrics):
    start = time.perf_counter()
    result = subprocess.run(
        ["/usr/bin/time", "-o", str(metrics), "-f", "%M %U %S %I %O", *map(str, command)],
        capture_output=True, check=True, timeout=120,
    )
    elapsed = (time.perf_counter() - start) * 1000
    rss, user, system, inputs, outputs = map(float, metrics.read_text().split())
    sample = dict(wall_ms=elapsed, peak_rss_kib=rss, user_cpu_s=user,
                  system_cpu_s=system, fs_input_ops=inputs, fs_output_ops=outputs)
    assert elapsed > 0 and all(math.isfinite(v) and v >= 0 for v in sample.values())
    return result.stdout, sample


def idle_status(pid):
    status = {}
    for line in Path(f"/proc/{pid}/status").read_text().splitlines():
        if line.startswith(("VmRSS:", "VmHWM:")):
            key, value, _ = line.split()
            status[key.rstrip(":")] = int(value)
    return status


def navigation(binary, root):
    left, right = root / "left", root / "right"
    left.mkdir()
    right.mkdir()
    for i in range(1000):
        (left / f"entry-{i:04}").touch()
    master, slave = pty.openpty()
    process = None
    try:
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 160, 0, 0))
        before = termios.tcgetattr(slave)
        start = time.perf_counter()
        process = subprocess.Popen([binary, "--panes", str(left), str(right)],
                                   stdin=slave, stdout=slave, stderr=slave,
                                   env=dict(os.environ, TERM="xterm-256color"))
        wait_output(master, process, b"Focus: Left | Row: 1")
        startup = (time.perf_counter() - start) * 1000
        time.sleep(0.25)
        idle = idle_status(process.pid)
        samples = []
        for row in range(2, 22):
            start = time.perf_counter()
            os.write(master, b"\x1b[B")
            wait_output(master, process, f"Focus: Left | Row: {row} |".encode())
            samples.append((time.perf_counter() - start) * 1000)
        peak = idle_status(process.pid)["VmHWM"]
        os.write(master, b"q")
        assert process.wait(timeout=8) == 0
        assert termios.tcgetattr(slave) == before
        assert len(list(left.iterdir())) == 1000
        return dict(startup_ms=startup, idle_rss_kib=idle["VmRSS"],
                    peak_rss_kib=peak, navigation_samples_ms=samples,
                    navigation_median_ms=statistics.median(samples))
    finally:
        if process is not None and process.poll() is None:
            process.kill()
            process.wait()
        os.close(master)
        os.close(slave)


def main(binary, operation):
    report = {"binary_bytes": Path(binary).stat().st_size, "samples": {}}
    with tempfile.TemporaryDirectory(prefix="tt-resource-") as tmp:
        root = Path(tmp)
        metrics = root / "metrics"
        search = root / "search"
        search.mkdir()
        expected = set()
        for i in range(1000):
            match = i % 20 == 0
            p = search / (f"needle-{i:04}.txt" if match else f"other-{i:04}.txt")
            p.write_text("needle\n" if match else "unrelated\n")
            if match:
                expected.add(os.fsencode(p))
        for name, flags in (("filename_search", []), ("content_search", ["--content"])):
            samples = []
            for _ in range(3):
                output, sample = measured([binary, "--find", search, "needle", *flags], metrics)
                assert set(output.splitlines()) == expected
                samples.append(sample)
            report["samples"][name] = samples
        source = root / "payload"
        payload = bytes(range(256)) * 4096
        source.write_bytes(payload)
        for compressed in (False, True):
            label = "gzip" if compressed else "tar"
            report["samples"][label + "_build"] = []
            report["samples"][label + "_list"] = []
            report["samples"][label + "_member"] = []
            for i in range(3):
                archive = root / f"{label}-{i}"
                flags = ["--gzip"] if compressed else []
                output, sample = measured([binary, "--archive-build", *flags,
                                           "--stdout", "payload", source], metrics)
                archive.write_bytes(output)
                report["samples"][label + "_build"].append(sample)
                output, sample = measured([binary, "--archive-list", archive], metrics)
                assert output == f"file\t{len(payload)}\tpayload\n".encode()
                report["samples"][label + "_list"].append(sample)
                output, sample = measured([binary, "--archive-member", archive, "payload",
                                           "--stdout"], metrics)
                assert output == payload
                report["samples"][label + "_member"].append(sample)
        for kind in ("copy", "move"):
            report["samples"][kind] = []
            for i in range(3):
                work = root / f"{kind}-{i}"
                work.mkdir()
                journal = root / f"journal-{kind}-{i}"
                journal.mkdir(mode=0o700)
                (work / "source").write_bytes(payload)
                _, sample = measured([operation, work, journal, kind], metrics)
                assert (work / "destination").read_bytes() == payload
                assert (work / "source").exists() == (kind == "copy")
                assert not list(journal.glob("*.pending.json"))
                report["samples"][kind].append(sample)
        nav = root / "nav"
        nav.mkdir()
        report["navigation"] = navigation(binary, nav)
    print("TT_RESOURCE_BENCHMARK " + json.dumps(report, sort_keys=True))


if __name__ == "__main__":
    main(str(Path(sys.argv[1]).resolve()), str(Path(sys.argv[2]).resolve()))
