"""Guest-only test harness, run on a disposable ext4 virtual disk."""
import hashlib
import json
import os
from pathlib import Path
import signal
import stat
import subprocess
import sys
import traceback
import urllib.parse

sys.path.insert(0, "/tt")
import tree

ROOT = Path("/mnt/data")
WORK = ROOT / "work"
SOURCE = WORK / "source.bin"
DEST = WORK / "destination.bin"
SENTINEL = WORK / "unrelated.txt"
DATA = bytes(range(256)) * 16384
EXPECTED = hashlib.sha256(DATA).hexdigest()
CASES = (
    "copy-partial", "copy-before-publish", "copy-before-sync",
    "copy-after-sync", "copy-done",
    "move-before-sync", "move-after-sync", "move-done",
)
PERSONAL_CASES = (
    "trash-journal",
    "trash-info-synced",
    "trash-before-sync",
    "trash-after-sync",
    "trash-done",
    "restore-journal",
    "restore-before-sync",
    "restore-after-sync",
    "restore-info-before-sync",
    "restore-info-after-sync",
    "restore-done",
    "purge-journal",
    "purge-entry-before-sync",
    "purge-entry-after-sync",
    "purge-data-synced",
    "purge-info-before-sync",
    "purge-info-after-sync",
    "purge-done",
)
TRASH = WORK / ".local/share/Trash"
JOURNAL = ROOT / "state/terminal-tree/operations"


def require(condition, message):
    if not condition:
        raise AssertionError(message)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def cut(case):
    # Host kills the QEMU process as soon as it observes this serial marker.
    # Never invoke sync or shutdown here.
    print("TT_CUT_READY " + case, flush=True)
    while True:
        signal.pause()


def tree_manifest(path):
    manifest, links = tree.content_manifest(str(path))
    return json.dumps([manifest, links], sort_keys=True, ensure_ascii=True,
                      default=lambda x: {"bytes": x.hex()} if isinstance(x, bytes) else str(x))


def prepare(engine="python", fixture="file"):
    WORK.mkdir()
    (ROOT / "home").mkdir()
    if fixture == "tree":
        SOURCE.mkdir()
        (SOURCE / "nested").mkdir()
        (SOURCE / "nested" / "empty").mkdir()
        (SOURCE / "large.bin").write_bytes(DATA)
        os.link(SOURCE / "large.bin", SOURCE / "nested" / "hard-link")
        (SOURCE / "nested" / "relative-link").symlink_to("../large.bin")
        (SOURCE / "external-link").symlink_to("../unrelated.txt")
        (SOURCE / "dangling-link").symlink_to("missing")
        raw = os.fsencode(SOURCE) + b"/raw-\xff"
        with open(raw, "wb") as f:
            f.write(b"raw path")
        os.chmod(SOURCE / "large.bin", 0o640)
        os.chmod(SOURCE / "nested", 0o750)
        os.setxattr(SOURCE / "large.bin", "user.tt-test", b"file-attr")
        os.setxattr(SOURCE / "nested", "user.tt-test", b"dir-attr")
        os.setxattr(SOURCE, "user.tt-test", b"root-attr")
        for p in [*SOURCE.rglob("*"), SOURCE]:
            os.utime(p, ns=(1700000000123456789, 1700000000123456789), follow_symlinks=False)
        (ROOT / "baseline.json").write_text(tree_manifest(SOURCE), encoding="ascii")
    else:
        SOURCE.write_bytes(DATA)
    if engine == "rust" and fixture == "file":
        os.chmod(SOURCE, 0o640)
        os.setxattr(SOURCE, "user.tt-test", b"preserve-xattr")
        os.utime(SOURCE, ns=(1700000000123456789, 1700000000123456789))
    SENTINEL.write_text("do not modify", encoding="utf-8")
    # Make the baseline durable before the tested operation. The journal is
    # intentionally absent so the first-use directory creation is exercised.
    os.sync()


