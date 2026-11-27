# Milestones

## Release strategy

Two separately installable editions share one Rust core and one source repository.

| Edition | Proposed release line | Build | Intended users |
|---|---|---|---|
| Terminal Tree Standard | 3.0.x, 3.1.x, ... | Native core only; advanced modules excluded at compile time | Everyday users |
| Terminal Tree Paranoid | 3.0.x-paranoid.N preview; 3.0.0-paranoid edition release naming to be finalized | Native core plus advanced security modules | Administrators, security teams, forensic practitioners, regulated environments |

Use distinct artifact names (`terminal-tree-standard`, `terminal-tree-paranoid`) and edition-specific manifests, not independent copies of the source tree. The final edition label is separate from SemVer: a stable Paranoid edition can have version 3.0.0 without a SemVer pre-release suffix. Paranoid must remain optional; its milestones must not delay a qualified Standard release. Maintain shared core fixes and security tests for both. Preserve Python Legacy untouched.

Current state: Rust 0.1.0 experimental; PR #2 introduces runtime Standard/Paranoid profiles, not yet separate binaries. All entries below are proposed delivery gates, not claims of completed functionality or enterprise certification.

## Standard release milestones

| Milestone | Deliverable | Acceptance gate |
|---|---|---|
| S0 — Edition isolation | Cargo feature boundaries; distinct artifacts; explicit edition/version output | Standard builds without advanced dependencies or entry points; both artifacts built from same core; no background service |
| S1 — Core assurance | Essential safety, atomic no-overwrite, reviewed identities, metadata preservation, durable journal and sync, independently verified copy, scoped writes, safe Trash and Undo | Existing safety and crash suites pass; no regression; measure startup, RSS, CPU, binary size and I/O |
| S2 — Linux release candidate | Recovery guidance, native installation/uninstallation, Bash integration, compatibility evidence | Disposable installation/upgrade tests and Linux filesystem tests; independently review remaining risks |
| S3 — Standard 3.0.0 | Independently release Standard | All mandatory gates pass; explicitly document supported environments and limitations |

## Paranoid edition milestones

Feature checklists are intentionally exhaustive. Implement or integrate each control with a documented threat model, capability probe, explicit failure semantics and evidence. Where a control is infeasible or outside the file manager's authority, document the limitation rather than simulate assurance.

### P0 — Edition foundations (preview)

- [ ] Compile-time `paranoid` feature layered on native operations; Standard artifact excludes advanced modules and dependencies.
- [ ] Separate binary/package identity and isolated edition settings; shared core, compatible journal schema and explicit migration.
- [ ] Profile shown in UI/CLI/audit; no silent fallback or cross-profile review reuse.
- [ ] Independent test and benchmark matrix; no unrequested daemons, root privileges or network access.

Gate: both editions build and install side by side; Standard has no Paranoid runtime entry points or dependency overhead. Paranoid failure never weakens Standard's core guards.

### P1 — Advanced Data Integrity

- [ ] Full SHA-256 verification
- [ ] Integrity baseline
- [ ] Integrity scanner
- [ ] Bit rot detection
- [ ] Merkle tree
- [ ] Digital signature verification
- [ ] Metadata integrity (permissions, ownership, ACL, xattrs)
- [ ] Optional fs-verity integration

Gate: trusted baseline provenance, changed/corrupted files detected, symlinks and raw paths handled; unsupported fs-verity reported explicitly.

### P2 — Advanced Transaction Safety

- [ ] Transaction simulation
- [ ] Preflight analysis
- [ ] Verified rollback
- [ ] Transaction recovery
- [ ] Transaction isolation
- [ ] Persistent undo
- [ ] Concurrent modification detection
- [ ] Storage capacity reservation where supported

Gate: crash/fault injection per transaction boundary; no overwrite or automatic destructive replay; detect uncertainty; rollback only with verified state and available recovery data.

### P3 — Audit and Accountability

- [ ] Detailed audit log
- [ ] Tamper-evident log
- [ ] Cryptographic log chain
- [ ] Signed audit records
- [ ] Remote audit storage (opt-in)
- [ ] Activity timeline
- [ ] Session recording (opt-in; redact secrets)
- [ ] Audit export (JSON/CSV/report)

