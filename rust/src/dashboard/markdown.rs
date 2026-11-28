//! Bounded Markdown preview formatting.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub(super) enum PreviewStyle {
    #[default]
    Plain,
    #[cfg(feature = "native-ops")]
    Heading,
    #[cfg(feature = "native-ops")]
    Code,
    #[cfg(feature = "native-ops")]
    Quote,
}
#[cfg(feature = "native-ops")]
pub(super) fn markdown_preview(lines: &[String], width: usize) -> (Vec<String>, Vec<PreviewStyle>) {
    use unicode_width::UnicodeWidthChar;
    let mut out = Vec::new();
    let mut styles = Vec::new();
    let mut fence = false;
    let width = width.max(1);
    for (i, raw) in lines.iter().enumerate() {
        if out.len() >= 8192 {
            break;
        }
        let trimmed = raw.trim_start();
        let (text, style) = if i == 0 {
            (raw.clone(), PreviewStyle::Heading)
        } else if trimmed.starts_with(&char::from(96).to_string().repeat(3))
            || trimmed.starts_with("~~~")
        {
            fence = !fence;
            (raw.clone(), PreviewStyle::Code)
        } else if fence {
            (raw.clone(), PreviewStyle::Code)
        } else if let Some((_, title)) = trimmed
            .split_once(' ')
            .filter(|(p, _)| !p.is_empty() && p.len() <= 6 && p.bytes().all(|c| c == b'#'))
        {
            (title.to_owned(), PreviewStyle::Heading)
        } else if let Some(s) = trimmed.strip_prefix("> ") {
            (format!("│ {s}"), PreviewStyle::Quote)
        } else if let Some(s) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
        {
            (format!("• {s}"), PreviewStyle::Plain)
        } else {
            (raw.clone(), PreviewStyle::Plain)
        };
        let mut line = String::new();
        let mut cells = 0;
        for ch in text.chars() {
            let n = ch.width().unwrap_or(0);
            if n > width {
                continue;
            }
            if cells + n > width {
                if out.len() >= 8192 {
                    return (out, styles);
                }
                out.push(std::mem::take(&mut line));
                styles.push(style);
                cells = 0;
            }
            line.push(ch);
            cells += n;
        }
        if out.len() < 8192 {
            out.push(line);
            styles.push(style);
        }
    }
    (out, styles)
}
