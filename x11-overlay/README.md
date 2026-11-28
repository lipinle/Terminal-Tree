# Experimental X11 overlay

An independent MIT Rust helper (x11rb, MIT/Apache-2.0 upstream). No new dependency in the core. It makes a bounded RGB child window of an explicitly selected active GNOME Terminal X11 client. Input shape is empty so clicks pass through. The window is destroyed when the lifetime socket closes, the target loses active-window status, its size changes, or the process exits. No screen capture, input grabs, or always-on-top window. X11 is not a security sandbox.

Build and install manually from the same checkout:

    cargo +stable build --release --manifest-path x11-overlay/Cargo.toml
    sudo install -m 755 x11-overlay/target/release/tt-x11-overlay /usr/local/bin/tt-x11-overlay

Build/install the current Visual Preview helper (RGB v2) too. Enable Visual Preview in Add-ons. In a dedicated, active GNOME Terminal X11 window with no other tabs, start:

    TT_GRAPHICS=x11 ./rust/target/release/terminal-tree --app "$HOME"

The helper queries the active GNOME Terminal client, pins its XID for the session and reads ICCCM WM_NORMAL_HINTS resize increments. It infers cell size from those hints and the current terminal rows/columns. Since exact menu and padding offsets are unknown, it uses the intersection of all possible grid positions, shrinking the image to avoid spilling outside the requested rectangle. Calibration is conservative, not exact VTE widget geometry. Unsupported or ambiguous hints fall back to Unicode. Both 24-bit and compatible 32-bit TrueColor visuals are accepted. Resize and focus gain refresh the pinned client's calibration; never switch to a different client automatically.

Optional manual override: use xprop -root _NET_ACTIVE_WINDOW for the ID and xprop -id XID WM_NORMAL_HINTS for resize increments. Supply TT_X11_WINDOW_ID and TT_X11_GEOMETRY=LEFT,TOP,CELL_WIDTH,CELL_HEIGHT together for measured client offsets. A window ID alone pins automatic detection to that client.

Use a dedicated single-tab window: switching tabs or changing font/menu layout without a resize may invalidate calibration. Restart if necessary. No Wayland, remote SSH, screen or tmux support. Expanded preview also attempts X11 and falls back to Unicode if placement fails. Xvfb verifies child geometry/input transparency; real GNOME/VTE validation and transitive dependency license/lock review remain necessary before redistribution.