Gate: detect log modification, reordering and truncation against trusted external anchors; protect sensitive paths and retention; local log alone cannot prove administrator-level integrity.

### P4 — Access Control

- [ ] Policy engine
- [ ] Role-based access control
- [ ] Protected directories
- [ ] Signed policy
- [ ] Operation allowlist
- [ ] Destination allowlist
- [ ] Dual authorization
- [ ] Step-up authentication
- [ ] Session auto-lock

Gate: deny by default for managed policy; verified policy provenance; OS/service-backed identity and enforcement for organization-wide controls. The UI alone cannot constrain other programs.

### P5 — Storage Intelligence

- [ ] SMART health integration
- [ ] NVMe health integration
- [ ] Filesystem health check
- [ ] Disk error awareness
- [ ] Mount trust policy
- [ ] Network filesystem detection
- [ ] Filesystem capability detection
- [ ] UPS awareness

Gate: read-only probes by default, bounded timeouts; unrecognized devices clearly marked; fail closed for unsupported destructive operations; no implicit repair or elevation.

### P6 — Anti-Ransomware and Backup / Data Lifecycle

- [ ] Mass deletion guard
- [ ] Mass modification guard
- [ ] Operation rate limit
- [ ] Canary files
- [ ] Emergency lockdown
- [ ] File entropy analysis
- [ ] Immutable backup integration
- [ ] Snapshot before destruction
- [ ] Verified backup
- [ ] Incremental backup integration
- [ ] Snapshot management
- [ ] Restore verification
- [ ] Backup retention policy
- [ ] Legal hold
- [ ] Secure deletion policy
- [ ] Backup recovery test

Gate: validate restore on disposable fixtures, protect backups from the application account where possible; entropy is a heuristic; lockdown only covers Terminal Tree unless external enforcement exists. Do not promise secure erasure on SSD, CoW or snapshots.

### P7 — Digital Forensics

- [ ] Forensic read-only mode
- [ ] Evidence hashing
- [ ] Chain of custody
- [ ] Evidence inventory
- [ ] File signature analysis
- [ ] Timestamp analysis
- [ ] Metadata extraction
- [ ] Evidence report
- [ ] Disk image inspection

Gate: evidence accessed through verified read-only media/mounts or write blockers; no automatic execution, atime/write limitations documented; raw bytes and provenance retained.

### P8 — Data Confidentiality

- [ ] Encrypted file vault integration
- [ ] Encrypted transfer
- [ ] Encrypted journal
- [ ] Encrypted backup
- [ ] Sensitive data classification
- [ ] Data loss prevention policy
- [ ] Metadata privacy
- [ ] Hardware key integration

Gate: established crypto tools/libraries, no custom cryptography; explicit key ownership and recovery; no plaintext secret logging; DLP scope restricted to mediated operations unless external enforcement is configured.

### P9 — Secure Execution Environment

- [ ] Executable integrity check
- [ ] Configuration integrity
- [ ] Sandboxed preview
- [ ] External command allowlist
- [ ] Offline mode
- [ ] Dependency verification
- [ ] Signed release
- [ ] Reproducible builds
- [ ] SBOM

Gate: enforce sandbox/network restrictions via the OS, not UI labels; pinned dependencies, verified release signatures and independent build evidence.

### P10 — Enterprise integration and Paranoid release candidate

- [ ] Threat model and independent security review
- [ ] OS-backed identity, remote audit and policy integration tests
- [ ] Real hardware / filesystem fault testing and recovery exercises
- [ ] Measure overhead with modules disabled and enabled
- [ ] Document deployment, key custody, retention, data protection, and supported configurations
- [ ] Separate edition artifacts and signed distribution
- [ ] Publish Paranoid only after its own acceptance gates are met

## Architecture rules

- Standard is the baseline, not an unsafe mode. Never remove essential no-overwrite, path, identity, journal or durability protections merely to improve a benchmark.
- Advanced features live behind the edition feature boundary and per-capability activation; use trusted external tools for encryption, authentication, backups, hardware diagnostics and other specialized tasks where appropriate.
- No automatically trusted journal paths or external command output. No destructive action from an unverified record.
- Maintain one shared core, edition-specific CI and release artifacts; do not fork security-critical logic.
- Track the 83 advanced feature proposals under P1–P9. P0/P10 add packaging and release acceptance work, not additional claims of completed controls.
