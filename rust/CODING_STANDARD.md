# Rust coding standard

This is a release gate for the shared core and Standard edition. Paranoid work follows the same rules.

## Formatting

- Use rustfmt for every Rust source file, including examples, tests and the isolated crash probe.
- Use four-space indentation and the repository's 100-column formatting target. Do not manually pack multiple statements or branches onto one line.
- Run from the repository root with the CI toolchain:
  `cargo +1.100.0 fmt --manifest-path rust/Cargo.toml --all -- --check`.
- Run the same command without `-- --check` to format changes.

## Lints

- Run `cargo +1.100.0 clippy --locked --manifest-path rust/Cargo.toml --all-targets --all-features -- -D warnings` on Linux.
- Also check the portable read-only build using `--no-default-features` in place of `--all-features`.
- Resolve warnings at their cause. Any necessary exception must be narrow, justified by a comment and independently reviewed. No broad suppression merely to pass CI.
- Maintain the declared minimum supported Rust version; do not adopt a lint suggestion requiring a newer compiler without a deliberate compatibility decision.

## Security-critical changes

- Separate mechanical formatting from behavior changes. Preserve operation ordering, fail-closed decisions, checkpoint names, journal schema and durability barriers.
- Prefer meaningful names and small functions. Explain why an invariant, repeated check, or synchronization barrier is required.
- Do not remove safety checks just to simplify code. Do not introduce unchecked filesystem operations, silent fallback, detached writes or unsafe code.
- Review errors and cancellation at pre-commit and post-commit boundaries. Do not treat a pending journal as proof of failure.
- Preserve existing unit, integration, PTY, compatibility and isolated crash tests. Add regression coverage for behavioral fixes.

## Acceptance

Formatting and both Clippy configurations must pass before a stable release, alongside the existing correctness and crash-consistency workflows. A clean lint result is not a security audit.
