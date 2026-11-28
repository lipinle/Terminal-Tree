//! Companion tools menu shared by the single and dual navigators.
use std::{io, path::Path};
use terminal_tree::external_tools;

pub fn show(directory: &Path, selected: Option<&Path>) -> io::Result<()> {
    let mut labels = vec![
        "eza: detailed directory listing".into(),
        "7-Zip: inspect selected archive (read-only)".into(),
        "Git: status of trusted repository".into(),
        "Check installed tools".into(),
    ];
    if selected.is_none() {
        labels[1] = "7-Zip: select an archive first".into();
    }
    let Some(choice) = super::choose("Tools | Esc cancel", &labels)? else {
        return Ok(());
    };
    let (title, lines) = match choice {
        0 => ("eza directory listing", external_tools::eza(directory)?),
        1 => (
            "7-Zip archive information (read-only)",
            external_tools::archive(
                selected.ok_or_else(|| io::Error::other("select an archive"))?,
            )?,
        ),
        2 => {
            let labels = vec![
                "Cancel".into(),
                "I trust this repository; run Git status".into(),
            ];
            if super::choose(
                "Git local configuration may run helpers; trusted projects only",
                &labels,
            )? != Some(1)
            {
                return Ok(());
            }
            ("Git status", external_tools::git_status(directory)?)
        }
        _ => ("Companion tools", external_tools::availability()),
    };
    super::page(title, &lines)
}
