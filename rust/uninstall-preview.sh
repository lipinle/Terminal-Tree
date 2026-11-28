#!/usr/bin/env bash
# Remove only the exact verified preview files. Leave the now-empty directory
# in place; never recursively delete, edit shell startup or remove user data.
set -euo pipefail
if (( EUID == 0 )); then echo "Run as normal user, not sudo." >&2; exit 1; fi
if (( $# != 2 )) || [[ -z "$1" || "$2" != --yes ]]; then
    echo "Usage: bash rust/uninstall-preview.sh INSTALL_DIRECTORY --yes" >&2
    exit 2
fi
IFS= read -r -d '' script_path < <(realpath -ze -- "$0")
here=${script_path%/*}
IFS= read -r -d '' target < <(realpath -ze -- "$1")
IFS= read -r -d '' lexical < <(realpath -zms -- "$1")
[[ "$target" == "$lexical" && ! -L "$1" ]] ||
    { echo "Plain installation directory required." >&2; exit 1; }
builtin cd -- "$target"
# Validate the pinned working directory and its current pathname before
# removing exactly these three names. No guarantee against a concurrent
# process running as the same user; close the preview before uninstalling.
bash "$here/verify-preview-install.sh" "$target"
[[ $(stat -c '%d:%i' -- .) == "$(stat -c '%d:%i' -- "$target")" ]] ||
    { echo "Installation directory replaced." >&2; exit 1; }
command rm -- terminal-tree integration.sh manifest.sha256
sync -f -- .
printf 'Preview removed. Empty installation directory retained: %q\n' "$target"
printf 'Shell configuration and user data unchanged. Open a fresh shell.\n'
