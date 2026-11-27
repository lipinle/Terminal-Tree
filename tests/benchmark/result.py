#!/usr/bin/env python3
"""Verify descriptor-backed result channel and shell preview installation."""
import os, pty, fcntl, struct, termios, subprocess, tempfile, sys
from pathlib import Path
from parity import wait_output

def main(binary):
    with tempfile.TemporaryDirectory(prefix="tt-result-") as tmp:
        base=Path(tmp); root=base/"root"; root.mkdir()
        child=root/"child"; child.mkdir()
        resultdir=base/"channel"; resultdir.mkdir(mode=0o700)
        result=resultdir/"result"; result.touch(mode=0o600)
        master,slave=pty.openpty()
        p=None
        try:
            fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack("HHHH",24,120,0,0))
            before=termios.tcgetattr(slave)
            p=subprocess.Popen([binary,"--result",str(result)],cwd=root,stdin=slave,stdout=slave,stderr=slave,env=dict(os.environ,TERM="xterm-256color"))
            wait_output(master,p,b"Rust / read-only")
            os.write(master,b"\r")
            assert p.wait(timeout=8)==0
            assert result.read_bytes()==os.fsencode(child.resolve())+b"\0"
            assert termios.tcgetattr(slave)==before
        finally:
            if p and p.poll() is None: p.kill(); p.wait()
            os.close(master); os.close(slave)
        sentinel=base/"sentinel"; sentinel.write_bytes(b"untouched")
        link=resultdir/"link"; link.symlink_to(sentinel)
        assert subprocess.run([binary,"--result",str(link)],capture_output=True).returncode!=0
        assert sentinel.read_bytes()==b"untouched"
        assert subprocess.run([binary,"--result",str(result)],capture_output=True).returncode!=0
        print("TT_RESULT_CHANNEL selection, terminal restoration, symlink and nonempty rejection passed")
if __name__=="__main__": main(str(Path(sys.argv[1]).resolve()))
