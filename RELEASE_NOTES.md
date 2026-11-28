# Terminal Tree Standard 3.0.0

First stable Rust Standard release for the documented supported scope. Linux x86-64 native binary; source is included by GitHub. Verify the archive using SHA256SUMS before extracting to a fresh owned directory. Start with `./terminal-tree --browse .` or a disposable workspace; do not run as root.

- Bounded tree and dual-pane navigation, tabs, search, bookmarks, themes and explicit preferences.
- Scoped, reviewed copy, move, rename, bulk rename, keep-both paste, Trash, restore and eligible undo.
- Bounded text/hex/PPM and USTAR/tar.gz handling; staged extraction into a new directory.
- Explicit retained cross-device file move and matching-evidence recovery. No automatic deletion of retained originals or replay of journals.
- Side-by-side source installer and optional Bash integration. Existing Python installation and shell startup are preserved.

The release is gated on the complete quality, compatibility, native and virtual crash workflows on its exact commit, including mounted fixtures and terminal regressions. The prebuilt Linux binary targets Ubuntu 24.04; use a source build if glibc compatibility requires it.

Known limits: native mutation is Linux-only, cross-device retained move is for one regular file, metadata fallback is limited to explicitly accepted timestamp rounding, and recovery requires matching current evidence. No overwrite, archive merge, general ZIP handling, arbitrary image formats, or automatic recovery from missing evidence. The new archive/retained operations have targeted interruption tests, not exhaustive VM power-cut coverage. No universal filesystem/concurrent-writer guarantees. Keep independent backups. Paranoid is not released here.
