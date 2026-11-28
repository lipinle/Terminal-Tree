#!/usr/bin/env python3
"""Exercise the Rust read-only tree in a real PTY."""
import fcntl
import os
from pathlib import Path
import pty
import struct
import subprocess
import sys
import tempfile
import termios

from parity import wait_output


def main(binary):
    with tempfile.TemporaryDirectory(prefix="tt-tree-pty-") as tmp:
        root = Path(tmp)
        (root / "a" / "b").mkdir(parents=True)
        (root / "a" / "b" / "leaf").write_bytes(b"unchanged")
        (root / "z").write_bytes(b"keep")
        master, slave = pty.openpty()
        process = None
        try:
            fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 100, 0, 0))
            before = termios.tcgetattr(slave)
            process = subprocess.Popen(
                [binary, "--tree", str(root)], stdin=slave, stdout=slave, stderr=slave,
                env=dict(os.environ, TERM="xterm-256color"),
            )

            def step(keys, marker):
                os.write(master, keys)
                wait_output(master, process, marker)

            wait_output(master, process, b"Depth: 4 | Rows: 2 | Selected: 1")
            step(b"\x1b[C", b"Depth: 4 | Rows: 3 | Selected: 1")
            step(b"\x1b[B", b"Depth: 4 | Rows: 3 | Selected: 2")
            step(b"\x1b[C", b"Depth: 4 | Rows: 4 | Selected: 2")
            step(b"-", b"Depth: 3 | Rows: 4 | Selected: 2")
            step(b"-", b"Depth: 2 | Rows: 3 | Selected: 2")
            step(b"\x1b[D", b"Depth: 2 | Rows: 3 | Selected: 1")
            # SGR mouse: left click visible first row; right click selects only.
            step(b"\x1b[<0;1;2M", b"Depth: 2 | Rows: 2 | Selected: 1")
            step(b"\x1b[<0;1;2M", b"Depth: 2 | Rows: 3 | Selected: 1")
            step(b"\x1b[<2;1;3M", b"Depth: 2 | Rows: 3 | Selected: 2")
            step(b"\x1b[<0;1;24M", b"Depth: 2 | Rows: 3 | Selected: 2")
            step(b"i", b"Observed size:")
            step(b"\x1b", b"Depth: 2 | Rows: 3 | Selected: 2")
            step(b"\x1bOP", b"Tree help")
            step(b"q", b"Depth: 2 | Rows: 3 | Selected: 2")
            step(b"d", b"Read-only tree. File mutations disabled.")
            os.write(master, b"q")
            assert process.wait(timeout=8) == 0
            assert termios.tcgetattr(slave) == before
            assert sorted(str(p.relative_to(root)) for p in root.rglob("*")) == [
                "a", "a/b", "a/b/leaf", "z"
            ]
            assert (root / "a" / "b" / "leaf").read_bytes() == b"unchanged"
            assert (root / "z").read_bytes() == b"keep"
            print("TT_TREE_PTY keyboard, mouse, depth and read-only invariants passed")
        finally:
            if process is not None and process.poll() is None:
                process.kill()
                process.wait()
            os.close(master)
            os.close(slave)


if __name__ == "__main__":
    main(str(Path(sys.argv[1]).resolve()))
