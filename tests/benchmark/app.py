#!/usr/bin/env python3
"""PTY acceptance for the application shell and its shared scoped engine."""
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


def session(binary, args, root, marker, steps, width=120):
    master, slave = pty.openpty()
    process = None
    try:
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 26, width, 0, 0))
        before = termios.tcgetattr(slave)
        process = subprocess.Popen([binary, *args], cwd=root, stdin=slave,
                                   stdout=slave, stderr=slave,
                                   env=dict(os.environ, TERM="xterm-256color", HOME=str(root)))
        wait_output(master, process, marker)
        for keys, expected in steps:
            os.write(master, keys)
            wait_output(master, process, expected)
        os.write(master, b"q")
        assert process.wait(timeout=8) == 0
        assert termios.tcgetattr(slave) == before
    finally:
        if process is not None and process.poll() is None:
            process.kill()
            process.wait()
        os.close(master)
        os.close(slave)


def main(binary):
    with tempfile.TemporaryDirectory(prefix="tt-app-") as tmp:
        root = Path(tmp)
        (root / "file.txt").write_text("preview-content-marker\n")
        session(binary, [], root, b"preview-content-marker", [
            (b"b", b"BOOKMARKS"),
            (b"B", b"Session bookmarks"),
            (b"\x1b", b"Clipboard: 0 files"),
            (b"\x1b[21~", b"Menu"),
            (b"\x1b", b"Clipboard: 0 files"),
        ])
        session(binary, ["--app", str(root)], root, b"Files *", [], width=45)
        work, journal = root / "work", root / "journal"
        work.mkdir(mode=0o700)
        journal.mkdir(mode=0o700)
        data = work / ".data"
        data.mkdir(mode=0o700)
        (work / "source").write_text("preserve me")
        session(binary, ["--app-workspace", str(work), str(journal), str(data)],
                root, b"Clipboard: 0 files", [(b"c", b"Clipboard: 1 files")])
        assert (work / "source").read_text() == "preserve me"
        assert not list(journal.glob("*.pending.json"))
    print("TT_APP dashboard, preview, menus, compact layout and shared clipboard passed")


if __name__ == "__main__":
    main(str(Path(sys.argv[1]).resolve()))
