# Experimental X11 overlay

An independent MIT Rust helper (x11rb, MIT/Apache-2.0 upstream). No new dependency in the core. It makes a bounded 24-bit RGB child window of an explicitly selected active GNOME Terminal X11 client. The input shape is empty, so clicks pass through. The window is destroyed when the lifetime socket closes, the target loses active-window status, its size changes, or the process exits. No screen capture, input grabs, or always-on-top window. X11 itself is not a security sandbox.

Build and install manually from the same checkout:

    cargo +stable build --release --manifest-path x11-overlay/Cargo.toml
    sudo install -m 755 x11-overlay/target/release/tt-x11-overlay /usr/local/bin/tt-x11-overlay

Also build/install the current Visual Preview helper (RGB v2), enable Visual Preview in Add-ons, and use a dedicated GNOME Terminal X11 window with no other tabs. Run `xprop -root _NET_ACTIVE_WINDOW` in that terminal for its XID. Use `xprop -id XID WM_NORMAL_HINTS` to inspect the resize increments (cell width and height); `xwininfo -id XID` gives client dimensions. Geometry requires explicit calibration: `TT_X11_GEOMETRY=LEFT,TOP,CELL_WIDTH,CELL_HEIGHT` (pixels measured from the client, not the screen). Top includes menu/tab bars and padding; left includes padding. These are not universal constants. Example launch syntax (replace both values):

    TT_GRAPHICS=x11 TT_X11_WINDOW_ID=0x123456 TT_X11_GEOMETRY=2,28,10,22 ./rust/target/release/terminal-tree --app "$HOME"

Use only a dedicated window; switching tabs within one client or changing font/menu layout without changing outer size may invalidate calibration. Restart with correct geometry if needed. Focus events remove overlay; the helper independently checks active window and client size. If placement fails, the dashboard falls back to Unicode. Unset TT_GRAPHICS or set unicode to disable. Expanded preview still uses Unicode on GNOME. No Wayland support; no automatic calibration yet. Xvfb tests child-window geometry and input transparency, not GNOME/VTE integration. Full transitive license and reproducibility audit required before redistribution.
