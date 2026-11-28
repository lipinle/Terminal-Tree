# Optional companion programs

Terminal Tree's own source remains under the MIT license (see LICENSE).
These optional companions are independent programs invoked through their
ordinary command-line interfaces, not linked libraries, embedded code,
vendored source, or bundled executables.

| Program | Upstream license | Upstream |
| --- | --- | --- |
| eza | EUPL-1.2 | https://github.com/eza-community/eza/blob/main/LICENSE.txt |
| 7-Zip | Mostly GNU LGPL, some BSD-licensed code and unRAR restrictions | https://github.com/ip7z/7zip/blob/main/DOC/License.txt |
| Git | GNU GPL version 2 (with compatible licenses for some parts) | https://git-scm.com/about |

These references are informational; the installed distribution packages'
license notices govern those programs. Terminal Tree does not change their
licenses or distribute their code. Any future redistribution or linking
requires a separate license review.

## Installation

The native Linux build detects eza, 7zz/7z and git under /usr/bin. They are
optional; native file browsing, USTAR handling and scoped file operations
remain available when none is installed. The user controls installation
and updates via the distribution package manager. For Linux Mint and
Ubuntu, where available:

```sh
sudo apt install eza 7zip git
```

Older distributions may use p7zip-full instead of 7zip. Package names and
availability vary. No automatic download, elevated command, or package
installation is performed by Terminal Tree.

Press Ctrl+P to see tool availability and installation help, or run
terminal-tree --tools. The tools menu invokes only explicit read-only
inspection: detailed listing, archive listing and Git status. Git status
is intended for trusted repositories; repository-local configuration can
request helpers. This is not a security sandbox. Archive extraction and
filesystem mutation remain the responsibility of the native reviewed
operations engine.

## Distribution boundary

Release archives contain Terminal Tree and its own integration files,
not eza, 7-Zip or Git. A future distribution package may recommend
optional independently packaged companions; bundling or linking
requires separate review.
