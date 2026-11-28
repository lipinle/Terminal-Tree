#!/usr/bin/env bash
# Capture the actual TUI under Xvfb with synthetic data only.
set -euo pipefail
binary=$(realpath -e -- "$1")
output=$(realpath -m -- "$2")
fixture=$(mktemp -d /tmp/terminal-tree-demo.XXXXXX)
pid=""
cleanup() {
    if [[ -n "$pid" ]]; then kill "$pid" 2>/dev/null || true; wait "$pid" 2>/dev/null || true; fi
    rm -rf -- "$fixture"
}
trap cleanup EXIT
home="$fixture/home"
project="$home/Documents/Terminal-Tree"
mkdir -p "$project"/{assets,icons,src} "$home"/{Desktop,Downloads,Pictures,Music,Videos}
printf '# Terminal Tree\n\nA desktop-inspired file manager.\n\n## Explore\n\n- Tree, List and Grid views\n- Tabs and bookmarks\n- Markdown preview\n- Built-in Bash panel\n\n## Customize\n\n- Themes and file-type icons\n- Resizable panels\n' > "$project/README.md"
printf '[package]\nname = "demo"\n' > "$project/Cargo.toml"
printf 'fn main() {}\n' > "$project/src/main.rs"
xterm -geometry 150x36+0+0 -fa 'DejaVu Sans Mono' -fs 11 -title 'Terminal Tree UX Preview' \
  -e env HOME="$home" XDG_CONFIG_HOME="$home/.config" TT_APP_CONFIG_DIR="$home/.config/terminal-tree" \
  TT_GRAPHICS=unicode TT_ICON_STYLE=unicode "$binary" --app "$project" &
pid=$!
id=$(timeout 8s xdotool search --sync --onlyvisible --name '^Terminal Tree UX Preview$' | head -n 1)
sleep 1
xdotool key --window "$id" End
sleep 1
kill -0 "$pid"
import -window "$id" "$output"
test -s "$output"
