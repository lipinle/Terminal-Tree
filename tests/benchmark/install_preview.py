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
    integration=target/"integration.sh"
    original_integration=integration.read_bytes()
    assert binary.is_file() and not binary.is_symlink()
    assert integration.is_file() and not integration.is_symlink()
    assert target.stat().st_mode & 0o777 == 0o700
    assert binary.stat().st_mode & 0o777 == 0o700
    assert integration.stat().st_mode & 0o777 == 0o600
    assert {p.name for p in target.iterdir()} == {"terminal-tree", "integration.sh"}
    assert subprocess.run(["bash",str(ROOT/"rust/install-preview.sh"),str(target)],env=env,capture_output=True).returncode!=0
    assert binary.read_bytes()==before
    assert integration.read_bytes()==original_integration
    dangling=base/"dangling"
    absent=base/"absent"
    dangling.symlink_to(absent)
    refused=subprocess.run(
        ["bash",str(ROOT/"rust/install-preview.sh"),str(dangling)],
        env=env,capture_output=True,timeout=15,
    )
    assert refused.returncode!=0
    assert dangling.is_symlink() and dangling.readlink()==absent
    assert not absent.exists()
    alias=base/"alias"
    alias.symlink_to(target, target_is_directory=True)
    refused=subprocess.run(
        ["bash",str(ROOT/"rust/install-preview.sh"),str(alias)],
        env=env,capture_output=True,timeout=15,
    )
    assert refused.returncode!=0
    assert alias.is_symlink() and alias.readlink()==target
    assert binary.read_bytes()==before
    assert integration.read_bytes()==original_integration
    subprocess.run([str(binary),"--version"],check=True,timeout=15)
    assert rc.read_bytes()==b"# untouched\n"
    print("TT_PREVIEW_INSTALL side-by-side install, existing target refusal and untouched bashrc passed")
