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

## Application view

Run `terminal-tree` or `terminal-tree --app PATH` to open the responsive application. Wide terminals display Places, Files and a bounded live preview; smaller terminals hide the preview or sidebar. Click a menu title or press F10, Tab switches Places/Files focus, Enter opens a selected item, and F9 toggles live preview. Press b to bookmark the current directory for this session, B to select a bookmark, Ctrl+L to go to a path (native Linux), Ctrl+F to filter and Ctrl+P for optional tools. Mouse clicks select file rows without opening files. F5 refreshes; preview does not follow links. The existing --browse, --tree and --panes views remain available.

For writes, explicitly start `terminal-tree --app-workspace ROOT JOURNAL DATA` or `terminal-tree --app-personal JOURNAL [MOUNT ...]`. File and Edit menus use the existing reviewed operations and clipboard; F4 opens the existing action menu. The sidebar does not grant write authorization. Task status is idle while browsing; operations still show their existing progress dialog. Bookmarks are session-local until application preferences are saved.

The application supports Ctrl+C/X/V (copy/cut/paste), Ctrl+A (mark all), Ctrl+Z (eligible undo), Ctrl+Q (exit), Alt+Left/Right (history), Backspace (parent), F12 (clipboard details) and a right-click context menu. Ctrl+C still cancels prompts and workers and exits the read-only view. In scoped application mode only, creating a new item, renaming one item and copying one item use a complete path review followed by Enter instead of typing yes. Move, Trash, batch and recovery confirmations remain unchanged, as do the underlying identity and no-overwrite checks. Legacy CLI views retain their previous behavior.

Press F8 in the application to switch between the three-column view and two independent file panes. Each pane retains its directory, selection, filter and navigation history. Tab changes the active pane (which is refreshed before use); a click on the inactive pane focuses it first. Copy or cut in one pane, switch to the destination, then paste with the shared clipboard. Scoped writes use the same existing review and operation engine. Marks are cleared when changing panes; the clipboard survives. Narrow terminals show the active pane only. F9 returns to the preview layout; Shift+F8 opens the existing hex preview. Read-only mode never grants write authorization.

The application starts in Tree view. Use the clickable Path, Search, Grid, List and Tree buttons in the upper-right toolbar (Ctrl+1/2/3 also switch views). Path toggles an editable location field; Ctrl+A clears it and Enter navigates. Search filters the current directory. Grid shows built-in colored file-type icons in cells; List shows names and sizes; Tree shows branches, and Right or a click on the branch expands the existing bounded read-only tree. Enter on an expanded row opens its directory or selects a file in its freshly loaded containing directory, before any operation can run. Esc closes expanded inspection. Ctrl+mouse wheel adjusts the application's row spacing/grid cell size from 1 to 3; Ctrl+0 resets. This is layout zoom, not terminal font resizing. The toolbar and these view modes apply to the single application pane; F8 retains the separate dual-pane workflow.

### File icons

Icons and colors are built into the application (Grid, List, Tree, Sidebar and Dual Pane). The program classifies native paths by filename and extension, never by running eza or reading file content. F11 or Settings > Icon style switches between Unicode (default), Seti and ASCII. For a preferred startup style, use `TT_ICON_STYLE=seti terminal-tree` or `TT_ICON_STYLE=ascii terminal-tree`. Seti mode expects a compatible Nerd Font Mono selected in the terminal; the application cannot detect the active font reliably. Switch to Unicode or ASCII if glyphs are missing or misaligned. No font or third-party artwork is distributed. See [icon attribution and compatibility](ICON-NOTICES.md).

### Settings Center and Add-ons

Click Settings or press Ctrl+, to open grouped Appearance, Icons, Display, Add-ons, Save and Reset options. Appearance controls text/background/accent/selection colors, dark/light presets and custom RGB; actual terminal font family and point size are managed by the terminal emulator. Display toggles sidebar, preview, toolbar, icons, type colors, hidden files, extra status details, view and layout zoom. Safety status is always shown.

Import a local, regular, non-link JSON icon mapping in Icons (native Linux), e.g. `{"rs":"R","zip":"Z"}`. Maximum 4 KiB, 48 lowercase extension keys, visible one- or two-cell glyphs only. No code, fonts or SVGs are installed; use only icons you have permission to use. ASCII mode ignores custom glyphs. Remove mappings to restore built-in icons.

