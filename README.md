# Terminal Tree — Rust development preview

Terminal Tree is a terminal file navigator with explicitly scoped native Linux file operations. The current Rust build is a preview, not a stable release or a replacement for independent backups.

## Installation and updates

On Linux, install the Rust toolchain, a C toolchain, Bash, GNU coreutils and gzip. Build as a normal user, never with sudo:

```sh
cargo build --locked --release --manifest-path rust/Cargo.toml --features native-ops --bin terminal-tree
rust/target/release/terminal-tree --version
rust/target/release/terminal-tree --edition
```

For a side-by-side installation, choose a new, nonexistent directory under an existing owned directory:

```sh
bash rust/install-preview.sh "$HOME/terminal-tree-preview"
bash rust/verify-preview-install.sh "$HOME/terminal-tree-preview"
source "$HOME/terminal-tree-preview/integration.sh"
```

The integration is optional and does not change .bashrc, cd, or the previous Python installation. Use tt_rust_nav, tt_rust_list, tt_rust_tree, tt_rust_panes and tt_rust_find in that interactive Bash session. Update by installing alongside the verified old version:

```sh
bash rust/install-preview.sh --update "$HOME/terminal-tree-preview" "$HOME/terminal-tree-preview-new"
```

Close the old preview before uninstalling it. Verification rejects unexpected files, links or modified manifests. Uninstallation removes only the three known installation files and leaves the empty directory; it does not remove user files or shell configuration.

```sh
bash rust/uninstall-preview.sh "$HOME/terminal-tree-preview" --yes
```

The local manifest detects changes; it is not a signature or proof of origin. If an old preview has no manifest, install a new directory rather than attempting to update or uninstall it using these scripts. Keep legacy Python installations and their settings intact until you have independently verified the new workflow.

## Navigation and search

Use the installed executable (examples below assume it is on your PATH), or substitute its full path.

```sh
terminal-tree --browse "$HOME"
terminal-tree --tree "$HOME"
terminal-tree --panes "$HOME" /tmp
terminal-tree --find "$HOME" report --depth 4
terminal-tree --find "$HOME" invoice --content --depth 4
```

In browse mode, arrow keys navigate, h toggles hidden entries, s cycles sorting, / filters the current directory, F7 filters entry type, F5 refreshes and F1 opens help. Enter may open an external application, which can change files. In dual panes, Tab changes focus, n opens a tab, [ and ] switch tabs, w closes a tab, f/F search recursively by filename/content, b saves a location, B opens bookmarks, J/K reorder within the bookmark menu. Esc or Ctrl+C cancels an active search. Filename search is case-insensitive; content search matches exact case-sensitive UTF-8 text. Search has explicit traversal and byte limits and can reject excessive work; it is not a complete filesystem index.

For persistent pane preferences, create a private directory and explicitly opt in:

```sh
mkdir -m 700 "$HOME/tt-pane-preferences"
terminal-tree --panes "$HOME" /tmp --preferences "$HOME/tt-pane-preferences" --theme dark
```

Press t to cycle the theme and T to save theme and navigation keys. CLI overrides saved values; use --keys 'up=down,down=up' for a simultaneous navigation-key swap. Mutation and confirmation keys cannot be remapped. Bookmark snapshots require explicit --bookmarks-out PATH and S; existing snapshot files are never overwritten. Use --bookmarks-in PATH to reload.

## Scoped file operations

Start with a disposable private workspace, not irreplaceable personal data:

```sh
demo=$(mktemp -d)
mkdir -m 700 "$demo/work" "$demo/journal" "$demo/work/data"
terminal-tree --workspace "$demo/work" "$demo/journal" "$demo/work/data"
```

Use Space to mark, c to copy, x to cut, v to review paste, r to rename, n to create a directory, t to create a file, d for Trash, R to restore and u for eligible session-local move undo. F6 opens further actions, including literal bulk rename, keep-both paste, archive extraction and retained cross-device move. Review every path and enter the exact requested confirmation. Existing destinations are refused. Batches commit individually and stop on failure; there is no automatic rollback.

Personal mode requires a real account home, an existing private journal and explicitly approved additional mount roots. It refuses protected paths and unapproved mounts. Personal Trash is limited to the existing private same-filesystem home Trash. Permanent deletion applies only to eligible Trash items and is irreversible. Do not use it as a fallback for failed Trash operations.

For a cross-device move, cut one regular file and choose the retained move from F6. Strict metadata is the default. The optional time-rounding policy permits at most two seconds of modification-time rounding, not changes to ownership, permissions, attributes or content. The verified destination is published before the source is renamed into private retention. The retained original is not automatically deleted. Record its path.

## Preview and archives

Use p/F3 for text or automatic hex, F8 for hex, I for PPM images and A for USTAR archive listing in dual panes. Native Linux archive commands support bounded regular-file USTAR and tar.gz data:

```sh
terminal-tree --archive-list archive.tar
terminal-tree --archive-layout archive.tar
terminal-tree --archive-member archive.tar item.txt --stdout > new-output.txt
terminal-tree --archive-build --gzip --stdout item.txt input.txt > new-archive.tar.gz
```

Use new output filenames; shell redirection can overwrite a file before the program runs. Use Bash noclobber if appropriate. Archive extraction in scoped mode publishes a new directory only; it cannot merge into an existing tree and does not restore archive metadata. ZIP, arbitrary image formats, recursive archive construction, and general lossy metadata conversion are not supported by these commands.

## Recovery and compatibility

If a write fails or is interrupted, open j in the same scoped navigator to inspect pending journal records. A pending record does not prove an operation failed. Preserve the journal, source, destination, staging data and any retained original. Compare independently before attempting manual recovery. Never execute a path from an untrusted journal or repeat a deletion blindly. For a retained move with both the published and retained copies present, select the published file in scoped mode, press O, enter the independently inspected original absolute path and transaction ID, then a new filename. Review the four paths and type recover. The program checks the journal structure and matching current file manifests, copies to a new name without overwrite, and preserves both files and the old journal. This is not proof of historical provenance. Missing or conflicting evidence requires independent manual recovery; do not purge retention or overwrite existing data. The journal is not a backup.

Native operations are Linux-specific and depend on actual filesystem semantics, including permissions, metadata, mount isolation and no-replace rename. Unsupported operations fail rather than silently degrade. Do not assume the preview is validated on every filesystem, network share, terminal or concurrent-writer scenario. Portable builds provide a reduced read-only navigator. No stable-release assurance is implied.

## Development checks

```sh
cargo test --locked --manifest-path rust/Cargo.toml --features native-ops
```

Resource benchmarks use synthetic fixtures, GNU time and a Linux PTY. They report raw samples, binary size, startup and navigation latency, idle/peak RSS, CPU time and filesystem I/O operation counts; these are not performance guarantees. Run only with the matching release binary and benchmark example.
