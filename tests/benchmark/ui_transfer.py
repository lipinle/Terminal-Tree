#!/usr/bin/env python3
"""Full terminal interaction against synthetic files, never the user's Trash."""
import os, pty, fcntl, struct, termios, subprocess, tempfile, sys
from pathlib import Path
from parity import wait_output

def main(binary):
    with tempfile.TemporaryDirectory(prefix="tt-ui-transfer-") as tmp:
        base=Path(tmp); root=base/"workspace"; root.mkdir(mode=0o700)
        journal=base/"journal"; journal.mkdir(mode=0o700)
        data=root/".data"; data.mkdir(mode=0o700)
        folder=root/"folder"; folder.mkdir()
        source=root/"source"; source.write_bytes(b"preserve me")
        sentinel=base/"untouched"; sentinel.write_bytes(b"outside")
        master,slave=pty.openpty(); p=None
        try:
            fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack("HHHH",28,160,0,0))
            before=termios.tcgetattr(slave)
            p=subprocess.Popen([binary,"--workspace",str(root),str(journal),str(data)],
                stdin=slave,stdout=slave,stderr=slave,env=dict(os.environ,TERM="xterm-256color"))
            def wait(marker): wait_output(master,p,marker)
            def send(keys): os.write(master,keys)
            def go(path):
                send(b"g"); wait(b"Go to path:")
                send(os.fsencode(path)+b"\r"); wait(b"Rust / workspace] | "+os.fsencode(path))
            def confirm(title,word=b"yes",final=b"Transfer completed"):
                wait(title); send(b"\r"); wait(b"Type "+word+b":")
                send(word+b"\r"); wait(final)
            wait(b"Rust / workspace")
            send(b"\x1b[B"); wait(b"Rust / workspace")
            send(b"i"); wait(b"Properties"); send(b"q"); wait(b"Rust / workspace")
            send(b"p"); wait(b"File preview"); send(b"q"); wait(b"Rust / workspace")
            send(b"c"); wait(b"Clipboard ready")
            go(folder)
            send(b"v"); confirm(b"Copy item")
            assert (folder/"source").read_bytes()==b"preserve me"
            send(b"r"); wait(b"Name:"); send(b"moved\r")
            confirm(b"Rename item",final=b"Renamed")
            send(b"x"); wait(b"Clipboard ready")
            go(root)
            send(b"v"); confirm(b"Move item")
            assert (root/"moved").read_bytes()==b"preserve me"
            assert not (folder/"moved").exists()
            send(b"u"); confirm(b"Undo move",final=b"Undone")
            assert (folder/"moved").read_bytes()==b"preserve me"
            go(folder)
            send(b"d"); confirm(b"Move to Trash",final=b"Trashed")
            assert not (folder/"moved").exists()
            trash=data/"Trash"/"files"
            go(trash)
            send(b"R"); confirm(b"Restore item",final=b"Restored")
            assert (folder/"moved").read_bytes()==b"preserve me"
            go(folder)
            send(b"d"); confirm(b"Move to Trash",final=b"Trashed")
            go(trash)
            send(b"D"); confirm(b"PERMANENT DELETE",word=b"DELETE",final=b"Deleted")
            assert not list(trash.iterdir())
            assert source.read_bytes()==b"preserve me"
            assert sentinel.read_bytes()==b"outside"
            send(b"q"); assert p.wait(timeout=8)==0
            assert termios.tcgetattr(slave)==before
            assert not list(journal.glob("*.pending.json"))
            print("TT_UI_TRANSFER copy, cut, move, undo, properties, preview, trash, restore, purge passed")
        finally:
            if p and p.poll() is None: p.kill(); p.wait()
            os.close(master); os.close(slave)
if __name__=="__main__": main(str(Path(sys.argv[1]).resolve()))
