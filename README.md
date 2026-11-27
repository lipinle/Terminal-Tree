# Terminal Tree

A lightweight interactive file explorer integrated into Bash. Keep your normal shell, see a directory tree after changing directories, and press Ctrl+Down to browse and manage files.

**Version 2.1.0 — consolidated development baseline (beta).**

## Requirements

- Linux, Bash, Python 3.9+ with curses, GNU coreutils, and a compatible terminal.
- Optional: eza for icons and colors (Nerd Font recommended), findmnt for device shortcuts, gio for Trash, xdg-open for opening files.

## Install or upgrade

Clone this repository with GitHub access (currently private), then run as your normal user:

```bash
git clone https://github.com/lipinle/Terminal-Tree.git
cd Terminal-Tree
bash install.sh
exec bash
```

Existing managed installations can upgrade with `git pull --ff-only` followed by `bash install.sh`. Close active navigators first. The installer backs up .bashrc and existing runtime files and recognizes both archived legacy Bash versions. It refuses unknown conflicting definitions. No sudo required.

Uninstall with `bash uninstall.sh` and restart Bash. Prior managed shell code is restored; settings and backup files remain.

## Usage

- `lsx` displays the tree; `lsx -a` includes hidden files.
- `cd` and Ctrl+L redraw the tree.
- Ctrl+Down opens the navigator; F1 shows all controls.
- Arrow keys navigate; Enter opens a file without leaving the navigator, or changes directory and returns to Bash for a folder. Esc / q exits.
- Mouse wheel moves one entry per event; left click opens, right click selects.

## Safety and limitations

Copy uses private staging and atomic no-overwrite publication; move refuses cross-filesystem operations. Destructive actions check file identities, protect system locations and mount points, and require review. Permanent deletion is restricted to top-level home Trash entries. Undo is session-only for moves and renames. These are safeguards, not a backup system or a guarantee against data loss. Full crash recovery, checksums, and recursive search are not implemented. Real eza, gio and desktop integration need further testing.

## Development

`src/tree.py` is the consolidated editable runtime. `shell/integration.sh` connects it to Bash. `archive/` preserves the original Bash versions and all historical installers and patches; do not run them over 2.1.0.

```bash
python3 -m unittest discover -s tests -v
python3 -m py_compile src/tree.py install.py
bash -n install.sh
```

See [CHANGELOG.md](CHANGELOG.md) and [ROADMAP.md](ROADMAP.md). The existing repository license is retained.
