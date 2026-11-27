#!/usr/bin/env bash
# Install a native preview side by side, without editing .bashrc or Python runtime.
set -euo pipefail
if (( EUID == 0 )); then echo "Run as normal user, not sudo." >&2; exit 1; fi
if (( $# != 1 )); then echo "Usage: bash rust/install-preview.sh NEW_DIRECTORY" >&2; exit 2; fi
here=$(builtin cd -- "$(dirname -- "$0")" && pwd -P)
target=$(realpath -m -- "$1")
if [[ -e "$target" || -L "$target" || -L "$1" ]]; then echo "Refusing existing destination." >&2; exit 1; fi
parent=$(dirname -- "$target")
[[ -d "$parent" && -O "$parent" ]] || { echo "Owned existing parent required." >&2; exit 1; }
cargo build --locked --release --manifest-path "$here/Cargo.toml" --features native-ops --bin terminal-tree
umask 077
mkdir -- "$target"
# Never overwrite existing files. Failure leaves a new directory for inspection.
( set -o noclobber; cat -- "$here/target/release/terminal-tree" > "$target/terminal-tree" )
chmod 700 -- "$target/terminal-tree"
( set -o noclobber; cat -- "$here/integration-preview.sh" > "$target/integration.sh" )
sync -f -- "$target"
"$target/terminal-tree" --version
printf 'Preview installed. Explicit opt-in: source %q\n' "$target/integration.sh"
printf 'Existing Python installation and .bashrc unchanged.\n'
