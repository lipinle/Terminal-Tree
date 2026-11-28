#!/usr/bin/env bash
# Local ownership and integrity check for installations made with a manifest.
# The manifest is a local change detector, not a signature or proof of origin.
set -euo pipefail
if (( EUID == 0 )); then echo "Run as normal user, not sudo." >&2; exit 1; fi
if (( $# != 1 )) || [[ -z "$1" ]]; then
    echo "Usage: bash rust/verify-preview-install.sh INSTALL_DIRECTORY" >&2
    exit 2
fi
IFS= read -r -d '' target < <(realpath -ze -- "$1")
IFS= read -r -d '' lexical < <(realpath -zms -- "$1")
[[ "$target" == "$lexical" && ! -L "$1" && -d "$target" ]] ||
    { echo "Plain installation directory required." >&2; exit 1; }
builtin cd -- "$target"
[[ "." -ef "$target" && $(stat -c '%u:%a' -- .) == "$EUID:700" ]] ||
    { echo "Private owned installation directory required." >&2; exit 1; }
shopt -s dotglob nullglob
entries=(*)
(( ${#entries[@]} == 3 )) || { echo "Unexpected installation contents." >&2; exit 1; }
for name in terminal-tree integration.sh manifest.sha256; do
    [[ -f "$name" && ! -L "$name" ]] ||
        { echo "Missing or linked installation file." >&2; exit 1; }
    mode=600
    [[ "$name" != terminal-tree ]] || mode=700
    [[ $(stat -c '%u:%a:%h' -- "$name") == "$EUID:$mode:1" ]] ||
        { echo "Unexpected installation file ownership, mode or links." >&2; exit 1; }
done
# Compare the fixed two-file manifest literally: do not let its contents
# select arbitrary paths for sha256sum to open.
[[ $(stat -c '%s' -- manifest.sha256) == 161 ]] ||
    { echo "Unexpected installation manifest size." >&2; exit 1; }
sha256sum -- terminal-tree integration.sh | cmp -s -- manifest.sha256 - ||
    { echo "Installation contents changed." >&2; exit 1; }
