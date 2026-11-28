//! Application preferences: display-only, bounded data; never write authority.
use crate::file_icons::{self, Style};
use crossterm::style::Color;
use std::{collections::BTreeMap, path::Path};
#[cfg(feature = "native-ops")]
use std::io;

#[derive(Clone)]
pub struct Preferences {
    pub foreground: Color,
    pub background: Color,
    pub accent: Color,
    pub selection: Color,
    pub sidebar: bool,
    pub toolbar: bool,
    pub show_icons: bool,
    pub file_colors: bool,
    pub status_details: bool,
    pub companions: [bool; 3],
    pub custom: BTreeMap<String, String>,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            foreground: Color::Reset, background: Color::Reset, accent: Color::Cyan,
            selection: Color::DarkGrey, sidebar: true, toolbar: true,
            show_icons: true, file_colors: true, status_details: true,
            companions: [true; 3], custom: BTreeMap::new(),
        }
    }
}

impl Preferences {
    pub fn icon(&self, path: &Path, directory: bool, symlink: bool, style: Style) -> (&str, Color) {
        let (builtin, color) = file_icons::icon(path, directory, symlink, style);
        let glyph = if !self.show_icons {
            ""
        } else if !directory && !symlink && style != Style::Ascii {
            path.extension().and_then(|x| x.to_str())
                .and_then(|ext| self.custom.get(&ext.to_ascii_lowercase()))
                .map(String::as_str).unwrap_or(builtin)
        } else { builtin };
        (glyph, if self.file_colors { color } else { self.foreground })
    }
}

#[cfg(feature = "native-ops")]
pub fn parse_color(value: &str) -> io::Result<Color> {
    match value {
        "default" => Ok(Color::Reset),
        "cyan" => Ok(Color::Cyan),
        "grey" => Ok(Color::DarkGrey),
        _ => {
            let hex = value.strip_prefix('#').ok_or_else(|| io::Error::other("use #RRGGBB or default"))?;
            if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(io::Error::other("invalid RGB color"));
            }
            let value = u32::from_str_radix(hex,16).map_err(io::Error::other)?;
            Ok(Color::Rgb { r: (value >> 16) as u8, g: (value >> 8) as u8, b: value as u8 })
        }
    }
}

#[cfg(feature = "native-ops")]
fn encode_color(color: Color) -> String {
    match color {
        Color::Rgb { r, g, b } => format!("#{r:02x}{g:02x}{b:02x}"),
        Color::Cyan => "cyan".into(),
        Color::DarkGrey => "grey".into(),
        _ => "default".into(),
    }
}

#[cfg(feature = "native-ops")]
pub fn parse_pack(value: &serde_json::Value) -> io::Result<BTreeMap<String,String>> {
    use unicode_width::UnicodeWidthStr;
    let object = value.as_object().ok_or_else(|| io::Error::other("icon pack must be a JSON object"))?;
    if object.len() > 48 { return Err(io::Error::other("maximum 48 imported extensions")); }
    let mut map = BTreeMap::new();
    for (ext, glyph) in object {
        if ext.is_empty() || ext.len() > 16 || !ext.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()) {
            return Err(io::Error::other("use lowercase extensions without a dot"));
        }
        let glyph = glyph.as_str().ok_or_else(|| io::Error::other("icon must be a string"))?;
        if glyph.len() > 8 || !(1..=2).contains(&glyph.width()) ||
            terminal_tree::display_name(std::ffi::OsStr::new(glyph)) != glyph ||
            glyph.trim() != glyph || glyph.chars().any(char::is_control) {
            return Err(io::Error::other("icon must be safe visible text of one or two cells"));
        }
        map.insert(ext.clone(),glyph.to_owned());
    }
    Ok(map)
}

#[cfg(feature = "native-ops")]
impl Preferences {
    pub fn encode(&self, icons: Style, preview: bool, view: &str, zoom: usize) -> serde_json::Value {
        serde_json::json!({
            "version":1, "foreground":encode_color(self.foreground), "background":encode_color(self.background),
            "accent":encode_color(self.accent), "selection":encode_color(self.selection),
            "sidebar":self.sidebar, "toolbar":self.toolbar, "show_icons":self.show_icons,
            "file_colors":self.file_colors, "status_details":self.status_details,
            "companions":self.companions, "custom":self.custom, "icons":icons.name(), "preview":preview,
            "view":view, "zoom":zoom
        })
    }

    pub fn decode(v: &serde_json::Value) -> io::Result<(Self,Style,bool,String,usize)> {
        if v["version"].as_u64() != Some(1) { return Err(io::Error::other("unsupported application settings")); }
        let boolean = |key: &str| v[key].as_bool().ok_or_else(|| io::Error::other("invalid display toggle"));
        let color = |key: &str| parse_color(v[key].as_str().ok_or_else(|| io::Error::other("invalid color"))?);
        let list = v["companions"].as_array().filter(|a| a.len()==3).ok_or_else(|| io::Error::other("invalid companions"))?;
        let mut companions = [false;3];
        for (i,item) in list.iter().enumerate() { companions[i] = item.as_bool().ok_or_else(|| io::Error::other("invalid companion toggle"))?; }
        let icons = match v["icons"].as_str() {
            Some("Unicode") => Style::Unicode, Some("Seti") => Style::Seti, Some("ASCII") => Style::Ascii,
            _ => return Err(io::Error::other("invalid icon style")),
        };
        let view = v["view"].as_str().filter(|s| matches!(*s,"Tree"|"Grid"|"List")).ok_or_else(|| io::Error::other("invalid view"))?;
        let zoom = v["zoom"].as_u64().filter(|n| (1..=3).contains(n)).ok_or_else(|| io::Error::other("invalid zoom"))? as usize;
        Ok((Self {
            foreground:color("foreground")?, background:color("background")?,
            accent:color("accent")?, selection:color("selection")?,
            sidebar:boolean("sidebar")?, toolbar:boolean("toolbar")?, show_icons:boolean("show_icons")?,
            file_colors:boolean("file_colors")?, status_details:boolean("status_details")?,
            companions, custom:parse_pack(&v["custom"])?,
        }, icons, boolean("preview")?, view.to_owned(), zoom))
    }
}

#[cfg(all(test,feature = "native-ops"))]
mod tests {
    use super::*;
    #[test]
    fn preferences_round_trip_and_reject_invalid_pack() {
        let mut p = Preferences::default();
        p.foreground = parse_color("#abcdef").unwrap();
        p.companions[1] = false;
        p.custom = parse_pack(&serde_json::json!({"rs":"R","zip":"Z"})).unwrap();
        let value = p.encode(Style::Seti,false,"Grid",3);
        let (q,style,preview,view,zoom) = Preferences::decode(&value).unwrap();
        assert_eq!(q.foreground,p.foreground);
        assert_eq!(q.companions,p.companions);
        assert_eq!(q.icon(Path::new("a.rs"),false,false,style).0,"R");
        assert_ne!(q.icon(Path::new("a.rs"),true,false,style).0,"R");
        assert!(!preview);
        assert_eq!((view.as_str(),zoom),("Grid",3));
        assert!(parse_pack(&serde_json::json!({"rs":"\u{1b}[m"})).is_err());
        assert!(parse_pack(&serde_json::json!({".rs":"R"})).is_err());
        assert!(parse_pack(&serde_json::json!({"rs":"four"})).is_err());
        assert!(parse_color("#12zz34").is_err());
    }
}
