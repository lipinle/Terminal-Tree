#!/usr/bin/env python3
"""Batch UI exercises against private synthetic files."""
import os, pty, fcntl, struct, termios, subprocess, tempfile, sys
from pathlib import Path
from parity import wait_output

def main(binary, mode):
    marker = b"Rust / " + mode.encode("ascii") + b"]"
    flags = ["--paranoid"] if mode == "Paranoid" else []
    with tempfile.TemporaryDirectory(prefix="tt-batch-") as tmp:
        base=Path(tmp); root=base/"workspace"; root.mkdir(mode=0o700)
        journal=base/"journal"; journal.mkdir(mode=0o700)
        data=root/".data"; data.mkdir(mode=0o700)
        source=root/"source"; source.mkdir()
        dest=root/"dest"; dest.mkdir()
        moved=root/"moved"; moved.mkdir()
        for n in ("a","b"):
            (source/n).write_bytes(n.encode())
        outside=base/"sentinel"; outside.write_bytes(b"outside")
        master,slave=pty.openpty(); p=None
        try:
            fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack("HHHH",28,160,0,0))
            before=termios.tcgetattr(slave)
            p=subprocess.Popen([binary,*flags,"--workspace",str(root),str(journal),str(data)],
                stdin=slave,stdout=slave,stderr=slave,env=dict(os.environ,TERM="xterm-256color"))
            def wait(marker): wait_output(master,p,marker)
            def send(keys): os.write(master,keys)
            def go(path):
                send(b"g"); wait(b"Go to path:")
                send(os.fsencode(path)+b"\r"); wait(marker+b" | "+os.fsencode(path))
            def confirm(title,final,word=b"yes"):
                wait(title); send(b"\r"); wait(b"Type "+word+b":")
                send(word+b"\r"); wait(final)
            wait(marker)
            go(source)
            send(b" "); wait(b"1 marked")
            send(b"\x1b[B"); wait(marker)
            send(b" "); wait(b"2 marked")
            send(b"c"); wait(b"Clipboard ready")
            go(dest)
            # Last destination conflicts: no first copy or journal mutation.
            (dest/"b").write_bytes(b"sentinel")
            send(b"v"); wait(b"destination exists")
            assert not (dest/"a").exists()
            assert (dest/"b").read_bytes()==b"sentinel"
            assert not list(journal.iterdir())
            (dest/"b").unlink()
            send(b"v"); confirm(b"Copy 2 items",b"Transfer: 2/2 complete")
            assert [(dest/n).read_bytes() for n in ("a","b")]==[b"a",b"b"]
            # Marked items retain their original stamp through refresh.
            send(b"a"); wait(b"2 marked")
            (dest/"b").write_bytes(b"changed")
            send(b"\x1b[15~"); wait(marker)
            send(b"x"); wait(b"marked item hidden, missing or changed")
            send(b"A"); wait(b"Marks cleared")
            send(b"a"); wait(b"2 marked")
            send(b"x"); wait(b"Clipboard ready")
            go(moved)
            send(b"v"); confirm(b"Move 2 items",b"Transfer: 2/2 complete")
            assert not list(dest.iterdir())
            assert [(moved/n).read_bytes() for n in ("a","b")]==[b"a",b"changed"]
            send(b"a"); wait(b"2 marked")
            send(b"d"); confirm(b"Trash 2 items",b"Trash: 2/2 complete")
            assert not list(moved.iterdir())
            trash=data/"Trash/files"
            go(trash)
            send(b"a"); wait(b"2 marked")
            send(b"R"); confirm(b"Restore 2 items",b"Restore: 2/2 complete")
            assert [(moved/n).read_bytes() for n in ("a","b")]==[b"a",b"changed"]
            go(moved)
            send(b"a"); wait(b"2 marked")
            send(b"d"); confirm(b"Trash 2 items",b"Trash: 2/2 complete")
            go(trash)
            send(b"a"); wait(b"2 marked")
            send(b"D"); confirm(b"PERMANENT DELETE 2 items",b"Purge: 2/2 complete",b"DELETE")
            assert not list(trash.iterdir())
            assert [(source/n).read_bytes() for n in ("a","b")]==[b"a",b"b"]
            assert outside.read_bytes()==b"outside"
            assert not list(journal.glob("*.pending.json"))
            send(b"q"); assert p.wait(timeout=8)==0
            assert termios.tcgetattr(slave)==before
            print(f"TT_BATCH_PTY {mode}: marks, stale refusal, collision preflight, copy, move, Trash, restore and purge passed")
        finally:
            if p and p.poll() is None: p.kill(); p.wait()
            os.close(master); os.close(slave)
if __name__=="__main__":
    binary = str(Path(sys.argv[1]).resolve())
    for mode in ("Standard", "Paranoid"):
        main(binary, mode)
