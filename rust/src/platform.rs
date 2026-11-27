//! Desktop integration without a shell or a mandatory desktop environment.
use std::{
    io,
    path::Path,
    process::{Child, Command, Stdio},
};
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn spawn(program: &str, args: &[&str], path: &Path) -> io::Result<Child> {
    Command::new(program)
        .args(args)
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
}
pub fn open_file(path: &Path) -> io::Result<Child> {
    if !path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "absolute file path required",
        ));
    }
    #[cfg(target_os = "linux")]
    return match spawn("xdg-open", &[], path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            match spawn("gio", &["open", "--"], path) {
                Err(e) if e.kind() == io::ErrorKind::NotFound => Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "No desktop opener: install xdg-utils or GLib/GIO, or use file preview",
                )),
                other => other,
            }
        }
        other => other,
    };
    #[cfg(target_os = "macos")]
    return spawn("open", &["--"], path);
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "External file opening is not implemented for this platform preview",
    ))
}
#[cfg(test)]
mod tests {
    #[test]
    fn relative_path_is_not_passed_to_opener() {
        assert_eq!(
            super::open_file(std::path::Path::new("-option"))
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::InvalidInput
        );
    }
}

pub fn home_dir() -> Option<std::path::PathBuf> {
    let key = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(key)
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_absolute())
}
pub fn filesystem_root() -> io::Result<std::path::PathBuf> {
    let current = std::env::current_dir()?;
    current
        .ancestors()
        .filter(|p| p.is_absolute())
        .last()
        .map(Path::to_path_buf)
        .ok_or_else(|| io::Error::other("filesystem root unavailable"))
}
pub fn properties(path: &Path) -> io::Result<Vec<String>> {
    let m = std::fs::symlink_metadata(path)?;
    let mut lines = vec![terminal_tree::display_name(path.as_os_str())];
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        lines.extend([
            format!("Mode: {:o}", m.mode()),
            format!("Size: {} bytes", m.len()),
            format!("UID/GID: {}/{}", m.uid(), m.gid()),
            format!("Modified (Unix): {}.{:09}", m.mtime(), m.mtime_nsec()),
            format!("Device/inode: {}/{}", m.dev(), m.ino()),
            format!("Hard links: {}", m.nlink()),
        ]);
    }
    #[cfg(not(unix))]
    {
        lines.extend([
            format!("Size: {} bytes", m.len()),
            format!("Read-only attribute: {}", m.permissions().readonly()),
            format!("Modified: {:?}", m.modified().ok()),
            format!("Directory: {}", m.is_dir()),
        ]);
    }
    if m.file_type().is_symlink() {
        lines.push(format!(
            "Link: {}",
            terminal_tree::display_name(std::fs::read_link(path)?.as_os_str())
        ));
    }
    Ok(lines)
}
