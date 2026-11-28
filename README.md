<div align="center">

# Terminal Tree

**A desktop-inspired file manager, right inside your terminal.**

Browse files, organize folders, inspect documents and work with Git — without leaving your shell.

[![Stable](https://img.shields.io/badge/Stable-3.0.0-238636?style=flat-square)](https://github.com/lipinle/Terminal-Tree/releases/tag/v3.0.0)
[![Rust](https://img.shields.io/badge/Rust-000000?style=flat-square&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![Linux](https://img.shields.io/badge/Linux-native-FCC624?style=flat-square&logo=linux&logoColor=black)](#system-requirements)
[![License](https://img.shields.io/badge/License-MIT-2563eb?style=flat-square)](LICENSE)

[Download](#install) · [Features](#features) · [Compatibility](#terminal-compatibility) · [Add-ons](#add-ons) · [Shortcuts](#shortcuts)

</div>

> **Choose your version:** [Standard 3.0.0](https://github.com/lipinle/Terminal-Tree/releases/tag/v3.0.0) is the stable Rust release. This development branch contains the experimental UX preview described below. New UI features and graphics add-ons are not part of the stable release archive.

## Features

| Explore | Work | Personalize |
| :--- | :--- | :--- |
| Tree, List and Grid views | Dual-pane file management | Dark and light themes |
| Clickable sidebar and bookmarks | Scoped copy, move and rename | File-type icons and colors |
| Tabs and clickable breadcrumbs | File and folder creation | Resizable panels and layout zoom |
| Text, Markdown, image and PDF preview | Git manager and archive inspection | Custom navigation shortcuts |
| Search and command palette | Built-in Bash panel | Saved preferences |

Image/PDF previews require optional helpers. The embedded interactive Bash panel is experimental; full-screen terminal programs are not yet supported.

## System requirements

| Operating system | Availability |
| :--- | :--- |
| Linux (Ubuntu 22.04 / 24.04) | Native file operations; stable source builds tested |
| Linux Mint / Debian-based distributions | Build from source; compatibility depends on system libraries |
| Other Linux distributions | Source build; not individually certified |
| Windows | Reduced read-only build; stable Windows CI tested |
| macOS | Not runtime-validated; no supported release package yet |

The stable prebuilt archive is Linux x86-64 and targets Ubuntu 24.04-compatible system libraries. The source build requires Rust, a C toolchain and GNU/Linux utilities. The optional visual helper requires Rust 1.92 or newer. The latest UX preview has development CI coverage, not a full release compatibility certification.

## Terminal compatibility

Terminal Tree shares the same file manager UI across terminals. Graphics quality depends on the terminal's capabilities.

| Terminal | Live image/PDF | Expanded image/PDF |
| :--- | :--- | :--- |
| GNOME Terminal (X11) | X11 RGB¹ | X11 RGB¹ |
| Kitty / Ghostty | Native RGB² | Native RGB² |
| WezTerm / Konsole | Unicode or Kitty RGB² | Kitty RGB or Sixel² |
| foot / mlterm | Unicode | Sixel² |
| xterm | Unicode | Sixel² |
| Alacritty / XFCE Terminal | Unicode | Unicode |
| tmux / screen / other terminals | Unicode | Unicode |

¹ Experimental X11 helper; use a dedicated single-tab GNOME Terminal on X11. If placement fails, the program uses Unicode.

² Requires a compatible terminal version and protocol. Kitty and Ghostty are automatically recognized; foot and mlterm use Sixel in expanded previews. Other terminals may require manual selection. Individual terminal integrations still need real-world validation.

If graphics do not work, start with `TT_GRAPHICS=unicode`. For compatible terminals, `TT_GRAPHICS=kitty` or `TT_GRAPHICS=sixel` selects the protocol explicitly. Native graphics are optional; browsing does not depend on them.

## Install

### Stable release (Linux x86-64)

1. Download `terminal-tree-3.0.0-linux-x86_64.tar.gz` and `SHA256SUMS` from [Standard 3.0.0](https://github.com/lipinle/Terminal-Tree/releases/tag/v3.0.0).
2. In the download directory, verify and extract:

~~~bash
sha256sum -c SHA256SUMS
mkdir "$HOME/terminal-tree-3.0.0"
tar -xzf terminal-tree-3.0.0-linux-x86_64.tar.gz \
  -C "$HOME/terminal-tree-3.0.0"
~~~

3. Open the extracted directory and run the included executable:

~~~bash
./terminal-tree --version
./terminal-tree --browse "$HOME"
~~~

If the binary cannot run on your distribution, build from source instead. The downloaded archive is not a package-manager installation.

### UX preview (Linux, build from source)

Install [Rust](https://www.rust-lang.org/tools/install) first. On Ubuntu or Linux Mint:

~~~bash
sudo apt install git build-essential coreutils gzip
git clone --branch ux-companion-tools \
  https://github.com/lipinle/Terminal-Tree.git terminal-tree-source
cd terminal-tree-source
bash rust/install-preview.sh "$HOME/terminal-tree-preview"
~~~

Open the application:

~~~bash
"$HOME/terminal-tree-preview/terminal-tree" --app "$HOME"
~~~

The installer uses a new directory and does not replace existing Terminal Tree installations or change your shell configuration. Run the installer as your normal user.

For optional Bash navigation commands, run:

~~~bash
source "$HOME/terminal-tree-preview/integration.sh"
tt_rust_nav
~~~

## Update and uninstall

### Update the UX preview

From your source checkout:

~~~bash
git pull --ff-only
bash rust/install-preview.sh --update \
  "$HOME/terminal-tree-preview" \
  "$HOME/terminal-tree-preview-new"
~~~

The update installs alongside the old version. Try the new executable before removing the previous installation. Use a fresh destination name each time. Rebuild any optional helpers from the same checkout if you use them.

For stable archives, download and verify the next release and extract into a new directory rather than overwriting the old version.

### Uninstall

Close Terminal Tree first. From the source checkout:

~~~bash
bash rust/uninstall-preview.sh "$HOME/terminal-tree-preview" --yes
~~~

The script removes only the verified, managed installation files. It leaves the empty directory, your files and preferences untouched. Substitute the exact installation directory you want to remove. For a downloaded archive, remove only the dedicated extracted application directory using your file manager; do not use the source-install uninstaller for it.

## Getting started

The UX preview opens in Tree view. Use the toolbar to switch views, click folders in Places, and select a file to preview it. Drag panel dividers to resize them. Open **Settings** to customize the layout and save preferences.

### File operations

The default application is read-only. To try copy, move, rename and other scoped file actions in an empty test workspace:

~~~bash
demo=$(mktemp -d)
mkdir -m 700 "$demo/work" "$demo/journal" "$demo/work/data"
"$HOME/terminal-tree-preview/terminal-tree" \
  --app-workspace "$demo/work" "$demo/journal" "$demo/work/data"
~~~

Review the displayed paths before confirming an operation. The journal is not a backup. Bash and Git commands use ordinary OS permissions outside the file-operation journal. Start with disposable files and keep independent backups.

### Shortcuts

| Shortcut | Action |
| :--- | :--- |
| Arrow keys / Enter | Navigate / open |
| Ctrl+1 / 2 / 3 | Grid / List / Tree |
| Ctrl+L / Ctrl+F | Location / filter |
| Ctrl+T / Ctrl+W | New / close tab |
| F8 / Tab | Dual panes / switch focus |
| Ctrl+C / X / V | Copy / cut / paste in writable mode |
| F9 / Alt+P / Alt+V | Toggle preview / change mode / expand visual |
| Alt+T / Alt+H | Focus / show or hide terminal panel |
| Alt+S | Start or close persistent Bash session |
| Alt+G / Ctrl+P | Git manager / companion tools |
| Ctrl+K / Ctrl+, | Command palette / settings |
| F1 / q | Help / exit |

Terminal shortcuts may take priority over application shortcuts. Use the on-screen menus when this occurs.

## Add-ons

Add-ons are optional. Terminal Tree works without them and does not install external software automatically.

| Add-on | What it does |
| :--- | :--- |
| eza | Detailed directory listing |
| 7-Zip | Inspect archive contents |
| Git | Status, diffs, history, staging, local commits and branches |
| Visual Preview | Display PNG, JPEG, WebP, GIF, BMP and PDF |
| X11 Overlay | High-resolution visual previews in GNOME Terminal on X11 |

Install the external companion packages on Ubuntu or Linux Mint:

~~~bash
sudo apt install eza 7zip git
~~~

Older distributions may provide `p7zip-full` instead of `7zip`. Package availability varies.

Open **Add-ons**, enable the integrations you want, then use **Tools** or Ctrl+P. For Git, open **Git** or press Alt+G in a trusted repository. The local Git manager does not provide remote push/pull or branch switching.

### Image and PDF previews (experimental)

Build the helper from your source checkout:

~~~bash
cargo +stable build --release --manifest-path visual-preview/Cargo.toml
sudo install -m 755 visual-preview/target/release/tt-visual-preview \
  /usr/local/bin/tt-visual-preview
~~~

Enable **Visual Preview** in Add-ons. Select an image or PDF; use the mouse wheel to turn PDF pages and Alt+V to expand. GIF previews are still images.

**GNOME Terminal on X11:** optionally install the additional overlay helper:

~~~bash
cargo +stable build --release --manifest-path x11-overlay/Cargo.toml
sudo install -m 755 x11-overlay/target/release/tt-x11-overlay \
  /usr/local/bin/tt-x11-overlay
~~~

Open a dedicated GNOME Terminal window, then launch:

~~~bash
TT_GRAPHICS=x11 "$HOME/terminal-tree-preview/terminal-tree" --app "$HOME"
~~~

Enable Visual Preview if needed. If calibration is unavailable, use Unicode or open the original file from expanded preview with O.

The two visual helpers are source-only experiments and are not bundled with the stable archive or UX preview artifact. To update, rebuild and reinstall the helper you previously installed. To uninstall manually installed helpers, remove only their exact files under `/usr/local/bin` after confirming they belong to this installation.

See [Visual Preview setup](visual-preview/README.md), [X11 Overlay setup](x11-overlay/README.md), and [companion tools](OPTIONAL-TOOLS.md) for more information.

## Help and troubleshooting

| Problem | What to try |
| :--- | :--- |
| Image shows as text or blocks | Install and enable Visual Preview; check graphics backend |
| Icons are missing | Select Unicode or ASCII in Settings → Icons |
| Panels are hidden | Enlarge the terminal, or check Settings → Display |
| Copy or rename is unavailable | Open an explicitly scoped workspace |
| Settings are not saved | Settings → Save application preferences |
| A file operation was interrupted | Preserve the journal and all copies; inspect before recovery |

Run the executable with `--help` to see the commands available in your build. Terminal Tree never promises universal filesystem compatibility or substitutes for backups.

---

<div align="center">

[Releases](https://github.com/lipinle/Terminal-Tree/releases) · [License](LICENSE) · [Icon notices](ICON-NOTICES.md) · [Optional tool licenses](OPTIONAL-TOOLS.md)

MIT-licensed Terminal Tree. Optional external programs retain their own licenses.

</div>
