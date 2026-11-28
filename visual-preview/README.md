# Visual Preview

Display images and PDF pages in Terminal Tree's UX preview. The helper is optional and experimental, and is not included in release archives.

## Install or update (Linux)

From the same source checkout as your Terminal Tree UX preview, with Rust 1.92 or newer:

~~~bash
cargo +stable build --release --manifest-path visual-preview/Cargo.toml
mkdir -p "$HOME/.local/bin"
install -m 700 visual-preview/target/release/tt-visual-preview "$HOME/.local/bin/tt-visual-preview"
~~~

No sudo, PATH modification or system-wide installation is required. The current core checks the fixed user-local path first, then the legacy root-owned `/usr/local/bin/tt-visual-preview`. The home directory, .local, bin and the helper must be owned by your account, not symlinks or group/world-writable; the helper must be executable. System timeout and prlimit are also required. Rebuild the helper after updating the source checkout. If updating from an older core, upgrade it first to recognize the local installation.

## Use

1. Launch Terminal Tree and open **Add-ons**.
2. Enable **Visual Preview**.
3. Select a PNG, JPEG, WebP, GIF, BMP or PDF in Auto mode.

Scroll over the preview to turn PDF pages; Alt+P changes mode and Alt+V opens the expanded view. GIFs are displayed as still images. Native graphics depend on your terminal. GNOME Terminal users on X11 can install the [optional X11 Overlay](../x11-overlay/README.md). Set TT_GRAPHICS=unicode if native graphics cause problems.

## Uninstall

Close Terminal Tree and remove the helper you installed:

~~~bash
rm -- "$HOME/.local/bin/tt-visual-preview"
~~~

An existing system-wide installation may remain detected. If you previously installed it under /usr/local/bin, verify it is your Terminal Tree helper before removing it with administrator privileges. No user files or preferences are removed by these steps.

## Limitations and license

Our helper is MIT-licensed; image-rs and Hayro are MIT/Apache-2.0 upstream. Font/CMap embedding is disabled pending license review; PDF rendering may omit non-embedded fonts. The helper remains source-only until the dependency lock and transitive notices audit are complete. Live preview decodes off the UI thread; expanded preview may briefly block. Only preview trusted local files; resource limits are not a security sandbox.

Upstream: https://github.com/image-rs/image and https://github.com/LaurenzV/hayro.
