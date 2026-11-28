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

            wait(b"Tab 1/1 | Focus: Left | Row: 1 | Theme: terminal")
            for theme in (b"dark", b"light", b"monochrome", b"terminal"):
                step(b"t", b"Theme: " + theme)
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


def custom_keys(binary):
    with tempfile.TemporaryDirectory(prefix="tt-pane-keys-") as tmp:
        root = Path(tmp)
        (root / "a").write_bytes(b"a")
        (root / "b").write_bytes(b"b")
        invalid = subprocess.run(
            [binary, "--panes", str(root), "--keys", "up=down"],
            capture_output=True, timeout=8,
        )
        assert invalid.returncode != 0
        assert b"invalid navigation keys" in invalid.stderr
        master, slave = pty.openpty()
        process = None
        try:
            fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 120, 0, 0))
            before = termios.tcgetattr(slave)
            process = subprocess.Popen(
                [binary, "--panes", str(root), "--keys", "up=down,down=up"],
                stdin=slave, stdout=slave, stderr=slave,
                env=dict(os.environ, TERM="xterm-256color"),
            )
            wait_output(master, process, b"Focus: Left | Row: 1")
            os.write(master, b"\x1b[A")
            wait_output(master, process, b"Focus: Left | Row: 2")
            os.write(master, b"\x1b[B")
            wait_output(master, process, b"Focus: Left | Row: 1")
            os.write(master, b"?")
            wait_output(master, process, b"up=Down down=Up")
            os.write(master, b"\x03")
            assert process.wait(timeout=8) == 0
            assert termios.tcgetattr(slave) == before
            assert (root / "a").read_bytes() == b"a"
            assert (root / "b").read_bytes() == b"b"
            print("TT_PANE_KEYS custom bindings and interrupt invariant passed")
        finally:
            if process is not None and process.poll() is None:
                process.kill()
                process.wait()
            os.close(master)
            os.close(slave)


def bookmarks(binary):
    with tempfile.TemporaryDirectory(prefix="tt-pane-bookmarks-") as tmp:
        root = Path(tmp)
        left, right = root / "left", root / "right"
        (left / "saved").mkdir(parents=True)
        right.mkdir()
        (left / "saved" / "saved-leaf").write_bytes(b"preserve")
        (right / "right-file").write_bytes(b"keep")
        master, slave = pty.openpty()
        process = None
        try:
            fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 160, 0, 0))
            before = termios.tcgetattr(slave)
            process = subprocess.Popen(
                [binary, "--panes", str(left), str(right)],
                stdin=slave, stdout=slave, stderr=slave,
                env=dict(os.environ, TERM="xterm-256color"),
            )

            def step(keys, marker):
                os.write(master, keys)
                wait_output(master, process, marker)

            wait_output(master, process, b"Focus: Left | Row: 1")
            step(b"b", b"Bookmark saved | 1 locations")
            step(b"b", b"cannot save bookmark: Duplicate")
            step(b"\t", b"Focus: Right | Row: 1")
            step(b"B", b"Bookmarks - session only")
            step(b"\r", b"saved-leaf")
            step(b"B", b"Bookmarks - session only")
            step(b"x", b"Terminal Tree [panes / read-only]")
            step(b"B", b"no bookmarks; press b to save")
            step(b"b", b"Bookmark saved | 1 locations")
            step(b"B", b"Bookmarks - session only")
            os.write(master, b"\x03")
            assert process.wait(timeout=8) == 0
            assert termios.tcgetattr(slave) == before
            assert (left / "saved" / "saved-leaf").read_bytes() == b"preserve"
            assert (right / "right-file").read_bytes() == b"keep"
            print("TT_PANE_BOOKMARKS save, open, remove and interrupt invariants passed")
        finally:
            if process is not None and process.poll() is None:
                process.kill()
                process.wait()
            os.close(master)
            os.close(slave)


def bookmark_snapshots(binary):
    with tempfile.TemporaryDirectory(prefix="tt-pane-snapshots-") as tmp:
        root = Path(tmp)
        left, right = root / "left", root / "right"
        (left / "saved").mkdir(parents=True)
        right.mkdir()
        leaf = left / "saved" / "saved-leaf"
        leaf.write_bytes(b"preserve")
        snapshot = root / "bookmarks.ttb"

        def session(options, actions):
            master, slave = pty.openpty()
            process = None
            try:
                fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 160, 0, 0))
                before = termios.tcgetattr(slave)
                process = subprocess.Popen(
                    [binary, "--panes", str(left), str(right), *options],
                    stdin=slave, stdout=slave, stderr=slave,
                    env=dict(os.environ, TERM="xterm-256color"),
                )
                wait_output(master, process, b"Focus: Left | Row: 1")
                for keys, marker in actions:
                    os.write(master, keys)
                    wait_output(master, process, marker)
                os.write(master, b"q")
                assert process.wait(timeout=8) == 0
                assert termios.tcgetattr(slave) == before
            finally:
                if process is not None and process.poll() is None:
                    process.kill()
                    process.wait()
                os.close(master)
                os.close(slave)

        session(["--bookmarks-out", str(snapshot)], [
            (b"b", b"Bookmark saved | 1 locations"),
        ])
        assert not snapshot.exists(), "Unexpected automatic save"
        session(["--bookmarks-out", str(snapshot)], [
            (b"b", b"Bookmark saved | 1 locations"),
            (b"S", b"Bookmark snapshot exported"),
        ])
        old = snapshot.read_bytes()
        assert snapshot.stat().st_mode & 0o777 == 0o600
        session(["--bookmarks-in", str(snapshot), "--bookmarks-out", str(snapshot)], [
            (b"\t", b"Focus: Right | Row: 0"),
            (b"B", b"Bookmarks - session only"),
            (b"\r", b"saved-leaf"),
            (b"S", b"bookmark snapshot already exists"),
        ])
        assert snapshot.read_bytes() == old
        assert leaf.read_bytes() == b"preserve"
        assert sorted(p.name for p in root.iterdir()) == ["bookmarks.ttb", "left", "right"]
        print("TT_PANE_SNAPSHOTS explicit save, reload and no-overwrite invariants passed")


if __name__ == "__main__":
    binary = str(Path(sys.argv[1]).resolve())
    main(binary)
    custom_keys(binary)
    bookmarks(binary)
    bookmark_snapshots(binary)
