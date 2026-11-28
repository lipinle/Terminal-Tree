# File icon interoperability

Terminal Tree's independently authored file classification, colors and icon selection are MIT-licensed. Unicode mode uses standard text characters, not extracted glyph images. ASCII mode uses ordinary text.

Seti mode emits font code points for compatibility with a user-installed Nerd Font. References:
- Seti UI, Jesse Weed, MIT: https://github.com/jesseweed/seti-ui/blob/master/LICENSE.md
- Nerd Fonts glyph names/code points: https://github.com/ryanoasis/nerd-fonts/blob/master/glyphnames.json
- Nerd Fonts licensing audit: https://github.com/ryanoasis/nerd-fonts/blob/master/license-audit.md

No Seti SVGs, font files, eza code, or other third-party icon artwork are copied into or distributed with Terminal Tree. The user's installed font supplies the shapes and retains its own license. Font and trademark rights are not granted by Terminal Tree. The built-in mapping functions without any external executable. Seti is an optional compatibility profile; Unicode and ASCII do not require a patched font.
