//! Companion tools menu shared by the single and dual navigators.
use std::{io, path::Path};
use terminal_tree::external_tools::{self, Tool};

pub fn show(directory: &Path, selected: Option<&Path>) -> io::Result<()> {
    show_filtered(directory, selected, [true; 3], &mut None)
}

pub fn show_filtered(
    directory: &Path,
    selected: Option<&Path>,
    enabled: [bool; 3],
    last: &mut Option<String>,
) -> io::Result<()> {
    let entries = [
        (Tool::Eza, "detailed directory listing"),
        (Tool::SevenZip, "inspect selected archive (read-only)"),
        (Tool::Git, "status of trusted repository"),
    ];
    let mut labels: Vec<_> = entries
        .iter()
        .enumerate()
        .map(|(i, (tool, description))| {
            let status = if !enabled[i] {
                "disabled in application settings"
            } else if tool.installed() {
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
        if !enabled[choice] {
            return super::page(
                "Companion disabled",
                &[format!(
                    "{} is disabled. Enable it in Add-ons.",
                    tool.label()
                )],
            );
        }
        if !tool.installed() {
            return installation_help();
        }
    }
    if choice >= entries.len() {
        return installation_help();
    }
    if choice == 2 {
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
    }
    use crossterm::{
        cursor::MoveTo,
        execute,
        style::Print,
        terminal::{Clear, ClearType},
    };
    let name = entries[choice].0.label();
    execute!(
        std::io::stdout(),
        MoveTo(0, 0),
        Clear(ClearType::All),
        Print(format!("Running {name} (foreground)"))
    )?;
    let result = match choice {
        0 => external_tools::eza(directory).map(|lines| ("eza directory listing", lines)),
        1 => selected
            .ok_or_else(|| io::Error::other("select an archive"))
            .and_then(external_tools::archive)
            .map(|lines| ("7-Zip archive information (read-only)", lines)),
        _ => external_tools::git_status(directory).map(|lines| ("Git status", lines)),
    };
    match result {
        Ok((title, lines)) => {
            *last = Some(format!("{name}: completed"));
            super::page(title, &lines)
        }
        Err(error) => {
            *last = Some(format!("{name}: failed"));
            Err(error)
        }
    }
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
