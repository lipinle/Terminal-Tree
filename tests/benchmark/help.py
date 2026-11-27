#!/usr/bin/env python3
"""Open categorized help, scroll a narrow terminal, and return without writes."""
import os, pty, fcntl, struct, termios, subprocess, tempfile, sys
from pathlib import Path
from parity import wait_output

def main(binary):
    with tempfile.TemporaryDirectory(prefix="tt-help-") as tmp:
        root=Path(tmp)
        (root/"fixture").write_bytes(b"unchanged")
        master,slave=pty.openpty();p=None
        try:
            fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack("HHHH",12,40,0,0))
            before=termios.tcgetattr(slave)
            p=subprocess.Popen([binary,"--browse",str(root)],stdin=slave,stdout=slave,stderr=slave,env=dict(os.environ,TERM="xterm-256color"))
            def wait(marker):wait_output(master,p,marker)
            def send(keys):os.write(master,keys)
            wait(b"Rust / read-only")
            send(b"?");wait(b"Terminal Tree help")
            send(b"\r");wait(b"Up / Down")
            send(b"\x1b[6~");wait(b"Lines ")
            send(b"q");wait(b"Terminal Tree help")
            send(b"\x1b[B");wait(b"Terminal Tree help")
            send(b"\r");wait(b"Toggle hidden entries.")
            send(b"q");wait(b"Terminal Tree help")
            send(b"q");wait(b"Rust / read-only")
            send(b"q");assert p.wait(timeout=8)==0
            assert termios.tcgetattr(slave)==before
            assert sorted(x.name for x in root.iterdir())==["fixture"]
            assert (root/"fixture").read_bytes()==b"unchanged"
            print("TT_HELP_PTY categorized help, narrow terminal scrolling and return passed")
        finally:
            if p and p.poll() is None:p.kill();p.wait()
            os.close(master);os.close(slave)
if __name__=="__main__":main(str(Path(sys.argv[1]).resolve()))
