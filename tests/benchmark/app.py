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
            (b"\x1b[23~", b"Icon style"),
            (b"\x1b[B", b"Seti"),
            (b"\r", b"Icons: Seti"),
            (b"\x1b[23~", b"Icon style"),
            (b"\x1b[F", b"ASCII"),
            (b"\r", b"Icons: ASCII"),
            (b"\x1b[<0;99;2M", b"Grid View"),
            (b"\x1b[<80;25;5M", b"Zoom 2"),
            (b"\x1b[<81;25;5M", b"Zoom 1"),
            (b"\x1b[<0;106;2M", b"List View"),
            (b"\x1b[<0;113;2M", b"Tree View"),
            (b"\x1b[<0;83;2M", b"Location:"),
            (b"\x1b", b" > " + root.name.encode()),
            (b"\x1b[<0;90;2M", b"Filter: type text"),
            (b"file", b"Search: file"),
            (b"\x1b", b"Icons: ASCII"),
            (b"\x1b[<0;55;1M", b"Add-ons / Integrations"),
            (b"\r", b"Disabled"),
            (b"\x1b", b"Icons: ASCII"),
            (b"\x10", b"disabled in application settings"),
            (b"\r", b"Companion disabled"),
            (b"\x1b", b"Icons: ASCII"),
            (b"\x1b[<0;45;1M", b"Settings Center"),
            (b"\x1b[B", b"Icons / style and import"),
            (b"\x1b", b"Icons: ASCII"),
            (b"b", b"BOOKMARKS"),
            (b"B", b"Session bookmarks"),
            (b"\x1b", b"Clipboard: 0 files"),
            (b"\x1b[21~", b"Menu"),
            (b"\x1b", b"Clipboard: 0 files"),
        ])
        session(binary, ["--app", str(root)], root, b"Files *", [], width=45)
        folder = root / "tree-folder"
        folder.mkdir()
        (folder / "nested.txt").write_text("nested")
        session(binary, ["--app", str(root)], root, b"Tree View", [
            (b"\x1b[C", b"Expanded tree:"),
            (b"\x1b[B", b"nested.txt"),
            (b"\r", b"Tree View"),
        ])
        work, journal = root / "work", root / "journal"
        work.mkdir(mode=0o700)
        journal.mkdir(mode=0o700)
        data = work / ".data"
        data.mkdir(mode=0o700)
        (work / "source").write_text("preserve me")
        session(binary, ["--app-workspace", str(work), str(journal), str(data)],
                root, b"Clipboard: 0 files", [
                    (b"\x03", b"Clipboard: 1 files"),
                    (b"\x1b[24~", b"Clipboard contents"),
                    (b"\x1b", b"Clipboard: 1 files"),
                    (b"n", b"Name:"),
                    (b"new-folder\r", b"Create item"),
                    (b"\r", b"Created"),
                ])
        assert (work / "source").read_text() == "preserve me"
        assert (work / "new-folder").is_dir()
        session(binary, ["--app-workspace", str(work), str(journal), str(data)],
                root, b"Clipboard: 0 files", [
                    (b"\x1b[19~", b"Dual Pane | Active: Left"),
                    (b"\t", b"Dual Pane | Active: Right"),
                    (b"\r", b"Right * | " + os.fsencode(work / "new-folder")),
                    (b"\t", b"Dual Pane | Active: Left"),
                    (b"\x1b[F", b"Dual Pane | Active: Left"),
                    (b"\x03", b"Clipboard: 1 files"),
                    (b"\t", b"Dual Pane | Active: Right"),
                    (b"\x16", b"Review paths | Enter confirm"),
                    (b"\r", b"Transfer completed"),
                    (b"\x1b[19~", b"Files *"),
                    (b"\x1b[19~", b"Dual Pane | Active: Right"),
                    (b"\t", b"Left * | " + os.fsencode(work)),
                ])
        assert (work / "new-folder" / "source").read_text() == "preserve me"
        assert (work / "source").read_text() == "preserve me"
        assert not list(journal.glob("*.pending.json"))
    print("TT_APP dashboard, preview, menus, compact layout and shared clipboard passed")


if __name__ == "__main__":
    main(str(Path(sys.argv[1]).resolve()))
