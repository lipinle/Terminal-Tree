#!/usr/bin/env python3
"""Boot a fresh ext4 disk for each case, kill QEMU, then replay and verify."""
import argparse
import os
from pathlib import Path
import selectors
import subprocess
import tempfile
import time

CASES = (
    "copy-partial", "copy-before-publish", "copy-before-sync",
    "copy-after-sync", "copy-done",
    "move-before-sync", "move-after-sync", "move-done",
)


def boot(kernel, initrd, disk, case, phase, log, timeout, engine="python"):
    expected = ("TT_CUT_READY " if phase == "execute" else "TT_PASS ") + case
    args = [
        "qemu-system-x86_64", "-machine", "q35", "-accel", "tcg",
        "-cpu", "max", "-m", "768", "-smp", "2",
        "-nodefaults", "-no-reboot", "-display", "none",
        "-serial", "stdio", "-monitor", "none", "-net", "none",
        "-kernel", str(kernel), "-initrd", str(initrd),
        "-append", f"console=ttyS0 quiet rdinit=/tt/init panic=1 tt_case={case} tt_phase={phase} tt_engine={engine}",
        "-drive", f"file={disk},format=raw,if=none,id=data,cache=none,aio=threads",
        "-device", "virtio-blk-pci,drive=data",
    ]
    with log.open("wb") as transcript, subprocess.Popen(
            args, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT) as vm, selectors.DefaultSelector() as selector:
        selector.register(vm.stdout, selectors.EVENT_READ)
        deadline = time.monotonic() + timeout
        pending = b""
        try:
            while time.monotonic() < deadline:
                for key, _ in selector.select(max(0, deadline-time.monotonic())):
                    chunk = os.read(key.fileobj.fileno(), 65536)
                    if not chunk:
                        raise RuntimeError(f"VM exited before {expected}")
                    transcript.write(chunk)
                    transcript.flush()
                    pending += chunk
                    while b"\n" in pending:
                        line, pending = pending.split(b"\n", 1)
                        line = line.decode("utf-8", errors="replace").strip()
                        if line.startswith("TT_FAIL"):
                            raise RuntimeError(line)
                        if line == expected or line.startswith(expected + " "):
                            # SIGKILL is directed only at the QEMU child process.
                            vm.kill()
                            vm.wait(timeout=15)
                            print(line, flush=True)
                            return
            raise TimeoutError(f"VM did not reach {expected}")
        finally:
            if vm.poll() is None:
                vm.kill()
                vm.wait(timeout=15)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--kernel", required=True, type=Path)
    parser.add_argument("--initrd", required=True, type=Path)
    parser.add_argument("--logs", required=True, type=Path)
    parser.add_argument("--timeout", type=int, default=120)
    parser.add_argument("--engine", choices=("python", "rust"), default="python")
    parser.add_argument("--case", choices=CASES, action="append")
    args = parser.parse_args()
    args.logs.mkdir(parents=True, exist_ok=True)
    cases = args.case or CASES
    for case in cases:
        with tempfile.TemporaryDirectory(prefix="tt-crash-") as temp:
            disk = Path(temp) / "disposable.raw"
            with disk.open("xb") as f:
                f.truncate(256 * 1024 * 1024)
            subprocess.run(["mkfs.ext4", "-F", "-q", "-E",
                            "lazy_itable_init=0,lazy_journal_init=0", str(disk)], check=True)
            for phase in ("execute", "verify"):
                log = args.logs / f"{case}-{phase}.log"
                try:
                    boot(args.kernel.resolve(), args.initrd.resolve(), disk, case,
                         phase, log, args.timeout, args.engine)
                except BaseException:
                    if log.exists():
                        print(log.read_bytes()[-24000:].decode(errors="replace"), flush=True)
                    raise
    print(f"VM crash/reboot checks passed: {len(cases)}/{len(cases)} (engine={args.engine})", flush=True)


if __name__ == "__main__":
    main()
