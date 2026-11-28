//! Built-in file-type classification and color palette.
//! Unicode characters and optional Seti-compatible font code points only.
//! No third-party font, vector artwork or executable is embedded.
use crossterm::style::Color;
use std::path::Path;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Style {
    #[default]
    Unicode,
    Seti,
    Ascii,
}

impl Style {
    pub fn from_environment() -> Self {
        match std::env::var("TT_ICON_STYLE").as_deref() {
            Ok("seti" | "nerd") => Self::Seti,
            Ok("ascii") => Self::Ascii,
            _ => Self::Unicode,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Unicode => "Unicode",
            Self::Seti => "Seti",
            Self::Ascii => "ASCII",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Link, Folder, File, Text, Image, Audio, Video, Archive, Pdf, Document,
    Table, Presentation, Rust, Python, Javascript, Typescript, Json,
    Html, Css, Markdown, Shell, Config, Git, License, Xml, Go, Java,
    C, Cpp, Lua, Docker, Font, Binary, Yaml,
}

fn classify(path: &Path, directory: bool, symlink: bool) -> Kind {
    use Kind::*;
    if symlink { return Link; }
    if directory { return Folder; }
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_ascii_lowercase();
    match name.as_str() {
        "dockerfile" | "containerfile" | ".dockerignore" => return Docker,
        "license" | "license.md" | "license.txt" | "copying" | "notice" => return License,
        ".gitignore" | ".gitattributes" | ".gitmodules" => return Git,
        "makefile" | "cmakelists.txt" | ".bashrc" | ".zshrc" => return Config,
        "readme" | "changelog" => return Markdown,
        _ => (),
    }
    match path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase().as_str() {
        "rs" => Rust,
        "py" | "pyw" | "pyi" => Python,
        "js" | "jsx" | "mjs" | "cjs" => Javascript,
        "ts" | "tsx" | "mts" | "cts" => Typescript,
        "json" | "jsonc" | "jsonl" => Json,
        "html" | "htm" => Html,
        "css" | "scss" | "sass" | "less" => Css,
        "md" | "markdown" | "rst" => Markdown,
        "sh" | "bash" | "zsh" | "fish" | "ps1" => Shell,
        "toml" | "ini" | "conf" | "cfg" | "env" => Config,
        "yaml" | "yml" => Yaml,
        "xml" | "xsl" | "xsd" => Xml,
        "go" => Go,
        "java" | "kt" | "kts" => Java,
        "c" | "h" => C,
        "cpp" | "cc" | "cxx" | "hpp" => Cpp,
        "lua" => Lua,
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp" | "ico" | "avif" | "heic" | "ppm" => Image,
        "mp3" | "flac" | "wav" | "ogg" | "m4a" | "opus" => Audio,
        "mp4" | "mkv" | "webm" | "avi" | "mov" | "mpeg" => Video,
        "zip" | "7z" | "rar" | "tar" | "gz" | "xz" | "bz2" | "zst" | "tgz" | "jar" => Archive,
        "pdf" => Pdf,
        "doc" | "docx" | "odt" | "rtf" => Document,
        "xls" | "xlsx" | "ods" | "csv" | "tsv" => Table,
        "ppt" | "pptx" | "odp" => Presentation,
        "ttf" | "otf" | "woff" | "woff2" => Font,
        "exe" | "dll" | "so" | "bin" | "elf" | "appimage" | "deb" | "rpm" => Binary,
        "txt" | "log" => Text,
        _ => File,
    }
}

pub fn icon(path: &Path, directory: bool, symlink: bool, style: Style) -> (&'static str, Color) {
    use Kind::*;
    let kind = classify(path, directory, symlink);
    // Each row is independently chosen. Seti code points are public font
    // interoperability identifiers, not copied glyph outlines.
    let (unicode, seti, ascii, color) = match kind {
        Link => ("↗", "↗", "L", Color::Cyan),
        Folder => ("📁", "\u{e613}", "D", Color::Blue),
        File => ("📄", "\u{e64e}", "F", Color::White),
        Text => ("≡", "\u{e64e}", "T", Color::Grey),
        Image => ("🖼", "\u{e60d}", "I", Color::Magenta),
        Audio => ("♫", "\u{e638}", "A", Color::Magenta),
        Video => ("▶", "\u{e69f}", "V", Color::Red),
        Archive => ("📦", "\u{e6aa}", "Z", Color::Yellow),
        Pdf => ("▤", "\u{e67d}", "P", Color::Red),
        Document => ("▤", "\u{e64e}", "W", Color::Blue),
        Table => ("▦", "\u{e64e}", "X", Color::Green),
        Presentation => ("▣", "\u{e64e}", "P", Color::DarkYellow),
        Rust => ("⚙", "\u{e68b}", "R", Color::DarkYellow),
        Python => ("Py", "\u{e606}", "Py", Color::Yellow),
        Javascript => ("JS", "\u{e60c}", "JS", Color::Yellow),
        Typescript => ("TS", "\u{e628}", "TS", Color::Cyan),
        Json => ("{}", "\u{e60b}", "{}", Color::Yellow),
        Html => ("<>", "\u{e60e}", "<>", Color::DarkYellow),
        Css => ("#", "\u{e614}", "#", Color::Blue),
        Markdown => ("M↓", "\u{e609}", "MD", Color::Cyan),
        Shell => ("$>", "\u{e691}", "$>", Color::Green),
        Config => ("⚙", "\u{e615}", "*", Color::Grey),
        Git => ("◆", "\u{e65d}", "G", Color::DarkYellow),
        License => ("§", "\u{e60a}", "L", Color::Yellow),
        Xml => ("<>", "\u{e619}", "<>", Color::DarkYellow),
        Go => ("Go", "\u{e627}", "Go", Color::Cyan),
        Java => ("J", "\u{e66d}", "J", Color::Red),
        C => ("C", "\u{e649}", "C", Color::Blue),
        Cpp => ("C+", "\u{e646}", "C+", Color::Blue),
        Lua => ("◐", "\u{e620}", "Lu", Color::Blue),
        Docker => ("▦", "\u{e650}", "Dk", Color::Cyan),
        Font => ("Aa", "\u{e659}", "Aa", Color::Magenta),
        Binary => ("▣", "\u{e64e}", "B", Color::Green),
        Yaml => ("Y", "\u{e6a8}", "Y", Color::Red),
    };
    (match style {
        Style::Unicode => unicode,
        Style::Seti => seti,
        Style::Ascii => ascii,
    }, color)
}

#[cfg(test)]
mod tests {
    use super::*;
    use unicode_width::UnicodeWidthStr;

    #[test]
    fn names_extensions_and_links_are_classified_without_io() {
        for (name, kind) in [
            ("main.RS", Kind::Rust), ("script.py", Kind::Python),
            ("Dockerfile", Kind::Docker), (".gitignore", Kind::Git),
            ("LICENSE.md", Kind::License), ("archive.tar.gz", Kind::Archive),
            ("photo.PNG", Kind::Image), ("report.xlsx", Kind::Table),
            ("unknown", Kind::File),
        ] {
            assert_eq!(classify(Path::new(name), false, false), kind);
            for style in [Style::Unicode, Style::Seti, Style::Ascii] {
                let (glyph, _) = icon(Path::new(name), false, false, style);
                assert!((1..=2).contains(&glyph.width()));
                assert!(!glyph.chars().any(char::is_control));
                if style == Style::Ascii { assert!(glyph.is_ascii()); }
            }
        }
        assert_eq!(classify(Path::new("main.rs"), true, false), Kind::Folder);
        assert_eq!(classify(Path::new("main.rs"), true, true), Kind::Link);
        assert_eq!(icon(Path::new("main.rs"), false, false, Style::Seti).0, "\u{e68b}");
    }
}
