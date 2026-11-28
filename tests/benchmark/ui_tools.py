#!/usr/bin/env python3
"""Read-only recovery, location picker, filters, settings and action UI."""
import os, pty, fcntl, struct, termios, subprocess, tempfile, sys, json
from pathlib import Path
from parity import wait_output

def main(binary):
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
                return subprocess.Popen([binary,"--workspace",str(root),str(journal),str(data)],
                    stdin=slave,stdout=slave,stderr=slave,env=dict(os.environ,TERM="xterm-256color",HOME=str(root)))
            def wait(marker):wait_output(master,p,marker)
            def send(keys):os.write(master,keys)
            p=launch();wait(b"Rust / workspace")
            send(b"j");wait(b"Recovery - pending journal")
            send(b"q");wait(b"Pending records: 1")
            assert record.read_bytes()==original
            send(b"m");wait(b"My Computer")
            send(b"\x1b");wait(b"Rust / workspace")
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
            send(b"\x1b");wait(b"Rust / workspace")
            send(b"q");assert p.wait(timeout=8)==0
            assert termios.tcgetattr(slave)==before
            p=launch();wait(b"kind Directories")
            send(b"q");assert p.wait(timeout=8)==0
            assert record.read_bytes()==original and (root/"file").read_bytes()==b"unchanged"
            assert not (root/"unused").exists()
            print("TT_UI_TOOLS recovery read-only, My Computer, filters, settings persistence and action menu passed")
        finally:
            if p and p.poll() is None:p.kill();p.wait()
            os.close(master);os.close(slave)
def retained_recovery(binary):
    import shutil
    with tempfile.TemporaryDirectory(prefix="tt-recovery-pty-") as tmp:
        base = Path(tmp)
        root, journal = base / "work", base / "journal"
        root.mkdir(mode=0o700)
        journal.mkdir(mode=0o700)
        data = root / ".data"
        data.mkdir(mode=0o700)
        txid = "0" * 32 + "-" + "1" * 8 + "-" + "2" * 16
        stage = root / (".terminal-tree-copy-" + txid)
        stage.mkdir(mode=0o700)
        retained = stage / "original"
        retained.write_bytes(b"retained evidence")
        published = root / "published"
        shutil.copy2(retained, published)
        original = root / "original-name"
        record = journal / (txid + ".done.json")
        payload = {"id": txid, "started": txid, "operation": "retained-move"}
        for key, path in (("source", original), ("destination", published), ("stage", retained)):
            payload[key] = str(path)
            payload[key + "_bytes_hex"] = os.fsencode(path).hex()
        record.write_text(json.dumps(payload))
        record.chmod(0o600)
        before_record = record.read_bytes()
        master, slave = pty.openpty()
        process = None
        try:
            fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 28, 160, 0, 0))
            before = termios.tcgetattr(slave)
            process = subprocess.Popen([binary, "--workspace", str(root), str(journal), str(data)],
                stdin=slave, stdout=slave, stderr=slave,
                env=dict(os.environ, TERM="xterm-256color", HOME=str(root)))
            wait_output(master, process, b"published")
            def step(keys, marker):
                os.write(master, keys)
                wait_output(master, process, marker)
            step(b"O", b"Original absolute path")
            step(os.fsencode(original) + b"\r", b"Retained move transaction ID:")
            step(txid.encode() + b"\r", b"Name:")
            step(b"recovered\r", b"Recover verified retained copy")
            step(b"\r", b"Type recover:")
            step(b"recover\r", b"Recovery copy created")
            os.write(master, b"q")
            assert process.wait(timeout=8) == 0
            assert termios.tcgetattr(slave) == before
            assert record.read_bytes() == before_record
            assert not original.exists()
            assert all(p.read_bytes() == b"retained evidence" for p in
                       (published, retained, root / "recovered"))
            print("TT_RETAINED_RECOVERY_PTY explicit recovery and preservation passed")
        finally:
            if process is not None and process.poll() is None:
                process.kill()
                process.wait()
            os.close(master)
            os.close(slave)


if __name__=="__main__":
    binary = str(Path(sys.argv[1]).resolve())
    main(binary)
    retained_recovery(binary)
