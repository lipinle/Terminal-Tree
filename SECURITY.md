# Security and data safety

Terminal Tree runs with the invoking user's privileges. Do not run the installer or navigator as root. Maintain independent backups of important files. Atomic no-overwrite operations do not guarantee durability after power loss; multi-item operations can partially complete. Path identity checks reduce ordinary races but are not a complete defense against concurrent hostile filesystem modification.

Report security or data-loss concerns privately to the repository maintainer; avoid posting personal paths or sensitive file contents in issues.
