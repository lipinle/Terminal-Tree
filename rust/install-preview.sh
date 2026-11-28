#!/usr/bin/env bash
# Install a native preview side by side, without editing .bashrc or Python runtime.
set -euo pipefail
if (( EUID == 0 )); then echo "Run as normal user, not sudo." >&2; exit 1; fi
here=$(builtin cd -- "$(dirname -- "$0")" && pwd -P)
if (( $# == 3 )) && [[ "$1" == --update && -n "$2" && -n "$3" ]]; then
    # A side-by-side upgrade leaves the previous installation available.
    bash "$here/verify-preview-install.sh" "$2"
    shift 2
fi
if (( $# != 1 )) || [[ -z "$1" ]]; then
    echo "Usage: bash rust/install-preview.sh NEW_DIRECTORY" >&2
    echo "       bash rust/install-preview.sh --update OLD_DIRECTORY NEW_DIRECTORY" >&2
    exit 2
fi
target=$(realpath -m -- "$1")
if [[ -e "$target" || -L "$target" || -L "$1" ]]; then echo "Refusing existing destination." >&2; exit 1; fi
parent=$(dirname -- "$target")
[[ -d "$parent" && -O "$parent" ]] || { echo "Owned existing parent required." >&2; exit 1; }
# Use the same explicit build directory that supplies the installed binary.
# An inherited CARGO_TARGET_DIR or cargo configuration must not select a
# different artifact while this script copies a stale local executable.
cargo build --locked --release --manifest-path "$here/Cargo.toml" --target-dir "$here/target" --features native-ops --bin terminal-tree
umask 077
mkdir -- "$target"
# Never overwrite existing files. Failure leaves a new directory for inspection.
( set -o noclobber; cat -- "$here/target/release/terminal-tree" > "$target/terminal-tree" )
chmod 700 -- "$target/terminal-tree"
( set -o noclobber; cat -- "$here/integration-preview.sh" > "$target/integration.sh" )
(
    builtin cd -- "$target"
    set -o noclobber
    sha256sum -- terminal-tree integration.sh > manifest.sha256
)
bash "$here/verify-preview-install.sh" "$target"
sync -f -- "$target"
"$target/terminal-tree" --version
printf 'Preview installed. Explicit opt-in: source %q\n' "$target/integration.sh"
printf 'Existing Python installation and .bashrc unchanged.\n'
