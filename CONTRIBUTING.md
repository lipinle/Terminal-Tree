# Contributing

Terminal Tree is in development. Contributions that improve daily-driver usability, correctness, performance and maintainability are welcome. Keep advanced Paranoid work separate from Standard release blockers.

## Development

Fork the repository, create a focused branch from main, and open a pull request. Describe the problem, the intended behavior, test evidence and any compatibility or data-safety trade-offs. Do not mix mechanical formatting with functional changes. Do not modify the archived Python implementation unless a separate maintainer decision explicitly changes its archival status.

```bash
cargo fmt --manifest-path rust/Cargo.toml --all -- --check
cargo test --locked --manifest-path rust/Cargo.toml --features native-ops
cargo test --locked --manifest-path rust/Cargo.toml --no-default-features
cargo clippy --locked --manifest-path rust/Cargo.toml --all-targets --all-features -- -D warnings
cargo clippy --locked --manifest-path rust/Cargo.toml --all-targets --no-default-features -- -D warnings
```

The strict formatting and Clippy CI gate is being developed in PR #3. Some existing lints may still fail until that PR is verified and merged. Use four-space indentation, readable names, rustfmt and narrowly justified lint exceptions. Preserve the declared minimum Rust version unless explicitly changed.

## Safety requirements

Test writes only on disposable fixtures and as an unprivileged user. Keep no-overwrite behavior, identity checks, journal semantics, error propagation, sync ordering and cancellation guarantees intact. New operations must include failure-path and regression tests. Filesystem and crash harnesses under tests/ require isolated environments; never point them at live user data. Do not add a root requirement to the application. Avoid unrequested background services, network calls and unnecessary dependencies.

For a potential vulnerability or data-loss bug, follow [SECURITY.md](SECURITY.md) rather than posting sensitive details in a public issue.

## Review

Every change should have a clear scope, test results and an explanation of limitations. A green CI result alone does not prove safety. Respect other contributors; focus criticism on the code and evidence. Contributions are distributed under the repository's MIT license.