Add-ons lists installed/enabled/idle status for eza, 7-Zip and Git. Toggle each integration with Enter. The application Tools menu refuses disabled integrations; explicitly invoked CLI and legacy views are independent. Foreground execution displays the tool name and records the last result. There are no background companion jobs or services to terminate.

For persistence on native Linux, create a private directory with `mkdir -m 700 ~/.config/terminal-tree` (if its parent already exists), or provide an existing owned mode-0700 directory through `TT_APP_CONFIG_DIR`. Settings > Save application preferences writes a separate bounded settings file atomically; it does not change shell settings or safety policy. Without an approved directory, preferences are session-only. Save can explicitly create the final private configuration directory under an existing owned, non-group/world-writable parent; it does not create missing ancestors or change existing permissions. Application preferences now include hidden-file visibility, sort order and lossless native bookmark paths. Settings > Manage bookmarks supports reordering and removal. Once saved, enable Autosave on normal exit to persist subsequent changes; no background writes or automatic recovery after a crash. Ctrl+K opens a searchable Command Palette using the same menu actions; write actions are excluded in read-only mode. Existing Ctrl+P remains the companion tools menu.

### Tabs, panel sizing and shortcuts

Ctrl+T opens a tab at the current location, Ctrl+W closes the active tab (the last tab stays open), and Ctrl+Tab / Ctrl+Shift+Tab switch tabs where the terminal forwards these keys. Click a tab or [+] on the tab strip. Up to eight tabs preserve independent locations, filters, history and dual-pane models in the session; inactive tabs refresh before selection. Clipboard remains shared, and marks clear on tab changes. Tabs are not restored after exiting.

Drag the vertical dividers to resize Sidebar, Preview or Dual Pane. Sizes are bounded and saved with application preferences. Settings > Display resets panel sizes. Settings > Navigation shortcuts lets you swap nine navigation bindings, without remapping file mutations, confirmation keys or cancellation. Menu actions use canonical shortcuts independently of these physical navigation bindings.

### Bottom terminal panel (native Linux)

The application displays a Bash command panel at the bottom by default when the terminal has at least 16 rows. Click the prompt or press Alt+T to focus it, type a command and press Enter. Esc returns focus to the file manager. Alt+H or the [Hide] button toggles visibility; View and Settings > Display also expose this option. The setting is saved with application preferences. A Terminal button in the top bar also toggles it when the window is wide enough; a drag handle [Resize] on the right of the panel heading adjusts its height from 20% to 60%, subject to minimum browser space. The configured height persists with preferences.

Commands run explicitly in the active file pane's current directory, with your ordinary OS permissions; the file manager's read-only/scoped label does **not** restrict shell commands. No command runs just by browsing. Output is escaped, bounded to 32 KiB, and displayed with exit status; Up/Down recall session-only command history, PageUp/PageDown or wheel scroll output, Ctrl+C stops the current process group, Ctrl+L clears output. A command is limited to 60 seconds. This is a noninteractive Bash command console, not a full PTY emulator: stdin is closed, commands such as vim, top, password prompts and interactive shells require a normal terminal; each command starts a new Bash process, so cd/export do not persist between commands. Commands that intentionally detach from the process group may outlive the console. File rows refresh after completion. Legacy CLI views remain unchanged.

### Optional visual preview (experimental)

The independent MIT-licensed helper in [visual-preview](visual-preview/README.md) decodes images and renders PDF pages using Rust libraries. Enable it explicitly in Add-ons after installing the helper. Truecolor half-blocks display RGB pixels in the preview panel; scroll over the panel to turn PDF pages. When disabled, existing text/hex preview remains unchanged. No changes to the core dependency lock or minimum Rust version. Development-only until the helper build and dependency license audit are complete.

### Preview controls and detailed list

Click the Preview heading or press Alt+P to cycle Auto (text or optional visual), Info (observed file metadata), and Hex. Scroll over text preview to inspect more lines, or over a visual PDF preview to change pages. List view uses readable binary size units and a Type column when space permits; Settings > Display can hide the Type column. Visual rendering remains opt-in.

### Git Manager (native Linux)

