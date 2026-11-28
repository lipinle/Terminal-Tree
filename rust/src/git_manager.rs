//! Explicit local Git workflow, separate from scoped filesystem operations.
//! Repository configuration and filters may execute programs; trusted repos only.
use crate::{display_name, external_tools};
use std::{
    ffi::{OsStr, OsString},
    io,
    os::unix::ffi::{OsStrExt, OsStringExt},
    path::{Component, Path, PathBuf},
};
fn err(s: &str) -> io::Error {
    io::Error::other(s)
}
fn args(s: &[&str]) -> Vec<OsString> {
    s.iter().map(OsString::from).collect()
}
fn run(path: &Path, command: &[OsString]) -> io::Result<Vec<u8>> {
    let mut a = args(&[
        "--no-pager",
        "--literal-pathspecs",
        "-c",
        "core.fsmonitor=false",
        "-c",
        "core.hooksPath=/dev/null",
        "-c",
        "submodule.recurse=false",
        "-c",
        "color.ui=false",
        "-c",
        "commit.gpgSign=false",
    ]);
    a.extend_from_slice(command);
    external_tools::run_bytes(Path::new("/usr/bin/git"), &a, path)
}
fn lines(data: &[u8]) -> Vec<String> {
    data.split(|b| *b == b'\n')
        .take(4096)
        .map(|s| display_name(OsStr::from_bytes(s)))
        .collect()
}
#[derive(Debug, Clone)]
pub struct Change {
    pub index: u8,
    pub work: u8,
    pub path: PathBuf,
}
impl Change {
    pub fn staged(&self) -> bool {
        self.index != b' ' && self.index != b'?'
    }
    pub fn unstaged(&self) -> bool {
        self.work != b' ' || self.index == b'?'
    }
    pub fn conflict(&self) -> bool {
        matches!(
            [self.index, self.work],
            [b'D', b'D']
                | [b'A', b'U']
                | [b'U', b'D']
                | [b'U', b'A']
                | [b'D', b'U']
                | [b'A', b'A']
                | [b'U', b'U']
        )
    }
    pub fn label(&self) -> String {
        format!(
            "{}{}  {}",
            char::from(self.index),
            char::from(self.work),
            display_name(self.path.as_os_str())
        )
    }
}
fn parse(data: &[u8]) -> io::Result<Vec<Change>> {
    if !data.is_empty() && !data.ends_with(&[0]) {
        return Err(err("truncated Git status"));
    }
    let mut changes = Vec::new();
    if data.is_empty() {
        return Ok(changes);
    }
    for row in data[..data.len() - 1].split(|b| *b == 0) {
        if row.len() < 4
            || row[2] != b' '
            || !b" MADRCU?!T".contains(&row[0])
            || !b" MADRCU?!T".contains(&row[1])
            || matches!(row[0], b'R' | b'C')
            || matches!(row[1], b'R' | b'C')
        {
            return Err(err("unsupported Git status entry"));
        }
        let path = PathBuf::from(OsString::from_vec(row[3..].to_vec()));
        if path.as_os_str().is_empty()
            || !path.components().all(|c| matches!(c, Component::Normal(_)))
            || changes.len() >= 4096
        {
            return Err(err("invalid or excessive Git paths"));
        }
        changes.push(Change {
            index: row[0],
            work: row[1],
            path,
        });
    }
    Ok(changes)
}
pub struct Repo {
    root: PathBuf,
}
impl Repo {
    pub fn discover(path: &Path) -> io::Result<Self> {
        let bytes = run(path, &args(&["rev-parse", "--show-toplevel"]))?;
        let raw = bytes
            .strip_suffix(b"\n")
            .ok_or_else(|| err("invalid repository root"))?;
        let root = PathBuf::from(OsString::from_vec(raw.to_vec()));
        if !root.is_absolute() || !root.is_dir() || root.canonicalize()? != root {
            return Err(err(
                "repository root must be an absolute canonical directory",
            ));
        }
        Ok(Self { root })
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    fn cmd(&self, a: &[&str]) -> io::Result<Vec<String>> {
        Ok(lines(&run(&self.root, &args(a))?))
    }
    pub fn status(&self) -> io::Result<Vec<Change>> {
        parse(&run(
            &self.root,
            &args(&[
                "--no-optional-locks",
                "status",
                "--porcelain=v1",
                "-z",
                "--no-renames",
                "--untracked-files=all",
                "--ignore-submodules=all",
            ]),
        )?)
    }
    pub fn branch(&self) -> io::Result<Vec<String>> {
        self.cmd(&[
            "--no-optional-locks",
            "status",
            "--short",
            "--branch",
            "--untracked-files=no",
            "--ignore-submodules=all",
        ])
    }
    pub fn history(&self) -> io::Result<Vec<String>> {
        self.cmd(&["log", "-30", "--date=short", "--format=%h %ad %s"])
    }
    pub fn branches(&self) -> io::Result<Vec<String>> {
        self.cmd(&["branch", "--list", "--no-color", "--no-column"])
    }
    pub fn diff(&self, change: &Change, staged: bool) -> io::Result<Vec<String>> {
        let mut a = args(&[
            "diff",
            "--no-ext-diff",
            "--no-textconv",
            "--no-color",
            "--no-renames",
        ]);
        if staged {
            a.push("--cached".into());
        }
        a.push("--".into());
        a.push(change.path.as_os_str().to_owned());
        Ok(lines(&run(&self.root, &a)?))
    }
    fn check_write(&self) -> io::Result<()> {
        // Linked worktrees, bare repos and operations in progress require normal Git CLI.
        if Self::discover(&self.root)?.root != self.root {
            return Err(err("repository changed"));
        }
        let git = self.root.join(".git");
        if !std::fs::symlink_metadata(&git)?.is_dir() {
            return Err(err("writes require an ordinary .git directory"));
        }
        for name in [
            "MERGE_HEAD",
            "CHERRY_PICK_HEAD",
            "REVERT_HEAD",
            "rebase-merge",
            "rebase-apply",
            "sequencer",
            "BISECT_LOG",
            "index.lock",
        ] {
            if std::fs::symlink_metadata(git.join(name)).is_ok() {
                return Err(err("Git operation already in progress; use normal Git"));
            }
        }
        if self.status()?.iter().any(Change::conflict) {
            return Err(err(
                "resolve conflicts using normal Git before editing here",
            ));
        }
        Ok(())
    }
    pub fn stage(&self, change: &Change, staged: bool) -> io::Result<()> {
        self.check_write()?;
        let now = self.status()?;
        if !now
            .iter()
            .any(|c| c.path == change.path && c.index == change.index && c.work == change.work)
        {
            return Err(err("file status changed; refresh and review"));
        }
        let mut a = if staged {
            args(&["add", "--"])
        } else {
            args(&["restore", "--staged", "--"])
        };
        a.push(change.path.as_os_str().to_owned());
        run(&self.root, &a)?;
        Ok(())
    }
    pub fn staged_summary(&self) -> io::Result<Vec<String>> {
        self.cmd(&[
            "diff",
            "--cached",
            "--stat",
            "--no-ext-diff",
            "--no-textconv",
            "--no-renames",
        ])
    }
    pub fn commit(&self, message: &str, reviewed: &[Change]) -> io::Result<()> {
        self.check_write()?;
        if message.trim().is_empty()
            || message.len() > 4096
            || message.chars().any(char::is_control)
        {
            return Err(err("invalid commit message"));
        }
        let current: Vec<_> = self.status()?.into_iter().filter(Change::staged).collect();
        if current.is_empty()
            || current.len() != reviewed.len()
            || !current
                .iter()
                .zip(reviewed)
                .all(|(a, b)| a.path == b.path && a.index == b.index && a.work == b.work)
        {
            return Err(err("staged paths changed; review again"));
        }
        run(
            &self.root,
            &[OsString::from("commit"), "-m".into(), message.into()],
        )?;
        Ok(())
    }
    pub fn create_branch(&self, name: &str) -> io::Result<()> {
        self.check_write()?;
        if name.is_empty()
            || name.len() > 128
            || name.starts_with('-')
            || name.chars().any(|c| c.is_control() || c.is_whitespace())
        {
            return Err(err("invalid branch name"));
        }
        run(&self.root, &args(&["check-ref-format", "--branch", name]))?;
        run(&self.root, &args(&["branch", "--", name]))?;
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn raw_paths_are_preserved() {
        let c = parse(b" M a\nb\0?? -x\xff\0").unwrap();
        assert_eq!(c[0].path.as_os_str().as_bytes(), b"a\nb");
        assert_eq!(c[1].path.as_os_str().as_bytes(), b"-x\xff");
        assert!(c[0].unstaged());
        assert!(!c[0].staged());
        assert!(parse(b" M ../escape\0").is_err());
        assert!(parse(b" M missing").is_err());
        assert!(parse(b"R  old\0new\0").is_err());
    }
    #[test]
    #[ignore]
    fn local_workflow() {
        let root = std::env::temp_dir().join(format!("tt-git-manager-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        struct Cleanup(PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(root.clone());
        run(&root, &args(&["init", "-b", "main"])).unwrap();
        run(&root, &args(&["config", "user.name", "Fixture"])).unwrap();
        run(
            &root,
            &args(&["config", "user.email", "fixture@example.invalid"]),
        )
        .unwrap();
        std::fs::write(root.join("a"), "first").unwrap();
        let repo = Repo::discover(&root).unwrap();
        repo.stage(&repo.status().unwrap()[0], true).unwrap();
        repo.commit("first", &repo.status().unwrap()).unwrap();
        std::fs::write(root.join("a"), "second").unwrap();
        let change = repo.status().unwrap().remove(0);
        assert!(repo
            .diff(&change, false)
            .unwrap()
            .iter()
            .any(|s| s.contains("+second")));
        repo.stage(&change, true).unwrap();
        repo.stage(&repo.status().unwrap()[0], false).unwrap();
        assert!(!repo.status().unwrap()[0].staged());
        repo.create_branch("fixture-branch").unwrap();
        assert!(repo
            .branches()
            .unwrap()
            .iter()
            .any(|s| s.contains("fixture-branch")));
        assert!(repo.history().unwrap().iter().any(|s| s.contains("first")));
    }
}
