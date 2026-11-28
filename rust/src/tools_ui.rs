//! Companion tools menu shared by the single and dual navigators.
use std::{io, path::Path};
use terminal_tree::external_tools::{self, Tool};

pub fn show(directory: &Path, selected: Option<&Path>) -> io::Result<()> {
    let entries = [
        (Tool::Eza, "detailed directory listing"),
        (Tool::SevenZip, "inspect selected archive (read-only)"),
        (Tool::Git, "status of trusted repository"),
    ];
    let mut labels: Vec<_> = entries
        .iter()
        .map(|(tool, description)| {
            let status = if tool.installed() {
                "available"
            } else {
                "optional; not installed"
            };
            format!("{}: {description} [{status}]", tool.label())
        })
        .collect();
    labels.push("Check installed tools / installation help".into());
    if selected.is_none() {
        labels[1].push_str(" — select an archive first");
    }
    let Some(choice) = super::choose("Tools | Esc cancel", &labels)? else {
        return Ok(());
    };
    if let Some((tool, _)) = entries.get(choice) {
        if !tool.installed() {
            return installation_help();
        }
    }
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
        _ => return installation_help(),
    };
    super::page(title, &lines)
}

fn installation_help() -> io::Result<()> {
    let mut lines = external_tools::availability();
    lines.extend([
        String::new(),
        "All companions are optional and independently installed.".into(),
        "Linux Mint / Ubuntu (package availability varies):".into(),
        "  sudo apt install eza 7zip git".into(),
        "  Older distributions may provide p7zip-full instead of 7zip.".into(),
        "Terminal Tree never installs or updates these programs automatically.".into(),
        "Native browsing and file operations work without them.".into(),
        "See OPTIONAL-TOOLS.md in the source repository for licenses.".into(),
    ]);
    super::page("Companion tools and installation", &lines)
}
