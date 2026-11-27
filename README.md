# Terminal Tree

A keyboard-first terminal file manager for Linux, written in Rust, with optional Bash integration.

**Development preview — not a stable release.** The native implementation is currently version 0.1.0. The original Python 2.1.1 implementation is preserved in [legacy/python](legacy/python/). The goal for the next stable product release is Standard 3.0.0.

## Features

- Interactive directory navigation, history, direct path entry, sorting, filtering, mouse selection and file properties.
- Text previews, multi-selection and batch file operations.
- Copy, same-filesystem move, rename, create files/directories, Trash, restore and session-local undo for eligible moves and renames.
- Reviewed write operations, no-overwrite safeguards, independent content verification for copies, and a local transaction journal with a read-only recovery inspector.
- Optional Bash integration and eza listing. The native engine does not depend on eza output for file operations.

These are capabilities of the current main branch, not a list of planned features. The tree-style listing currently displays the selected directory; expandable multi-level trees, recursive search, bookmarks, tabs, dual panes and archive tools are planned.

## Requirements

- Linux for native file operations, Bash integration and preview installation.
- Rust and Cargo; the manifest declares Rust 1.75. The exact supported minimum toolchain still needs release verification.
- A terminal with ANSI and keyboard support. Mouse modifiers depend on the terminal.
- GLib/GIO (`gio`) for workspace Trash; `xdg-open` or GIO for external file opening. eza is optional.
- No root privileges. Use disposable files for early evaluation.

## Quick start

Build from source (no system installation):

```sh
git clone https://github.com/lipinle/Terminal-Tree.git
cd Terminal-Tree
cargo build --locked --release --manifest-path rust/Cargo.toml --features native-ops --bin terminal-tree
./rust/target/release/terminal-tree --version
./rust/target/release/terminal-tree --browse .
```

Read-only browsing does not write through Terminal Tree, but an external application opened from it can modify files.

To try write operations on disposable data only:

```bash
demo=$(mktemp -d)
mkdir -m 700 "$demo/workspace" "$demo/journal" "$demo/workspace/.data"
printf 'example\n' > "$demo/workspace/example.txt"
./rust/target/release/terminal-tree --workspace "$demo/workspace" "$demo/journal" "$demo/workspace/.data"
```

Keep the demo directory if a transaction is unresolved. The journal is not a backup.

### Optional side-by-side installation

From the repository root, in Bash:

```bash
bash rust/install-preview.sh "$HOME/terminal-tree-preview"
source "$HOME/terminal-tree-preview/integration.sh"
tt_rust_nav
```

The destination must not already exist. This installer does not modify .bashrc or replace the legacy installation. Source the integration explicitly in each new interactive Bash session. A production installer, updater and uninstaller are not yet available.

## Key bindings

| Key | Action |
|---|---|
| Arrow keys, Enter | Navigate / open |
| [ / ] | Back / forward |
| g / m (F4) | Go to path / locations |
| h / s / / | Hidden files / sort / current-directory name filter |
| F7 / F5 | Entry-type filter / refresh |
| i / p (F3) | Properties / text preview |
| Space / a / A | Mark / mark visible / clear marks |
| c / x / v | Copy / cut / paste |
| r (F2) / n / t | Rename / new directory / new empty file |
| d / R / D | Trash / restore / permanent deletion |
| u / j | Session undo / inspect pending transactions |
| F6 / F1 | Actions / help |
| q / Esc | Exit or cancel, depending on context |

Write commands require workspace or personal mode and explicit review. Cross-filesystem moves, overwriting existing destinations and automatic recovery are not implemented on main.

## Project direction

Standard is intended to become a complete daily-driver file manager first. Advanced organizational security is deferred to a separately built Paranoid edition, sharing the same core. Neither a stable Standard release nor a separate Paranoid binary is available yet. See [Roadmap](ROADMAP.md).

## Compatibility and limitations

Linux is the primary target. Existing CI exercises Ubuntu 22.04/24.04 and disposable ext4, XFS and Btrfs filesystems; exFAT coverage currently verifies strict metadata refusal rather than general write compatibility. Windows has a read-only portability test; native write support is not implemented. macOS has no verified CI result. Cross-filesystem moves are refused, strict metadata checks may reject external drives, and recovery is currently inspection-only.

Testing reduces risk but does not certify safety on every filesystem or hardware configuration. Keep independent backups, do not use this preview as your sole file manager for irreplaceable data, and read [Security](SECURITY.md).

## Development

See [Contributing](CONTRIBUTING.md) for build and test commands, and [Roadmap](ROADMAP.md) for priorities. Check [Actions](https://github.com/lipinle/Terminal-Tree/actions) for the exact commit you intend to use. Pending pull requests are not shipped features.

## License

[MIT](LICENSE). The archived Python implementation retains its original license and files.
