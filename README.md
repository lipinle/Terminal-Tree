# Terminal Tree — Standard 3.0.0

Terminal Tree is a terminal file navigator with explicitly scoped native Linux file operations. Standard 3.0.0 is the first stable Rust release for the documented supported scope. It is not a replacement for independent backups. Paranoid remains a separate preview build.

To start browsing after building or installing, run `terminal-tree --browse .`. Use the arrow keys to navigate, F1 for help and q to exit. Run `terminal-tree --help` for the commands available in your build.

## Installation and updates

The Linux x86-64 release archive contains a native Standard executable, Bash integration, this guide and the license. Download it and SHA256SUMS from the matching GitHub release, verify with `sha256sum -c SHA256SUMS`, and extract into a fresh owned directory. Run `./terminal-tree --edition` and `./terminal-tree --version` from the extracted directory before use. Ubuntu 22.04 and 24.04 source builds are tested; the hosted prebuilt binary targets the Ubuntu 24.04 runner and may require a compatible glibc. The extracted archive is not a manifest-managed source installation.

To build from source on Linux, install the Rust toolchain, a C toolchain, Bash, GNU coreutils and gzip. Build as a normal user, never with sudo:

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

The integration is optional and does not change .bashrc, cd, or the previous Python installation. Use tt_rust_nav, tt_rust_list, tt_rust_tree, tt_rust_panes and tt_rust_find in that interactive Bash session. In tt_rust_nav, Right enters a folder while browsing, Enter selects a folder for Bash, and q, Esc or Ctrl+C exits without selecting. Update by installing alongside the verified old version:

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

## Companion tools

On native Linux, Ctrl+P opens the same Tools menu in browse, workspace and dual-pane modes. Install the distribution packages for eza, 7-Zip (7zz or 7z) and Git; the program checks fixed /usr/bin locations. The companions are optional, independently installed programs. Terminal Tree neither bundles nor downloads their code or executables. See [optional tools and licenses](OPTIONAL-TOOLS.md). Missing tools do not prevent native browsing or file operations.

```sh
terminal-tree --tools
terminal-tree --eza .
terminal-tree --7zip-list example.7z
terminal-tree --git-status .
```

eza is a display-only detailed listing. 7-Zip lists selected regular archives, never extracts or changes them. Git status is explicit and intended only for trusted repositories: local configuration and attributes may execute helpers. No automatic Git commands, staging, commit, pull or push. Companion output is bounded, escaped display text, never a file-operation plan. The existing native engine remains responsible for filesystem mutations. These companions are separate programs, not sandboxed code; GNU timeout limits foreground execution. Their respective upstream licenses remain applicable to the separately installed programs.

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

Native operations are Linux-specific and depend on actual filesystem semantics, including permissions, metadata, mount isolation and no-replace rename. Unsupported operations fail rather than silently degrade. Native regression coverage includes Ubuntu 22.04/24.04, ext4, XFS, Btrfs, exFAT and an isolated size-limited mount; this does not mean every operation is supported on every filesystem. Windows is tested in reduced read-only mode. macOS runtime validation is deferred. Do not assume support for network shares, every terminal or hostile concurrent writers. The virtual power-cut matrix covers copy, move, Trash, restore and purge; the newer archive extraction, retained move and retained recovery paths have targeted interruption and conflict tests, not exhaustive power-loss coverage. Retained moves intentionally preserve the original in private retention and do not perform automatic cleanup. Use an independent backup and start in a disposable workspace.

## Development checks

```sh
cargo test --locked --manifest-path rust/Cargo.toml --features native-ops
```

Resource benchmarks use synthetic fixtures, GNU time and a Linux PTY. They report raw samples, binary size, startup and navigation latency, idle/peak RSS, CPU time and filesystem I/O operation counts; these are not performance guarantees. Run only with the matching release binary and benchmark example.
