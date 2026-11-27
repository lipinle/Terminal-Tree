# Security and data safety

Terminal Tree runs with the invoking user's privileges. Do not run the installer or navigator as root. Maintain independent backups of important files. Atomic no-overwrite operations do not guarantee durability after power loss; multi-item operations can partially complete. Path identity checks reduce ordinary races but are not a complete defense against concurrent hostile filesystem modification.

Report security or data-loss concerns privately to the repository maintainer; avoid posting personal paths or sensitive file contents in issues.

## 2.1.1 limitations

Verified copies refuse filesystems or permissions that cannot preserve checked metadata (including ownership, nanosecond modification time and extended attributes). Directory sync failures may leave a completed rename with uncertain durability. `terminal_tree_recovery` reports unresolved journal records; inspect both paths and any staging directory before retrying. Records are private local files under `$XDG_STATE_HOME/terminal-tree/operations` (default `~/.local/state/terminal-tree/operations`). Do not delete pending records or stages without inspecting them. A pending record is not proof that an operation failed or succeeded. The journal is not a backup, cannot protect against disk/controller failure, and does not provide automatic rollback or protect against all hostile filesystem races. GIO controls Trash destination and durability. Permanent deletion in Trash is irreversible. A successful CI run is not proof of crash consistency on every filesystem.
