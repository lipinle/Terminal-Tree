#!/usr/bin/env bash
# Only on disposable hosted Linux runners. Never format an existing device.
set -euo pipefail
[[ "${GITHUB_ACTIONS:-}" == true && "${RUNNER_OS:-}" == Linux ]]
kind="$1"
case "$kind" in ext4|xfs|btrfs|exfat) ;; *) exit 2;; esac
base=$(mktemp -d /tmp/tt-fs-compat.XXXXXXXX)
ext=$(sudo mktemp -d /mnt/tt-fs-compat.XXXXXXXX)
loop=""
mounted=0
cleanup(){
    if (( mounted )); then sudo umount "$ext" || return; fi
    if [[ -n "$loop" ]]; then sudo losetup -d "$loop" || return; fi
    sudo rmdir "$ext"
    rm -rf -- "$base"
}
trap cleanup EXIT
mkdir -m 700 "$base/home"
# Newly created exclusive image, no device name supplied by caller.
( set -o noclobber; : > "$base/disposable.img" )
truncate -s 512M "$base/disposable.img"
loop=$(sudo losetup --find --show "$base/disposable.img")
case "$kind" in
    ext4) sudo mkfs.ext4 -q "$loop";;
    xfs) sudo mkfs.xfs -q "$loop";;
    btrfs) sudo mkfs.btrfs -q "$loop";;
    exfat) sudo mkfs.exfat "$loop";;
esac
if [[ "$kind" == exfat ]]; then
    sudo mount -t exfat -o "uid=$(id -u),gid=$(id -g),fmask=0077,dmask=0077,nosuid,nodev,noexec" "$loop" "$ext"
else
    sudo mount -t "$kind" -o nosuid,nodev,noexec "$loop" "$ext"
fi
mounted=1
if [[ "$kind" != exfat ]]; then
    sudo chown "$(id -u):$(id -g)" "$ext"
    chmod 700 "$ext"
fi
TT_TEST_HOME="$base/home" TT_TEST_EXTERNAL_ROOT="$ext" TT_TEST_FS="$kind" \
    cargo test --locked --manifest-path rust/Cargo.toml --features native-ops --test filesystem_compat -- --ignored --nocapture
