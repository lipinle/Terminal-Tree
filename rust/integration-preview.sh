# Opt-in preview integration. Source explicitly; never modifies existing cd/clear.
[[ $- == *i* ]] || return 0
_tt_rust_dir=$(builtin cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
_tt_rust_program="$_tt_rust_dir/terminal-tree"
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
    "$_tt_rust_program" --result "$tmp/result"
    rc=$?
    if (( rc == 0 )) && [[ -f "$tmp/result" && ! -L "$tmp/result" ]] && IFS= read -r -d '' dest < "$tmp/result"; then
        [[ "$dest" == /* ]] && builtin cd -- "$dest" || rc=1
    fi
    command rm -f -- "$tmp/result"
    command rmdir -- "$tmp"
    return "$rc"
}
tt_rust_workspace() {
    "$_tt_rust_program" --workspace "$@"
}

tt_rust_personal() {
    "$_tt_rust_program" --personal "$@"
}
