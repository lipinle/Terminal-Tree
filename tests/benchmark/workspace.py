#!/usr/bin/env python3
"""PTY end-to-end writes only inside a private synthetic workspace."""
import json, os, pty, fcntl, struct, termios, subprocess, tempfile, sys
from pathlib import Path
from parity import wait_output

def main(binary, mode):
    marker = b"Rust / " + mode.encode("ascii") + b"]"
    flags = ["--paranoid"] if mode == "Paranoid" else []
    with tempfile.TemporaryDirectory(prefix="tt-workspace-") as tmp:
        base=Path(tmp)
        root=base/"workspace"; root.mkdir(mode=0o700)
        journal=base/"journal"; journal.mkdir(mode=0o700)
        data=root/".data"; data.mkdir(mode=0o700)
        (root/"original").write_bytes(b"keep")
        master, slave=pty.openpty()
        p=None
        try:
            fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack("HHHH",24,120,0,0))
            before=termios.tcgetattr(slave)
            p=subprocess.Popen([binary,*flags,"--workspace",str(root),str(journal),str(data)],stdin=slave,stdout=slave,stderr=slave,env=dict(os.environ,TERM="xterm-256color"))
            def wait(marker): wait_output(master,p,marker)
            def send(keys): os.write(master,keys)
            wait(marker)
            send(b"r"); wait(b"Name:")
            send(b"renamed\r"); wait(b"Rename item")
            send(b"\r"); wait(b"Type yes:")
            send(b"yes\r"); wait(b"Renamed")
            assert not (root/"original").exists() and (root/"renamed").read_bytes()==b"keep"
            send(b"u"); wait(b"Undo move")
            send(b"\r"); wait(b"Type yes:")
            send(b"yes\r"); wait(b"Undone")
            assert (root/"original").read_bytes()==b"keep"
            send(b"t"); wait(b"Name:")
            send(b"newfile\r"); wait(b"Create item")
            send(b"\r"); wait(b"Type yes:")
            send(b"no\r"); wait(b"Cancelled")
            assert not (root/"newfile").exists()
            send(b"t"); wait(b"Name:")
            send(b"newfile\r"); wait(b"Create item")
            send(b"\r"); wait(b"Type yes:")
            send(b"yes\r"); wait(b"Created")
            assert (root/"newfile").is_file()
            send(b"q"); assert p.wait(timeout=8)==0
            assert termios.tcgetattr(slave)==before
            assert not list(journal.glob("*.pending.json"))
            done = list(journal.glob("*.done.json"))
            assert len(done)==3
            assert all(json.loads(p.read_text())["safety_mode"]==mode for p in done)
            print(f"TT_WORKSPACE_PTY {mode}: rename, undo, cancellation, create, journal and terminal restoration passed")
        finally:
            if p and p.poll() is None: p.kill(); p.wait()
            os.close(master); os.close(slave)
if __name__=="__main__":
    binary = str(Path(sys.argv[1]).resolve())
    for mode in ("Standard", "Paranoid"):
        main(binary, mode)
