#!/usr/bin/env python3
"""Cross-language scan compatibility and native terminal smoke tests."""
import argparse
import fcntl
import os
from pathlib import Path
import pty
import select
import struct
import subprocess
import sys
import tempfile
import termios
import time

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, os.environ.get("TT_PYTHON_REFERENCE", str(ROOT / "legacy/python/src")))
import tree


def scan_parity(binary, root):
    root.mkdir()
    names = ("Alpha.txt", "beta.rs", "ข้อมูล.txt", ".hidden", "z space",
             "line\nbreak", "a.tar.gz", "École.txt")
    for i, name in enumerate(names):
        p = root / name
        p.write_bytes(b"x" * (i + 1))
        os.utime(p, ns=(1_700_000_000_000_000_000 + i * 1_000_000_000,) * 2)
    (root / "Folder").mkdir()
    (root / "FolderLink").symlink_to("Folder")
    (root / "Dangling").symlink_to("missing")
    cases = 0
    for hidden in (False, True):
        for order in ("name", "size", "date", "type"):
            for query in ("", "ALPHA", "ข้อมูล", "éco"):
                expected = [os.fsencode(p) for p in tree.scan(str(root), hidden, order, query)]
                actual = subprocess.check_output(
                    [binary, "--scan-paths0", str(root), order, str(int(hidden)), query]
                ).split(b"\0")[:-1]
                if actual != expected:
                    raise AssertionError((hidden, order, query, expected, actual))
                cases += 1
    # Check raw filename bytes are preserved independently of Unicode sorting.
    raw = os.fsencode(root) + b"/invalid-\xff"
    fd = os.open(raw, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600)
    os.close(fd)
    result = subprocess.check_output(
        [binary, "--scan-paths0", str(root), "name", "1", ""]
    ).split(b"\0")
    assert raw in result
    print(f"TT_RUST_PARITY {cases}/32 sorted scans and raw-byte path passed")


def wait_output(master, process, marker, timeout=8):
    end = time.monotonic() + timeout
    data = b""
    while time.monotonic() < end:
        if process.poll() is not None:
            raise AssertionError(f"navigator exited early: {process.returncode}: {data!r}")
        readable, _, _ = select.select([master], [], [], 0.1)
        if readable:
            data += os.read(master, 65536)
            if marker in data:
                return
    raise AssertionError(f"terminal marker missing: {marker!r}: {data!r}")


def terminal_smoke(binary, root):
    root.mkdir()
    (root / "child").mkdir()
    (root / "file.txt").write_text("preserve me")
    master, slave = pty.openpty()
    process = None
    try:
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 100, 0, 0))
        before = termios.tcgetattr(slave)
        process = subprocess.Popen([binary, "--browse", str(root)], stdin=slave,
                                   stdout=slave, stderr=slave,
                                   env=dict(os.environ, TERM="xterm-256color"))
        wait_output(master, process, b"Rust / read-only")
        os.write(master, b"d")
        wait_output(master, process, b"Disabled:")
        os.write(master, b"\r")
        wait_output(master, process, os.fsencode(root / "child"))
        os.write(master, b"q")
        assert process.wait(timeout=8) == 0
        assert termios.tcgetattr(slave) == before, "Terminal state not restored"
        assert (root / "file.txt").read_text() == "preserve me"
        assert sorted(p.name for p in root.iterdir()) == ["child", "file.txt"]
        print("TT_RUST_PTY read-only guard, directory navigation and terminal restoration passed")
    finally:
        if process is not None and process.poll() is None:
            process.kill()
            process.wait()
        os.close(master)
        os.close(slave)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True, type=Path)
    args = parser.parse_args()
    binary = str(args.binary.resolve())
    with tempfile.TemporaryDirectory(prefix="tt-rust-parity-") as tmp:
        scan_parity(binary, Path(tmp) / "scan")
        terminal_smoke(binary, Path(tmp) / "terminal")


if __name__ == "__main__":
    main()