def execute(case):
    prepare()
    original_rename = tree.rename_noreplace
    original_fsync = os.fsync
    original_run = tree.subprocess.run

    def interrupted_copy(args, **kwargs):
        require(args[0] == "cp", "unexpected subprocess")
        # Deterministic partial payload, not a claim of interrupting GNU cp.
        with open(args[-1], "wb") as f:
            f.write(DATA[:len(DATA)//2])
            f.flush()
        cut(case)

    def interrupted_rename(*args, **kwargs):
        if case == "copy-before-publish":
            cut(case)
        result = original_rename(*args, **kwargs)
        cut(case)
        return result

    def interrupted_sync(fd):
        # renameat2 has already succeeded; its directory fsync has not run.
        if DEST.exists():
            cut(case)
        return original_fsync(fd)

    if case == "copy-partial":
        tree.subprocess.run = interrupted_copy
    elif case in ("copy-before-publish", "copy-after-sync", "move-after-sync"):
        tree.rename_noreplace = interrupted_rename
    elif case.endswith("-before-sync"):
        tree.os.fsync = interrupted_sync
    try:
        op = tree.copy_item if case.startswith("copy") else tree.move_item
        op(str(SOURCE), str(DEST), tree.identity(SOURCE))
    finally:
        tree.subprocess.run = original_run
        tree.rename_noreplace = original_rename
        tree.os.fsync = original_fsync
    require(case.endswith("-done"), "cut point was not reached")
    cut(case)


def verify(case, engine="python", fixture="file"):
    require(SENTINEL.read_text(encoding="utf-8") == "do not modify",
            "unrelated data changed")
    src, dst = SOURCE.exists(), DEST.exists()
    if fixture == "tree":
        baseline = (ROOT / "baseline.json").read_text(encoding="ascii")
        for p in (SOURCE, DEST):
            if p.exists():
                require(tree_manifest(p) == baseline, "published tree differs from durable baseline")
                require(os.path.samefile(p / "large.bin", p / "nested" / "hard-link"), "hard-link topology lost")
                require((p / "external-link").is_symlink() and os.readlink(p / "external-link") == "../unrelated.txt", "external link changed")
                require((p / "dangling-link").is_symlink(), "dangling link lost")
    else:
        if src:
            require(digest(SOURCE) == EXPECTED, "source checksum mismatch")
        if dst:
            require(digest(DEST) == EXPECTED, "published destination checksum mismatch")
    if case.startswith("copy"):
        require(src, "copy lost its source")
        if case in ("copy-partial", "copy-before-publish"):
            require(not dst, "unverified destination published")
        if case in ("copy-after-sync", "copy-done"):
            require(dst, "synced destination was lost")
    else:
        require(src != dst, "move must retain exactly one intact name")
        if case in ("move-after-sync", "move-done"):
            require(dst, "synced move was lost")
    if engine == "rust" and fixture == "file":
        for p in (SOURCE, DEST):
            if p.exists():
                st = p.stat()
                require(stat.S_IMODE(st.st_mode) == 0o640, "mode mismatch")
                require(st.st_uid == 0 and st.st_gid == 0, "ownership mismatch")
                require(st.st_mtime_ns == 1700000000123456789, "mtime mismatch")
                require(os.getxattr(p, "user.tt-test") == b"preserve-xattr", "xattr mismatch")
    records = tree.recovery_records()
    directory = Path(tree.state_directory())
    done = list(directory.glob("*.done.json"))
    if case.endswith("-done"):
        require(not records and len(done) == 1, "completed journal missing or pending")
        record = json.loads(done[0].read_text())
        require(not list(WORK.glob(".terminal-tree-copy-*")), "completed copy left staging")
    else:
        require(len(records) == 1 and not done, "pending journal missing or ambiguous")
        record = records[0]
    require(record.get("source") == str(SOURCE), "wrong source in journal")
    require(record.get("destination") == str(DEST), "wrong destination in journal")
    require(record.get("operation") == case.split("-")[0], "wrong journal operation")
    print("TT_PASS " + case + " source=" + str(src) + " destination=" + str(dst)
          + " pending=" + str(len(records)), flush=True)



def personal_entries(path):
    """Exact surviving non-directory data and metadata, without following links."""
    result = {}
    for p in [path, *path.rglob("*")]:
        s = p.lstat()
        record = {
            "mode": s.st_mode, "uid": s.st_uid, "gid": s.st_gid,
            "attrs": {k: os.getxattr(p, k, follow_symlinks=False).hex()
                      for k in os.listxattr(p, follow_symlinks=False)},
        }
        if not stat.S_ISDIR(s.st_mode):
            record["mtime"] = s.st_mtime_ns
            if stat.S_ISLNK(s.st_mode):
                record["target"] = os.readlink(p)
            else:
                require(stat.S_ISREG(s.st_mode), "unexpected special file")
                record["hash"] = digest(p)
        result[str(p.relative_to(path))] = record
    return result


def prepare_personal(case, fixture):
    prepare("rust", fixture)
    # Purge intentionally refuses hard-linked payloads. Use independent files
    # for its recursive fixture, keeping the rest of the tree unchanged.
    if fixture == "tree" and case.startswith("purge-"):
        link = SOURCE / "nested/hard-link"
        link.unlink()
        link.write_bytes(DATA)
        os.chmod(link, 0o640)
    for p in (TRASH, TRASH / "files", TRASH / "info", JOURNAL):
        p.mkdir(parents=True, exist_ok=True)
        p.chmod(0o700)
    # The actual production personal policy must run as a normal user.
    for p in [*WORK.rglob("*"), WORK, JOURNAL, JOURNAL.parent, JOURNAL.parent.parent]:
        os.chown(p, 1000, 1000, follow_symlinks=False)
    if fixture == "tree":
        for p in [*SOURCE.rglob("*"), SOURCE]:
            os.utime(p, ns=(1700000000123456789, 1700000000123456789), follow_symlinks=False)
        (ROOT / "baseline.json").write_text(tree_manifest(SOURCE), encoding="ascii")
    (ROOT / "personal-baseline.json").write_text(
        json.dumps(personal_entries(SOURCE), ensure_ascii=True), encoding="ascii")
    if not case.startswith("trash-"):
        SOURCE.rename(TRASH / "files/source.bin")
        info = TRASH / "info/source.bin.trashinfo"
        info.write_text("[Trash Info]\nPath=" + urllib.parse.quote(str(SOURCE), safe="/")
                        + "\nDeletionDate=2026-01-01T00:00:00\n", encoding="ascii")
        info.chmod(0o600)
        os.chown(info, 1000, 1000)
    # Additional Trash sentinel: purge must not touch other Trash entries.
    (TRASH / "files/unrelated").write_text("trash sentinel", encoding="ascii")
    (TRASH / "info/unrelated.trashinfo").write_text("sentinel metadata", encoding="ascii")
    os.sync()


def verify_personal(case, fixture):
    require(SENTINEL.read_text() == "do not modify", "unrelated home data changed")
    require((TRASH / "files/unrelated").read_text() == "trash sentinel", "unrelated Trash data changed")
    require((TRASH / "info/unrelated.trashinfo").read_text() == "sentinel metadata", "unrelated Trash metadata changed")
    op = ("native-trash" if case.startswith("trash-") else
          "restore" if case.startswith("restore-") else "permanent-delete")
    pending = list(JOURNAL.glob("*.pending.json"))
    done = list(JOURNAL.glob("*.done.json"))
    completed = case.endswith("-done")
    require(len(pending) == (0 if completed else 1) and len(done) == (1 if completed else 0),
            "incorrect durable journal state")
    record = json.loads((done if completed else pending)[0].read_text())
    require(record["operation"] == op, "incorrect journal operation")
    item = Path(record["destination"]) if op == "native-trash" else TRASH / "files/source.bin"
    info = Path(record["stage"])
    require(item.parent == TRASH / "files" and info == TRASH / "info" / (item.name + ".trashinfo"),
            "journal paths outside expected Trash")
    require(record["source"] == str(SOURCE if op == "native-trash" else item), "wrong source")
    require(record["destination"] == (str(item) if op == "native-trash" else str(SOURCE) if op == "restore" else None), "wrong destination")
    src, dst, meta = SOURCE.exists(), item.exists(), info.exists()
    require(set((TRASH / "files").iterdir()) <= {item, TRASH / "files/unrelated"}, "unexpected Trash entries")
    require(set((TRASH / "info").iterdir()) <= {info, TRASH / "info/unrelated.trashinfo"}, "unexpected Trash metadata")
    if meta:
        raw = info.read_text(encoding="ascii").splitlines()
        paths = [x[5:] for x in raw if x.startswith("Path=")]
        require(raw[0] == "[Trash Info]" and len(paths) == 1 and
                urllib.parse.unquote_to_bytes(paths[0]) == os.fsencode(SOURCE), "invalid surviving Trash metadata")
    baseline = json.loads((ROOT / "personal-baseline.json").read_text(encoding="ascii"))
    if op != "permanent-delete":
        require(src != dst, "exactly one intact payload required")
        payload = SOURCE if src else item
        require(personal_entries(payload) == baseline, "payload or metadata changed")
        if fixture == "tree":
            require(tree_manifest(payload) == (ROOT / "baseline.json").read_text(encoding="ascii"),
                    "full tree or hard-link topology changed")
        if op == "native-trash":
            require(not dst or meta, "trashed payload has no restore metadata")
            if case == "trash-journal":
                require(src and not meta, "mutation before metadata publication")
            if case == "trash-info-synced":
                require(src and meta, "synced metadata missing")
            if case in ("trash-after-sync", "trash-done"):
                require(dst and meta, "synced Trash data missing")
        else:
            require(not dst or meta, "unrestored payload has no restore metadata")
            if case == "restore-journal":
                require(dst and meta, "restore changed before rename")
            if case in ("restore-after-sync", "restore-info-before-sync",
                        "restore-info-after-sync", "restore-done"):
                require(src, "synced restored data missing")
            if case in ("restore-journal", "restore-before-sync", "restore-after-sync"):
                require(meta, "metadata removed before data sync")
            if case in ("restore-info-after-sync", "restore-done"):
                require(not meta, "synced metadata removal lost")
    else:
        require(not src, "purge recreated original")
        if dst:
            survivors = personal_entries(item)
            require(all(k in baseline and v == baseline[k] for k, v in survivors.items()),
                    "unexpected or corrupted surviving purge entries")
            require(meta, "partial purge lost metadata")
            if case == "purge-journal":
                require(survivors == baseline, "purge changed before journal checkpoint")
            if case == "purge-entry-after-sync":
                require(len(survivors) < len(baseline), "synced entry removal lost")
        if case == "purge-journal":
            require(dst and meta, "purge changed before deletion")
        if case in ("purge-data-synced", "purge-info-before-sync",
                    "purge-info-after-sync", "purge-done"):
            require(not dst, "synced purge resurrected data")
        if case in ("purge-info-after-sync", "purge-done"):
            require(not meta, "synced metadata removal lost")
        if case in ("purge-journal", "purge-entry-before-sync",
                    "purge-entry-after-sync", "purge-data-synced"):
            require(meta, "purge metadata removed before data sync")
    print(f"TT_PASS {case} source={src} trash={dst} info={meta} pending={len(pending)}", flush=True)


def main():
    args = dict(token.split("=", 1) for token in Path("/proc/cmdline").read_text().split()
                if "=" in token)
    case = args.get("tt_case")
    require(case in CASES + PERSONAL_CASES, "invalid case")
    engine = args.get("tt_engine", "python")
    require(engine in ("python", "rust"), "invalid engine")
    fixture = args.get("tt_fixture", "file")
    require(fixture == "file" or (engine == "rust" and fixture == "tree"), "invalid fixture")
    if case in PERSONAL_CASES:
        require(engine == "rust", "personal cases require native engine")
        if args.get("tt_phase") == "execute":
            prepare_personal(case, fixture)
            env = dict(os.environ, HOME=str(WORK), XDG_DATA_HOME=str(TRASH.parent))
            subprocess.run(["/tt/tt-crash-probe", case, fixture], env=env,
                           user=1000, group=1000, extra_groups=[], check=True)
            raise RuntimeError("native personal probe unexpectedly returned")
        if args.get("tt_phase") == "verify":
            verify_personal(case, fixture)
            return
        raise ValueError("invalid phase")
    if args.get("tt_phase") == "execute":
        if engine == "rust":
            prepare(engine, fixture)
            subprocess.run(["/tt/tt-crash-probe", case, fixture], check=True)
            raise RuntimeError("native probe unexpectedly returned")
        execute(case)
    elif args.get("tt_phase") == "verify":
        verify(case, engine, fixture)
    else:
        raise ValueError("invalid phase")


if __name__ == "__main__":
    try:
        main()
    except BaseException:
        traceback.print_exc()
        print("TT_FAIL guest assertion or runtime error", flush=True)
        raise
