# Roadmap

## Product direction

Terminal Tree Standard must be a complete practical terminal file manager before expanding the advanced security toolkit. Retain essential data safety throughout development. Paranoid is a separate future edition; do not let its feature backlog delay Standard.

Python 2.1.1 is archived unchanged. Rust 0.1.0 is a development preview; Standard 3.0.0 is the intended next stable product version, not a released version.

## Standard milestones

| ID | Deliverable | Exit criteria |
|---|---|---|
| S1 | Source formatting | All Rust targets pass rustfmt; regression tests pass. |
| S2 | Clippy and technical debt | Native and portable lint gates pass without broad suppressions; compatibility and crash suites pass. |
| S3 | Edition separation | Distinct build artifacts; Standard retains essential guards and excludes optional advanced modules. |
| S4 | Navigation and true tree | Expand/collapse, depth controls, keyboard/mouse navigation with bounded scanning. |
| S5 | Layout | Tabs and dual panes; consistent selection, focus and state. |
| S6 | File operations and filesystem compatibility | Cross-filesystem copy-verify-delete with recoverable state, explicit conflict resolution, bulk rename and opt-in metadata fallback. |
| S7 | Search and bookmarks | Recursive filename/content search, filtering, cancellation and saved locations; bounded resource usage. |
| S8 | Preview, archives and utilities | Text/hex preview, capability-aware image preview, safe archive browsing/extraction/compression. |
| S9 | Customization and UX | Themes, keybindings, settings, useful progress, error messages and help. |
| S10 | Basic recovery and essential safety | Detect interrupted operations; verify evidence; offer safe recovery; never blindly delete or replay. |
| S11 | Optimization and benchmarks | Measure idle/peak RSS, CPU, I/O, startup, navigation, file operations and binary size on reproducible fixtures. |
| S12 | Installer and shell integration | Safe install/update/uninstall and Bash integration; preserve unrelated user configuration. |
| S13 | Documentation | Accurate installation, usage, compatibility, recovery and migration guides. |
| S14 | Security review and regression | Review both editions and supported filesystems; fault injection, real-world testing and no unresolved release-blocking defects. |
| S15 | Release candidate | Versioned, reproducible candidate; bug fixes only; publish known limitations. |
| S16 | Standard 3.0.0 | Release only when acceptance gates pass. |

S1 is verified on the code-quality branch, not yet merged. S2 fixes are committed on that branch but CI confirmation is blocked by exhausted compute; do not mark S2 complete until tests pass. PR #2 has passed its earlier test matrix but has not been merged and requires reconciliation with the quality branch. S3–S16 are planned, not implemented as milestones.

Recovery safety must be designed alongside S6: cross-filesystem moves are not atomic across devices, so destination publication, source deletion and journal state must be recoverable before enabling writes. Do not weaken content integrity, no-overwrite, path validation or failure handling to improve convenience.

## Paranoid edition

Defer advanced integrity, forensic, audit, organizational policy and related modules until Standard is stable. Keep the detailed proposed milestones on the Paranoid development branch (PR #2). They are not part of the Standard release commitment.

No release date is promised. Scope and priority may change with evidence from users and testing.
