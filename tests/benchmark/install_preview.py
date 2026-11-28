#!/usr/bin/env python3
"""Install into a fresh private directory; prove no modification to a fake existing Bashrc."""
import os, subprocess, tempfile, sys
from pathlib import Path

ROOT=Path(__file__).resolve().parents[2]
with tempfile.TemporaryDirectory(prefix="tt-preview-install-") as tmp:
    base=Path(tmp); target=base/"native\n"
    rc=base/".bashrc"; rc.write_bytes(b"# untouched\n")
    # Installer is passed an explicit target and must not read/write HOME's shell config.
    env=dict(os.environ,HOME=str(base),CARGO_HOME=os.environ.get("CARGO_HOME",str(Path.home()/".cargo")),RUSTUP_HOME=os.environ.get("RUSTUP_HOME",str(Path.home()/".rustup")))
    # The installer deliberately selects its own build directory.
    env["CARGO_TARGET_DIR"] = str(base / "unexpected-build")
    subprocess.run(["bash",str(ROOT/"rust/install-preview.sh"),str(target)],check=True,env=env,timeout=300)
    assert not (base / "unexpected-build").exists()
    empty = subprocess.run(
        ["bash",str(ROOT/"rust/install-preview.sh"),""],
        env=env,capture_output=True,timeout=15,
    )
    assert empty.returncode == 2
    assert rc.read_bytes()==b"# untouched\n"
    binary=target/"terminal-tree"; before=binary.read_bytes()
    integration=target/"integration.sh"
    original_integration=integration.read_bytes()
    assert binary.is_file() and not binary.is_symlink()
    assert integration.is_file() and not integration.is_symlink()
    assert target.stat().st_mode & 0o777 == 0o700
    assert binary.stat().st_mode & 0o777 == 0o700
    assert integration.stat().st_mode & 0o777 == 0o600
    manifest=target/"manifest.sha256"
    assert manifest.stat().st_mode & 0o777 == 0o600
    assert {p.name for p in target.iterdir()} == {"terminal-tree", "integration.sh", "manifest.sha256"}
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
    nested_alias = subprocess.run(
        ["bash", str(ROOT/"rust/install-preview.sh"), str(alias/"new")],
        env=env, capture_output=True, timeout=15,
    )
    assert nested_alias.returncode != 0 and not (target/"new").exists()
    missing_args = subprocess.run(
        ["bash", str(ROOT/"rust/install-preview.sh"), "--update"],
        cwd=base, env=env, capture_output=True, timeout=15,
    )
    assert missing_args.returncode == 2 and not (base/"--update").exists()
    assert alias.is_symlink() and alias.readlink()==target
    assert binary.read_bytes()==before
    assert integration.read_bytes()==original_integration
    subprocess.run([str(binary),"--version"],check=True,timeout=15)
    assert rc.read_bytes()==b"# untouched\n"
    verify=ROOT/"rust/verify-preview-install.sh"
    uninstall=ROOT/"rust/uninstall-preview.sh"
    subprocess.run(["bash",str(verify),str(target)],check=True,env=env,timeout=15)
    updated=base/"native"
    subprocess.run(["bash",str(ROOT/"rust/install-preview.sh"),"--update",str(target),str(updated)],
                   check=True,env=env,timeout=300)
    assert binary.read_bytes()==before
    assert integration.read_bytes()==original_integration
    subprocess.run(["bash",str(verify),str(updated)],check=True,env=env,timeout=15)
    def refuse(*args):
        result=subprocess.run(["bash",str(uninstall),*map(str,args)],
                              capture_output=True,env=env,timeout=15)
        assert result.returncode!=0
    refuse(target)
    refuse(alias,"--yes")
    extra=target/"user-note"
    extra.write_bytes(b"keep")
    refuse(target,"--yes")
    assert extra.read_bytes()==b"keep"
    extra.unlink()
    integration.write_bytes(b"changed")
    refuse(target,"--yes")
    assert integration.read_bytes()==b"changed"
    integration.write_bytes(original_integration)
    subprocess.run(["bash",str(uninstall),str(target),"--yes"],check=True,env=env,timeout=15)
    assert target.is_dir() and not list(target.iterdir())
    assert rc.read_bytes()==b"# untouched\n"
    subprocess.run([str(updated/"terminal-tree"),"--version"],check=True,timeout=15)
    print("TT_PREVIEW_INSTALL install, side-by-side update, guarded uninstall and untouched bashrc passed")
