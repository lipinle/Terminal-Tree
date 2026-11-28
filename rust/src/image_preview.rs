//! Bounded, dependency-free P6 PPM image thumbnails for plain terminals.
//! No external decoder, file opening, escape sequences or image protocol.
use std::io;

#[cfg(all(feature = "native-ops", target_os = "linux"))]
pub mod native;

const MAX_HEADER: usize = 4096;
const MAX_DIMENSION: usize = 4096;
const MAX_PIXELS: usize = 4 * 1024 * 1024;
const MAX_COLUMNS: usize = 80;
const MAX_ROWS: usize = 40;
const SHADES: &[u8] = b" .:-=+*#%@";

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\r' | b'\n' | 0x0b | 0x0c)
}

struct Header<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Header<'a> {
    fn peek(&self) -> Option<u8> {
        if self.pos >= MAX_HEADER {
            return None;
        }
        self.bytes.get(self.pos).copied()
    }

    fn token(&mut self) -> io::Result<&'a [u8]> {
        loop {
            match self.peek() {
                Some(b) if space(b) => self.pos += 1,
                Some(b'#') => {
                    while let Some(b) = self.peek() {
                        self.pos += 1;
                        if b == b'\n' || b == b'\r' {
                            break;
                        }
                    }
                }
                Some(_) => break,
                None => return Err(invalid("truncated or oversized PPM header")),
            }
        }
        let start = self.pos;
        while let Some(b) = self.peek() {
            if space(b) || b == b'#' {
                break;
            }
            self.pos += 1;
        }
        if start == self.pos || self.peek().is_none() {
            return Err(invalid("invalid PPM token"));
        }
        Ok(&self.bytes[start..self.pos])
    }

    fn number(&mut self) -> io::Result<usize> {
        let token = self.token()?;
        if token.len() > 10 || !token.iter().all(u8::is_ascii_digit) {
            return Err(invalid("invalid PPM number"));
        }
        std::str::from_utf8(token)
            .ok()
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| invalid("PPM number overflow"))
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Thumbnail {
    pub width: usize,
    pub height: usize,
    pub columns: usize,
    pub rows: Vec<String>,
}

/// Render one strict P6 image with maxval 255 as printable ASCII.
/// Only one whitespace byte separates the header and raster; reject
/// trailing bytes, multiple images, oversized headers and unsupported forms.
/// The caller supplies the input slice; this function allocates only a
/// bounded thumbnail and does not read a file or authenticate its contents.
pub fn ppm_thumbnail(bytes: &[u8], columns: usize, rows: usize) -> io::Result<Thumbnail> {
    if !(1..=MAX_COLUMNS).contains(&columns) || !(1..=MAX_ROWS).contains(&rows) {
        return Err(invalid("invalid image thumbnail bounds"));
    }
    if !bytes.starts_with(b"P6") {
        return Err(invalid("only binary P6 PPM images are supported"));
    }
    let mut header = Header { bytes, pos: 0 };
    if header.token()? != b"P6" {
        return Err(invalid("invalid PPM magic"));
    }
    let width = header.number()?;
    let height = header.number()?;
    if !(1..=MAX_DIMENSION).contains(&width)
        || !(1..=MAX_DIMENSION).contains(&height)
        || width * height > MAX_PIXELS
    {
        return Err(invalid("PPM image dimensions exceed limits"));
    }
    if header.number()? != 255 {
        return Err(invalid("only 8-bit PPM samples are supported"));
    }
    if !header.peek().is_some_and(space) {
        return Err(invalid("missing PPM raster separator"));
    }
    header.pos += 1;
    let raster = &bytes[header.pos..];
    if raster.len() != width * height * 3 {
        return Err(invalid("PPM raster length mismatch"));
    }

    // Approximate a terminal cell as twice as tall as it is wide.
    // Never exceed the requested output or original horizontal resolution.
    let mut out_width = width.min(columns);
    let mut out_height = (height * out_width).div_ceil(width * 2).max(1);
    if out_height > rows {
        out_height = rows;
        out_width = (width * out_height * 2 / height).clamp(1, out_width);
    }
    let mut lines = Vec::with_capacity(out_height);
    for y in 0..out_height {
        let mut line = String::with_capacity(out_width);
        for x in 0..out_width {
            let source_y = y * height / out_height;
            let source_x = x * width / out_width;
            let i = (source_y * width + source_x) * 3;
            let luminance = (299 * usize::from(raster[i])
                + 587 * usize::from(raster[i + 1])
                + 114 * usize::from(raster[i + 2]))
                / 1000;
            let shade = luminance * (SHADES.len() - 1) / 255;
            line.push(char::from(SHADES[shade]));
        }
        lines.push(line);
    }
    Ok(Thumbnail {
        width,
        height,
        columns: out_width,
        rows: lines,
    })
}
