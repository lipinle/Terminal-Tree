# Terminal Tree shell integration (interactive Bash only).
[[ $- == *i* ]] || return 0
_tree_program="${XDG_DATA_HOME:-$HOME/.local/share}/terminal-tree/tree.py"
show_my_computer() {
    [[ -t 1 ]] || return 0
    python3 "$_tree_program" --list "$@"
}
lsx() { show_my_computer "$@"; }
terminal_tree_recovery() { python3 "$_tree_program" --recovery; }
_tree_nav() {
    local tmp dest rc
    [[ -t 0 && -t 1 ]] || return 1
    tmp=$(mktemp) || return 1
    python3 "$_tree_program" --result "$tmp"
    rc=$?
    if (( rc == 0 )) && IFS= read -r -d '' dest < "$tmp"; then
        if [[ "$dest" != "$PWD" ]]; then builtin cd -- "$dest" || rc=$?; fi
    fi
    command rm -f -- "$tmp"
    show_my_computer
    return "$rc"
}
cd() {
    builtin cd "$@" || return
    if [[ -t 1 ]]; then show_my_computer; fi
    return 0
}
clear() {
    command clear "$@" || return
    show_my_computer
}
bind -x '"\C-l": "clear"'
bind -x '"\e[1;5B": "_tree_nav"'
show_my_computer
