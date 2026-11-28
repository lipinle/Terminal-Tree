# Opt-in preview integration. Source explicitly; never modifies existing cd/clear.
[[ $- == *i* ]] || return 0
IFS= read -r -d '' _tt_rust_source < <(realpath -ze -- "${BASH_SOURCE[0]}") || return 1
_tt_rust_program="${_tt_rust_source%/*}/terminal-tree"
unset _tt_rust_source
tt_rust_list() {
    "$_tt_rust_program" --list "$@"
}
tt_rust_nav() {
    local tmp dest rc
    [[ -t 0 && -t 1 ]] || return 1
    tmp=$(mktemp -d) || return 1
    if ! (umask 077; set -o noclobber; : > "$tmp/result"); then
        command rmdir -- "$tmp"
        return 1
    fi
    # An error must reach cleanup even when the caller enabled errexit.
    if "$_tt_rust_program" --result "$tmp/result"; then
        rc=0
    else
        rc=$?
    fi
    if (( rc == 0 )) && [[ -f "$tmp/result" && ! -L "$tmp/result" ]] && IFS= read -r -d '' dest < "$tmp/result"; then
        [[ "$dest" == /* ]] && builtin cd -- "$dest" || rc=1
    fi
    command rm -f -- "$tmp/result"
    command rmdir -- "$tmp"
    return "$rc"
}
# Read-only commands use the same installed binary and preserve argument boundaries.
tt_rust_tree() {
    "$_tt_rust_program" --tree "$@"
}
tt_rust_panes() {
    "$_tt_rust_program" --panes "$@"
}
tt_rust_find() {
    "$_tt_rust_program" --find "$@"
}
tt_rust_workspace() {
    "$_tt_rust_program" --workspace "$@"
}

tt_rust_personal() {
    "$_tt_rust_program" --personal "$@"
}

# Optional external listing. eza owns its formatting/configuration; the
# native file engine does not consume or trust its output.
tt_rust_ls() {
    if (( $# > 1 )); then
        printf 'Usage: tt_rust_ls [PATH]\n' >&2
        return 2
    fi
    local path="${1:-.}"
    if command -v eza >/dev/null 2>&1; then
        command eza -- "$path"
    else
        # Absolute pathname prevents an argument from becoming a list option.
        [[ "$path" == /* ]] || path="$PWD/$path"
        "$_tt_rust_program" --list "$path"
    fi
}
