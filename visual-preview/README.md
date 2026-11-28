# Terminal Tree Visual Preview (experimental)

Independent optional Rust executable. Our code is MIT; image-rs 0.25.10 and Hayro 0.9.0 are MIT/Apache-2.0 upstream. Default Hayro font/CMap embedding is disabled pending license review; PDF rendering may omit non-embedded fonts. No external executable or font is redistributed by the core.

Build separately with Rust 1.92 or later: `cargo build --release --manifest-path visual-preview/Cargo.toml`. The core discovers only `/usr/local/bin/tt-visual-preview` plus system timeout and prlimit. Installation is manual. For local testing, after building, `sudo install -m 755 visual-preview/target/release/tt-visual-preview /usr/local/bin/tt-visual-preview`. Enable Visual Preview in Add-ons; default off. Scroll over the preview to change PDF pages. GIF is a still image. Core uses truecolor half-blocks, not image escape protocols.

Development only: a generated Cargo.lock and full transitive license/notice audit are required before redistribution. The first CI metadata report listed permissive SPDX licenses (MIT, Apache-2.0, BSD-3-Clause, Zlib, 0BSD, Unlicense, Unicode-3.0); this is an initial screening, not verification of bundled notices or a reproducible dependency graph. CI reports dependency licenses; metadata is not legal verification. No release assets yet. Resources are bounded, but subprocess isolation is not a security sandbox. Preview is synchronous and may briefly block. Only trusted local files should be previewed. The helper accepts an already opened file via stdin and returns a bounded RGB frame, not terminal control sequences.

Upstream: https://github.com/image-rs/image and https://github.com/LaurenzV/hayro.
