//! Explicit color palettes for the terminal interface.

use crossterm::style::Color;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Theme {
    #[default]
    Terminal,
    Dark,
    Light,
    Monochrome,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    pub background: Color,
    pub text: Color,
    pub heading: Color,
    pub directory: Color,
    pub link: Color,
    pub selected_background: Color,
    pub selected_text: Color,
    pub error: Color,
}

impl Theme {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "terminal" => Some(Self::Terminal),
            "dark" => Some(Self::Dark),
            "light" => Some(Self::Light),
            "monochrome" => Some(Self::Monochrome),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Terminal => "terminal",
            Self::Dark => "dark",
            Self::Light => "light",
            Self::Monochrome => "monochrome",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Terminal => Self::Dark,
            Self::Dark => Self::Light,
            Self::Light => Self::Monochrome,
            Self::Monochrome => Self::Terminal,
        }
    }

    pub fn palette(self) -> Palette {
        match self {
            Self::Terminal => Palette {
                background: Color::Reset,
                text: Color::Reset,
                heading: Color::Cyan,
                directory: Color::Blue,
                link: Color::Cyan,
                selected_background: Color::DarkGrey,
                selected_text: Color::White,
                error: Color::Red,
            },
            Self::Dark => Palette {
                background: Color::Black,
                text: Color::White,
                heading: Color::Cyan,
                directory: Color::Cyan,
                link: Color::Yellow,
                selected_background: Color::White,
                selected_text: Color::Black,
                error: Color::Red,
            },
            Self::Light => Palette {
                background: Color::White,
                text: Color::Black,
                heading: Color::DarkBlue,
                directory: Color::DarkBlue,
                link: Color::DarkMagenta,
                selected_background: Color::Black,
                selected_text: Color::White,
                error: Color::DarkRed,
            },
            Self::Monochrome => Palette {
                background: Color::Black,
                text: Color::White,
                heading: Color::White,
                directory: Color::White,
                link: Color::White,
                selected_background: Color::White,
                selected_text: Color::Black,
                error: Color::White,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_theme_names_round_trip() {
        for theme in [
            Theme::Terminal,
            Theme::Dark,
            Theme::Light,
            Theme::Monochrome,
        ] {
            assert_eq!(Theme::parse(theme.name()), Some(theme));
        }
        assert_eq!(Theme::parse("unknown"), None);
        assert_eq!(Theme::default(), Theme::Terminal);
    }

    #[test]
    fn cycling_returns_to_terminal_defaults() {
        let mut theme = Theme::default();
        for expected in [
            Theme::Dark,
            Theme::Light,
            Theme::Monochrome,
            Theme::Terminal,
        ] {
            theme = theme.next();
            assert_eq!(theme, expected);
        }
    }

    #[test]
    fn explicit_palettes_distinguish_selection_and_text() {
        for theme in [Theme::Dark, Theme::Light, Theme::Monochrome] {
            let palette = theme.palette();
            assert_ne!(palette.background, palette.text);
            assert_ne!(palette.selected_background, palette.selected_text);
            assert_ne!(palette.background, palette.selected_background);
        }
    }
}