Click [Git] in the top bar, press Alt+G, or search Git Manager in the command palette. The Git add-on must be enabled and installed. Explicitly trust the repository before Git reads its configuration. Review changes, staged and unstaged diffs, local history, branches and the next commit summary. Enable local editing separately for each manager session to stage/unstage a selected file, create a local commit from all staged files, or create a branch without switching. Each write requires review. The manager does not run push, pull, fetch, checkout, reset or force commands. Git editing is separate from the file-operation journal, is not undoable there, and can affect files outside scoped roots via Git configuration; only ordinary repositories without ongoing Git operations or conflicts are eligible. Hooks, global configuration and commit signing are disabled by the bounded companion runner; configure local identity and use normal Git for workflows requiring hooks/signing. No automatic Git calls while browsing.

### Clickable navigation and Markdown preview

Click a fully visible breadcrumb segment to navigate to its directory. In List view, click Name, Size or Type in the header to select the existing sort order. The active sort column has an indicator. Markdown Auto preview formats headings, lists, quotes and code fences, wraps to panel width, and never opens embedded links or images. The status bar reports when the embedded Bash console is running.

### Terminal line editing and disk status

The bottom command console retains bounded session scrollback across commands (up to 128 KiB and 2048 lines). Left/Right, Home/End, Backspace/Delete and Ctrl+A/E/U edit the command without retyping it. Ctrl+L clears scrollback. The status bar reports available filesystem space for the active directory on native Linux, cached for two seconds. The console remains noninteractive Bash, not a persistent PTY shell.

### Experimental persistent Bash PTY

Alt+S starts or closes an explicitly approved interactive Bash session inside the terminal panel. The shell preserves working directory, variables and job control; it starts from the active file directory but subsequently navigates independently. Alt+T switches focus without terminating it, and Alt+H hides it. Ctrl+C and Ctrl+D reach the shell when focused. The original command console remains the default. Linux PTY allocation uses rustix and system setsid. The display is a bounded, escaped TERM=dumb transcript, not a VT100 emulator: full-screen editors and curses programs are not supported yet. Use F5 to refresh the file list after interactive shell changes. Shell commands run with ordinary OS permissions, outside the file-operation journal; detached descendants may survive closure.

### Dashboard redraw and visual preview troubleshooting

The dashboard buffers frames and paints complete rows instead of clearing the whole display on Linux. Unchanged idle frames and passive mouse movement do not repaint the screen. Supported terminals also receive synchronized-update boundaries. Image and PDF Auto previews explain whether the optional helper is missing or disabled rather than silently displaying hex. Add-ons includes installation instructions; the helper is not bundled in the UX preview artifact. See [helper setup](visual-preview/README.md).

### Add-ons and expanded visual preview

The native Add-ons screen separates the four companion states from their descriptions, highlights the selected entry, and offers installation help with H. Click a row or use arrows and Enter to toggle; Esc closes. The dashboard compares rendered rows for idle and ordinary single-pane arrow navigation; other actions still repaint when necessary. Alt+V opens an expanded image/PDF preview, arrows or wheel change PDF pages, and O explicitly offers to open the original through the desktop file association (ordinary OS permissions, not a sandbox). The expanded preview still uses terminal half-blocks and the helper's 160-pixel bound; it is not native bitmap graphics. On text-only terminals the configured desktop viewer provides original-resolution display.

### Native Kitty graphics (experimental)

The optional visual helper supports RGB v2 up to 1024 pixels per side. In a direct Kitty session (TERM=xterm-kitty and KITTY_WINDOW_ID), the dashboard and expanded viewer use native RGB bitmap placement. Set TT_GRAPHICS=unicode to force the existing half-block renderer, or TT_GRAPHICS=kitty only when you know your terminal supports the Kitty protocol. tmux/screen are excluded. Old helpers or failed HD decodes fall back to v1. Rebuild the helper as well as the core. The core generates the graphics commands itself, chunks bounded pixel data, suppresses terminal replies, and deletes its own placement when leaving the view. GNOME Terminal/VTE and other non-Kitty terminals continue to use the half-block preview or the original-resolution desktop viewer. No Sixel support yet. See https://sw.kovidgoyal.net/kitty/graphics-protocol.html.

### Responsive visual previews

The live image/PDF panel queues the latest selection after a 120 ms debounce. At most one decoder runs at a time, with one replaceable pending request. The dashboard remains interactive while loading and shows the current task status; obsolete results are discarded rather than painted over a newer selection. Hidden panels cancel pending work; already-running helpers may finish under the existing resource limits. Expanded preview remains synchronous and waits for an in-flight decoder to finish before opening. No new runtime dependency or change to the file-operation engine.

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
