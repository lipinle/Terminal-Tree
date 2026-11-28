//! Preview selection, bounded content loading and visual worker lifecycle.
use super::file_rows::{entry_type, human_size};
#[cfg(feature = "native-ops")]
use super::markdown::markdown_preview;
use super::State;
use terminal_tree::{display_name, Entry};

impl State {
    pub(super) fn preview(&mut self, entry: Option<&Entry>, width: u16, rows: usize) {
        let key = entry.map(|e| {
            let (size, modified) = self.metadata_cache.borrow_mut().get(e);
            (e.path.clone(), size.unwrap_or(e.size), modified)
        });
        if key != self.preview_key {
            self.preview_scroll = 0;
        }
        #[cfg(feature = "native-ops")]
        {
            let size = if crate::native_graphics::supported() && !self.graphics_failed {
                crate::native_graphics::pixel_bounds(
                    width,
                    rows.saturating_sub(2).min(u16::MAX as usize) as u16,
                )
            } else {
                (
                    u32::from(width).clamp(1, 160),
                    (rows.saturating_sub(2) * 2).clamp(1, 160) as u32,
                )
            };
            if key != self.preview_key {
                self.visual_page = 1;
            }
            if size != self.visual_size {
                self.visual_size = size;
                self.preview_lines.clear();
                self.preview_scroll = 0;
            }
        }
        #[cfg(not(feature = "native-ops"))]
        let _ = (width, rows);
        if key == self.preview_key && !self.preview_lines.is_empty() {
            return;
        }
        self.preview_key = key;
        self.preview_styles.clear();
        #[cfg(feature = "native-ops")]
        {
            self.preview_worker.cancel();
            self.content_worker.cancel();
            self.visual_frame = None;
            self.visual_bitmap = false;
            self.graphics_placed = false;
            if let Some(e) = entry.filter(|e| {
                self.preview_mode == 0
                    && !e.directory
                    && !e.symlink
                    && terminal_tree::visual_preview::format(&e.path).is_some()
            }) {
                if !self.prefs.visual_preview || !terminal_tree::visual_preview::installed() {
                    self.preview_lines = vec![
                        e.name.clone(),
                        if !terminal_tree::visual_preview::installed() {
                            "Visual Preview: not installed".into()
                        } else {
                            "Visual Preview: disabled".into()
                        },
                        "Open Add-ons (Shift+F11)".into(),
                        "Visual Preview installation help".into(),
                        "Alt+P for Info / Hex".into(),
                    ];
                    return;
                }
                if self.prefs.visual_preview {
                    let page = if terminal_tree::visual_preview::format(&e.path) == Some("pdf") {
                        self.visual_page
                    } else {
                        1
                    };
                    self.preview_worker.request(crate::preview_worker::Request {
                        path: e.path.clone(),
                        name: e.name.clone(),
                        page,
                        pdf: terminal_tree::visual_preview::format(&e.path) == Some("pdf"),
                        hd: (crate::native_graphics::supported() && !self.graphics_failed)
                            .then_some(self.visual_size),
                        fallback: (
                            u32::from(width).clamp(1, 160),
                            (rows.saturating_sub(2) * 2).clamp(1, 160) as u32,
                        ),
                    });
                    self.preview_lines = vec![e.name.clone(), "Loading visual preview...".into()];
                    return;
                }
            }
        }
        if self.preview_mode == 1 {
            self.preview_lines = match entry {
                None => vec!["No selection".into()],
                Some(e) => vec![
                    e.name.clone(),
                    format!("Type: {}", entry_type(e)),
                    format!("Size: {}", human_size(e.size)),
                    format!("Bytes: {}", e.size),
                    format!("Path: {}", display_name(e.path.as_os_str())),
                    if e.symlink {
                        "Symbolic link; not followed".into()
                    } else {
                        "Observed file metadata".into()
                    },
                ],
            };
            return;
        }
        self.preview_lines = match entry {
            None => vec!["No selection".into()],
            Some(e) if e.directory => vec![e.name.clone(), "Folder".into(), "Enter to open".into()],
            Some(e) if e.symlink => vec![
                e.name.clone(),
                "Symbolic link".into(),
                "Preview not followed".into(),
            ],
            Some(e) => {
                #[cfg(feature = "native-ops")]
                {
                    self.content_worker
                        .request(crate::preview_worker::ContentRequest {
                            path: e.path.clone(),
                            name: e.name.clone(),
                            mode: if self.preview_mode == 2 {
                                terminal_tree::preview::Mode::Hex
                            } else {
                                terminal_tree::preview::Mode::Auto
                            },
                            markdown: self.preview_mode == 0
                                && e.path
                                    .extension()
                                    .and_then(|x| x.to_str())
                                    .is_some_and(|x| {
                                        x.eq_ignore_ascii_case("md")
                                            || x.eq_ignore_ascii_case("markdown")
                                    }),
                            width: usize::from(width),
                        });
                    vec![e.name.clone(), "Loading content preview...".into()]
                }
                #[cfg(not(feature = "native-ops"))]
                vec![
                    e.name.clone(),
                    format!("{} bytes", e.size),
                    "Content preview requires native Linux".into(),
                ]
            }
        };
    }

    #[cfg(feature = "native-ops")]
    pub(super) fn poll_visual_preview(&mut self) {
        if let Some(done) = self.content_worker.poll() {
            self.preview_styles.clear();
            self.preview_lines = match done.result {
                Ok(mut lines) => {
                    if let Some(first) = lines.first_mut() {
                        *first = done.name;
                    }
                    if done.markdown && lines.get(1).is_some_and(|s| s != "Hex preview") {
                        let (formatted, styles) = markdown_preview(&lines, done.width);
                        self.preview_styles = styles;
                        formatted
                    } else {
                        lines
                    }
                }
                Err(error) => vec![
                    done.name,
                    display_name(std::ffi::OsStr::new(&error.to_string())),
                ],
            };
        }
        if let Some(done) = self.preview_worker.poll() {
            self.visual_bitmap = done.bitmap;
            self.visual_frame = None;
            self.graphics_placed = false;
            match done.result {
                Ok(frame) => {
                    let label = if done.pdf {
                        format!("Page {}/{} | Scroll | Alt+V", done.page, frame.pages)
                    } else {
                        format!(
                            "Thumbnail {}x{} {} | Alt+V",
                            frame.width,
                            frame.height,
                            if done.bitmap { "RGB" } else { "" }
                        )
                    };
                    self.preview_lines = vec![done.name, label];
                    self.visual_frame = Some(frame);
                }
                Err(error) => {
                    self.preview_lines = vec![
                        done.name,
                        display_name(std::ffi::OsStr::new(&error.to_string())),
                    ]
                }
            }
        }
    }

    #[cfg(feature = "native-ops")]
    pub(super) fn suspend_visual_preview(&mut self) {
        // Invalidate only if loading; cached completed previews may be reused.
        if self.preview_worker.busy() || self.content_worker.busy() {
            self.preview_key = None;
            self.preview_lines.clear();
        }
        self.preview_worker.cancel();
        let _ = self.preview_worker.poll(); // reap completed obsolete work
        self.content_worker.cancel();
        let _ = self.content_worker.poll();
    }
}
