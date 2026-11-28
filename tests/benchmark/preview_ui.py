#!/usr/bin/env python3
"""Check actual read-only text/hex preview rendering and terminal restoration."""
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

BROWSE_END = b"F5 refresh | q exit"
PAGE_END = b"Up/Down | PgUp/PgDn | q/Esc close"


def frame(master, process, footer):
    deadline = time.monotonic() + 8
    data = bytearray()
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise AssertionError(f"navigator exited early: {process.returncode}")
        if select.select([master], [], [], 0.1)[0]:
            data.extend(os.read(master, 65536))
            assert len(data) <= 1_048_576, "unexpected terminal output size"
            if footer in data:
                return bytes(data)
    raise AssertionError(f"frame footer missing: {footer!r}, {bytes(data[-4096:])!r}")


def session(binary, root, actions):
    master, slave = pty.openpty()
    process = None
    try:
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 28, 160, 0, 0))
        before = termios.tcgetattr(slave)
        process = subprocess.Popen(
            [binary, "--browse", str(root)],
            stdin=slave, stdout=slave, stderr=slave,
            env=dict(os.environ, TERM="xterm-256color"),
        )
        assert b"Rust / read-only" in frame(master, process, BROWSE_END)
        for keys, footer, expected, forbidden in actions:
            os.write(master, keys)
            result = frame(master, process, footer)
            for marker in expected:
                assert marker in result, f"missing {marker!r}: {result!r}"
            for marker in forbidden:
                assert marker not in result, f"unexpected {marker!r}: {result!r}"
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
    with tempfile.TemporaryDirectory(prefix="tt-preview-ui-") as tmp:
        base = Path(tmp)
        text_dir, binary_dir, link_dir, folder_dir = (
            base / name for name in ("text", "binary", "link", "folder")
        )
        for directory in (text_dir, binary_dir, link_dir, folder_dir):
            directory.mkdir()
        text_file = text_dir / "text"
        # Terminal control bytes must be escaped rather than emitted as commands.
        text_data = b"hello\x1b]0;TT_UNSAFE_TITLE\x07\nworld"
        text_file.write_bytes(text_data)
        binary_file = binary_dir / "binary"
        binary_data = b"\x00\x1bA" + b"\xff" * 65534
        binary_file.write_bytes(binary_data)
        link = link_dir / "alias"
        link.symlink_to(text_file)
        (folder_dir / "directory").mkdir()
        close = (b"\x1b", BROWSE_END, [b"Rust / read-only"], [])
        session(binary, text_dir, [
            (b"p", PAGE_END, [b"File preview", br"hello\u{1b}", b"world"],
             [b"\x1b]0;TT_UNSAFE_TITLE"]),
            close,
            (b"\x1b[19~", PAGE_END, [b"Hex preview", b"00000000  68 65 6c 6c 6f 1b "], []),
            close,
        ])
        session(binary, binary_dir, [
            (b"\x1bOR", PAGE_END, [b"Hex preview", b"00000000  00 1b 41 ff "], []),
            (b"\x1b[F", PAGE_END, [b"0000fff0", b"[Preview limited to 64 KiB]"], []),
            close,
        ])
        for directory in (link_dir, folder_dir):
            session(binary, directory, [
                (b"p", BROWSE_END, [b"read requires a regular non-link file"], [b"File preview"]),
            ])
        assert text_file.read_bytes() == text_data
        assert binary_file.read_bytes() == binary_data
        assert link.is_symlink() and link.readlink() == text_file
        assert sorted(str(p.relative_to(base)) for p in base.rglob("*")) == [
            "binary", "binary/binary", "folder", "folder/directory",
            "link", "link/alias", "text", "text/text",
        ]
        print("TT_PREVIEW_UI text escaping, hex, bounds, link refusal and read-only invariants passed")


if __name__ == "__main__":
    main(str(Path(sys.argv[1]).resolve()))
