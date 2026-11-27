# Changelog

## 2.1.0 — consolidated baseline

- Imported the original Bash navigator and earlier _print_branch implementation.
- Preserved installers 2.0.0, 2.0.1, 2.0.2 and 2.0.3.
- Integrated eza styling, 25 ms ESC delay, categorized F1 help, persistent file-opening navigation and mouse controls.
- Integrated the data safety patch: staged copies, atomic no-overwrite moves, review and identity checks, protected locations and restricted permanent deletion.
- Separated the current runtime, shell integration and installer; added regression tests and runtime backups on upgrade.

Earlier standalone patches are preserved byte-for-byte under archive/patches. The consolidated runtime was verified against the result of replaying those patches.
