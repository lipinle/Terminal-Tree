//! Explicit read-only companion commands. Output is display data, never an
//! operation plan. No shell, PATH lookup, automatic installation or extraction.
use crate::{display_name, readonly_file::ReadSession};
use std::{
    ffi::{OsStr, OsString},
    fs,
    io::{self, Read},
    os::unix::{ffi::OsStrExt, fs::PermissionsExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

const MAX_OUTPUT: usize = 256 * 1024;
const MAX_LINES: usize = 4096;

#[derive(Clone, Copy, Debug)]
pub enum Tool {
    Eza,
    SevenZip,
    Git,
}

impl Tool {
    fn names(self) -> &'static [&'static str] {
        match self {
            Self::Eza => &["eza"],
            Self::SevenZip => &["7zz", "7z"],
            Self::Git => &["git"],
        }
    }

    pub fn installed(self) -> bool {
        resolve(self).is_ok()
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Eza => "eza",
            Self::SevenZip => "7-Zip",
            Self::Git => "Git",
        }
    }
}

fn executable(name: &str) -> Option<PathBuf> {
    // Only a distribution-installed executable; never search a project or PATH.
    let path = Path::new("/usr/bin").join(name);
    let meta = fs::metadata(&path).ok()?;
    (meta.is_file() && meta.permissions().mode() & 0o111 != 0).then_some(path)
}

fn resolve(tool: Tool) -> io::Result<PathBuf> {
    tool.names()
        .iter()
        .find_map(|name| executable(name))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "{} is not installed in /usr/bin; install the distribution package",
                    tool.label()
                ),
            )
        })
}

pub fn availability() -> Vec<String> {
    [Tool::Eza, Tool::SevenZip, Tool::Git]
        .into_iter()
        .map(|tool| match resolve(tool) {
            Ok(path) => format!("{}: {}", tool.label(), display_name(path.as_os_str())),
            Err(_) => format!("{}: not installed", tool.label()),
        })
        .collect()
}

fn safe_lines(bytes: &[u8]) -> io::Result<Vec<String>> {
    let mut lines = Vec::new();
    for line in bytes.split(|&b| b == b'\n') {
        if lines.len() == MAX_LINES {
            return Err(io::Error::other("companion line budget exceeded"));
        }
        lines.push(display_name(OsStr::from_bytes(line)));
    }
    Ok(lines)
}

fn run(program: &Path, arguments: &[OsString], directory: &Path) -> io::Result<Vec<String>> {
    safe_lines(&run_bytes(program, arguments, directory)?)
}
pub(crate) fn run_bytes(
    program: &Path,
    arguments: &[OsString],
    directory: &Path,
) -> io::Result<Vec<u8>> {
    let timeout = executable("timeout")
        .ok_or_else(|| io::Error::other("GNU timeout is required for companion tools"))?;
    let mut child = Command::new(timeout)
        .args(["--kill-after=1s", "8s"])
        .arg(program)
        .args(arguments)
        .current_dir(directory)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LC_ALL", "C")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_NO_LAZY_FETCH", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let mut output = Vec::new();
    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        child.wait()?;
        return Err(io::Error::other("companion output pipe unavailable"));
    };
    let result = stdout
        .take((MAX_OUTPUT + 1) as u64)
        .read_to_end(&mut output);
    // Dropping the pipe stops consuming excess output; timeout bounds the
    // foreground process. Always reap the supervising process.
    let status = child.wait()?;
    result?;
    if output.len() > MAX_OUTPUT {
        return Err(io::Error::other("companion output budget exceeded"));
    }
    if !status.success() {
        return Err(io::Error::other(format!(
            "companion failed or exceeded time limit ({status})"
        )));
    }
    Ok(output)
}

fn directory(path: &Path) -> io::Result<()> {
    if !path.is_absolute() || !fs::symlink_metadata(path)?.is_dir() {
        return Err(io::Error::other("select an absolute non-link directory"));
    }
    Ok(())
}

fn args(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}

pub fn eza(path: &Path) -> io::Result<Vec<String>> {
    directory(path)?;
    let mut arguments = args(&[
        "--long",
        "--group-directories-first",
        "--color=never",
        "--icons=never",
        "--",
    ]);
    arguments.push(path.as_os_str().to_owned());
    run(&resolve(Tool::Eza)?, &arguments, Path::new("/"))
}

pub fn archive(path: &Path) -> io::Result<Vec<String>> {
    let session = ReadSession::open(path)?;
    if session.len() > 512 * 1024 * 1024 {
        return Err(io::Error::other("archive input budget exceeded (512 MiB)"));
    }
    let mut arguments = args(&["l", "-slt", "-bd", "-sccUTF-8", "-pTT-no-password", "--"]);
    arguments.push(path.as_os_str().to_owned());
    let lines = run(&resolve(Tool::SevenZip)?, &arguments, Path::new("/"))?;
    session.verify()?;
    Ok(lines)
}

/// Explicit use in a trusted repository only: local Git configuration and
/// attributes can request external helpers. This is not a sandbox.
pub fn git_status(path: &Path) -> io::Result<Vec<String>> {
    directory(path)?;
    let arguments = args(&[
        "--no-optional-locks",
        "-c",
        "core.fsmonitor=false",
        "-c",
        "core.hooksPath=/dev/null",
        "-c",
        "submodule.recurse=false",
        "-c",
        "core.quotePath=true",
        "status",
        "--porcelain=v1",
        "--branch",
        "--ignore-submodules=all",
        "--untracked-files=normal",
    ]);
    run(&resolve(Tool::Git)?, &arguments, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_escapes_terminal_controls_and_bounds_lines() {
        let lines = safe_lines(b"a\x1b[31m\ninvalid\xff").unwrap();
        assert!(!lines[0].contains('\x1b'));
        assert!(lines[1].contains("\\xff"));
        assert!(safe_lines(&vec![b'\n'; MAX_LINES]).is_err());
    }
}
