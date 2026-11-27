#!/usr/bin/env bash
# CI-only disposable mounts; never run on existing user paths.
set -euo pipefail
[[ "${GITHUB_ACTIONS:-}" == true && "${RUNNER_OS:-}" == Linux ]]
home=$(mktemp -d /tmp/tt-personal-fixture.XXXXXXXX)
ext=$(sudo mktemp -d /mnt/tt-personal-fixture.XXXXXXXX)
cleanup(){
    sudo umount "$ext/nested" 2>/dev/null || true
    sudo umount "$ext" 2>/dev/null || true
    sudo rmdir "$ext" 2>/dev/null || true
    rm -rf -- "$home"
}
trap cleanup EXIT
sudo mount -t tmpfs -o "size=8M,mode=0700,uid=$(id -u),gid=$(id -g),nosuid,nodev,noexec" tmpfs "$ext"
mkdir "$ext/nested" "$home/bind-source"
sudo mount --bind "$home/bind-source" "$ext/nested"
TT_TEST_HOME="$home" TT_TEST_EXTERNAL_ROOT="$ext" cargo test --locked --manifest-path rust/Cargo.toml --features native-ops --test mounted -- --ignored --nocapture
