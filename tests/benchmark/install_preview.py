#!/usr/bin/env python3
"""Install into a fresh private directory; prove no modification to a fake existing Bashrc."""
import os, subprocess, tempfile, sys
from pathlib import Path

ROOT=Path(__file__).resolve().parents[2]
with tempfile.TemporaryDirectory(prefix="tt-preview-install-") as tmp:
    base=Path(tmp); target=base/"native"
    rc=base/".bashrc"; rc.write_bytes(b"# untouched\n")
    # Installer is passed an explicit target and must not read/write HOME's shell config.
    env=dict(os.environ,HOME=str(base),CARGO_HOME=os.environ.get("CARGO_HOME",str(Path.home()/".cargo")),RUSTUP_HOME=os.environ.get("RUSTUP_HOME",str(Path.home()/".rustup")))
    subprocess.run(["bash",str(ROOT/"rust/install-preview.sh"),str(target)],check=True,env=env)
    assert rc.read_bytes()==b"# untouched\n"
    binary=target/"terminal-tree"; before=binary.read_bytes()
    assert binary.is_file() and (target/"integration.sh").is_file()
    assert subprocess.run(["bash",str(ROOT/"rust/install-preview.sh"),str(target)],env=env,capture_output=True).returncode!=0
    assert binary.read_bytes()==before
    subprocess.run([str(binary),"--version"],check=True)
    assert rc.read_bytes()==b"# untouched\n"
    print("TT_PREVIEW_INSTALL side-by-side install, existing target refusal and untouched bashrc passed")
