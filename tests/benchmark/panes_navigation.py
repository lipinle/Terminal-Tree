#!/usr/bin/env python3
"""Exercise the Rust dual-pane navigator in a real terminal."""
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
    with tempfile.TemporaryDirectory(prefix="tt-panes-pty-") as tmp:
        root = Path(tmp)
        left, right = root / "left", root / "right"
        (left / "a").mkdir(parents=True)
        (right / "b").mkdir(parents=True)
        (left / "a" / "left-leaf").write_bytes(b"left-data")
        (right / "b" / "right-leaf").write_bytes(b"right-data")
        (left / "z").write_bytes(b"keep")
        master, slave = pty.openpty()
        process = None
        try:
            fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 120, 0, 0))
            before = termios.tcgetattr(slave)
            process = subprocess.Popen(
                [binary, "--panes", str(left), str(right)],
                stdin=slave, stdout=slave, stderr=slave,
                env=dict(os.environ, TERM="xterm-256color"),
            )

            def wait(marker):
                wait_output(master, process, marker)

            def step(keys, marker):
                os.write(master, keys)
                wait(marker)

            wait(b"Tab 1/1 | Focus: Left | Row: 1")
            step(b"\x1b[C", b"left-leaf")
            step(b"\x1b[B", b"Tab 1/1 | Focus: Left | Row: 2")
            step(b"\t", b"Tab 1/1 | Focus: Right | Row: 1")
            step(b"\x1b[C", b"right-leaf")
            step(b"n", b"Tab 2/2 | Focus: Left | Row: 1")
            step(b"[", b"Tab 1/2 | Focus: Right | Row: 1")
            step(b"\t", b"Tab 1/2 | Focus: Left | Row: 2")
            # SGR mouse click on the right pane's first row.
            step(b"\x1b[<0;61;3M", b"Tab 1/2 | Focus: Right | Row: 1")
            step(b"]", b"Tab 2/2 | Focus: Left | Row: 1")
            step(b"w", b"Tab 1/1 | Focus: Right | Row: 1")
            step(b"w", b"cannot close the last tab")
            step(b"n", b"Tab 2/2 | Focus: Left | Row: 1")
            step(b"n", b"Tab 3/3 | Focus: Left | Row: 1")
            step(b"n", b"Tab 4/4 | Focus: Left | Row: 1")
            step(b"n", b"tab limit reached")
            step(b"d", b"Read-only panes. File mutations disabled.")
            os.write(master, b"q")
            assert process.wait(timeout=8) == 0
            assert termios.tcgetattr(slave) == before
            assert sorted(str(path.relative_to(root)) for path in root.rglob("*")) == [
                "left", "left/a", "left/a/left-leaf", "left/z",
                "right", "right/b", "right/b/right-leaf",
            ]
            assert (left / "a" / "left-leaf").read_bytes() == b"left-data"
            assert (right / "b" / "right-leaf").read_bytes() == b"right-data"
            assert (left / "z").read_bytes() == b"keep"
            print("TT_PANES_PTY independent state, focus, limits and read-only invariants passed")
        finally:
            if process is not None and process.poll() is None:
                process.kill()
                process.wait()
            os.close(master)
            os.close(slave)


if __name__ == "__main__":
    main(str(Path(sys.argv[1]).resolve()))
