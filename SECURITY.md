# Security policy

## Supported versions

The Rust native implementation is a development preview (0.1.0); no stable Rust version is currently supported. Python 2.1.1 is archived and not actively maintained. Standard 3.0.0 and Paranoid are future releases. No security response time or long-term support commitment is implied.

## Reporting vulnerabilities

Please report suspected vulnerabilities, unsafe file operations, corruption or unintended deletion privately. Use GitHub's private vulnerability reporting if it is enabled for this repository. If it is not available, contact the maintainer privately through an established channel; do not publish exploit details, personal paths, credentials, or file contents in a public issue. Include the affected commit, OS/filesystem, sanitized reproduction steps and expected versus observed behavior. Use synthetic data for reproduction.

## Current Rust preview limitations

- Run the application as an unprivileged user. Keep independent backups; a journal is not a backup.
- Native write operations require a scoped workspace or personal mode. Existing destinations are never overwritten. Cross-filesystem moves are refused on main.
- Strict metadata checks can reject legitimate copies to filesystems without the required capabilities. A successful rename may have uncertain durability if a later sync fails.
- Multi-item operations and permanent deletion can partially complete. Undo is session-local for eligible moves/renames; it cannot undo permanent deletion.
- Pending records are neither proof of failure nor success. Inspect source, destination, staging and metadata before retrying. The recovery inspector does not automatically replay, delete or roll back.
- Path identity checks reduce races but are not a complete defense against hostile concurrent filesystem modification. External applications may modify files opened through them.
- VM crash tests are controlled simulations, not certification for every device, controller, filesystem or physical power-loss scenario.
- Do not use the preview as the only tool for irreplaceable data. No guarantee of data recovery or absolute security is provided.

The historical Python-specific policy is retained at [legacy/python/SECURITY.md](legacy/python/SECURITY.md).
