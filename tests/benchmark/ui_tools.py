#!/usr/bin/env python3
"""Read-only recovery, location picker, filters, settings and action UI."""
import os, pty, fcntl, struct, termios, subprocess, tempfile, sys, json
from pathlib import Path
from parity import wait_output

def main(binary, mode):
    marker = b"Rust / " + mode.encode("ascii") + b"]"
    flags = ["--paranoid"] if mode == "Paranoid" else []
    with tempfile.TemporaryDirectory(prefix="tt-ui-tools-") as tmp:
        base=Path(tmp);root=base/"work";root.mkdir(mode=0o700)
        journal=base/"journal";journal.mkdir(mode=0o700)
        data=root/".data";data.mkdir(mode=0o700)
        (root/"folder").mkdir()
        (root/"file").write_bytes(b"unchanged")
        record=journal/"fixture.pending.json"
        record.write_text(json.dumps({"id":"fixture","operation":"copy","source_bytes_hex":os.fsencode(root/"file").hex(),"destination":str(root/"unused"),"stage":str(root/"stage")}))
        record.chmod(0o600);original=record.read_bytes()
        master,slave=pty.openpty();p=None
        try:
            fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack("HHHH",28,160,0,0))
            before=termios.tcgetattr(slave)
            def launch():
                return subprocess.Popen([binary,*flags,"--workspace",str(root),str(journal),str(data)],
                    stdin=slave,stdout=slave,stderr=slave,env=dict(os.environ,TERM="xterm-256color",HOME=str(root)))
            def wait(marker):wait_output(master,p,marker)
            def send(keys):os.write(master,keys)
            p=launch();wait(marker)
            send(b"j");wait(b"Recovery - pending journal")
            send(b"q");wait(b"Pending records: 1")
            assert record.read_bytes()==original
            send(b"m");wait(b"My Computer")
            send(b"\x1b");wait(marker)
            send(b"\x1b[18~");wait(b"Filter entry type") # F7
            send(b"\x1b[B");wait(b"Filter entry type")
            send(b"\r");wait(b"kind Directories")
            send(b"s");wait(b"sort Size")
            send(b"S");wait(b"Rust view settings saved")
            settings=journal/"rust-view-settings.json"
            v=json.loads(settings.read_text())
            assert v=={"version":1,"hidden":False,"order":"Size","kind":"Directories"}
            assert settings.stat().st_mode&0o077==0
            send(b"\x1b[17~");wait(b"Actions - writes require review") # F6
            send(b"\x1b");wait(marker)
            send(b"q");assert p.wait(timeout=8)==0
            assert termios.tcgetattr(slave)==before
            p=launch();wait(b"kind Directories")
            send(b"q");assert p.wait(timeout=8)==0
            assert record.read_bytes()==original and (root/"file").read_bytes()==b"unchanged"
            assert not (root/"unused").exists()
            print(f"TT_UI_TOOLS {mode}: recovery read-only, My Computer, filters, settings persistence and action menu passed")
        finally:
            if p and p.poll() is None:p.kill();p.wait()
            os.close(master);os.close(slave)
if __name__=="__main__":
    binary = str(Path(sys.argv[1]).resolve())
    for mode in ("Standard", "Paranoid"):
        main(binary, mode)
