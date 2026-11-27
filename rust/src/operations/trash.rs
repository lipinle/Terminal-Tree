//! Experimental home Trash adapter for an explicit workspace.
//! GIO controls trash placement; it is not a guarantee of power-loss durability.
use super::{dir_id, fail, fd_path, identity, no_conflict, open_dir, require, same_parent,
    unique, reject_mounts, Destination, Engine, Identity, Review, Snapshot, Transfer};
use rustix::fs::{self as rx, AtFlags, Mode, OFlags};
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, Read};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

pub struct Trash<'a> {
    engine: &'a Engine,
    data_home: PathBuf,
    root: PathBuf,
}
#[derive(Debug)]
pub struct RestoreReview {
    item: Review,
    destination: Destination,
    info: PathBuf,
    info_identity: Identity,
    info_parent: (u64,u64),
    info_bytes: Vec<u8>,
}
#[derive(Debug)]
pub struct PurgeReview {
    item: Review,
    info: Option<(PathBuf, Identity, Vec<u8>)>,
    info_parent: (u64,u64),
}
#[derive(Debug, PartialEq, Eq)]
pub enum PurgeOutcome { Deleted, MetadataPending }
#[derive(Debug, PartialEq, Eq)]
pub enum RestoreOutcome { Restored, MetadataPending }

fn percent_decode(input: &[u8]) -> io::Result<PathBuf> {
    let mut bytes = Vec::new();
    let mut i = 0;
    let hex = |c| match c { b'0'..=b'9' => Some(c-b'0'), b'a'..=b'f' => Some(c-b'a'+10),
        b'A'..=b'F' => Some(c-b'A'+10), _ => None };
    while i < input.len() {
        if input[i] == b'%' {
            let hi = input.get(i+1).and_then(|&c| hex(c)).ok_or_else(|| fail("bad percent escape"))?;
            let lo = input.get(i+2).and_then(|&c| hex(c)).ok_or_else(|| fail("bad percent escape"))?;
            bytes.push(hi*16+lo); i+=3;
        } else { bytes.push(input[i]); i+=1; }
    }
    require(!bytes.contains(&0), "NUL in restore path")?;
    let p = PathBuf::from(OsString::from_vec(bytes.clone()));
    require(p.is_absolute() && p.file_name().is_some(), "absolute restore path required")?;
    let mut normalized = PathBuf::from("/");
    for c in p.components() {
        match c { Component::RootDir => (), Component::Normal(s) => normalized.push(s),
            _ => return Err(fail("non-normal restore path")) }
    }
    require(normalized.as_os_str().as_bytes() == bytes, "non-normal restore path")?;
    Ok(p)
}
fn parse_info(bytes: &[u8]) -> io::Result<PathBuf> {
    let mut lines = bytes.split(|b| *b == b'\n').map(|s| s.strip_suffix(b"\r").unwrap_or(s));
    require(lines.next() == Some(b"[Trash Info]"), "invalid Trash metadata header")?;
    let paths: Vec<_> = lines.filter_map(|s| s.strip_prefix(b"Path=")).collect();
    require(paths.len() == 1, "missing or ambiguous Trash path")?;
    percent_decode(paths[0])
}
fn info_file(parent: &File, name: &std::ffi::OsStr) -> io::Result<(Identity, Vec<u8>)> {
    let file: File = rx::openat(parent, name, OFlags::RDONLY | OFlags::NONBLOCK |
        OFlags::NOFOLLOW | OFlags::CLOEXEC, Mode::empty())?.into();
    let m = file.metadata()?;
    require(m.is_file() && m.uid() == rustix::process::getuid().as_raw()
        && m.len() <= 65536, "invalid Trash info file")?;
    let expected = identity(&m);
    let mut bytes = Vec::new();
    (&file).take(65537).read_to_end(&mut bytes)?;
    require(bytes.len() <= 65536 && identity(&file.metadata()?) == expected, "Trash info changed")?;
    Ok((expected, bytes))
}

fn children(snapshot: &Snapshot, rel: &Path) -> Vec<(OsString, PathBuf)> {
    snapshot.keys().filter_map(|p| {
        if p == rel { return None; }
        let parent = p.parent()?;
        if parent == rel { Some((p.file_name()?.to_os_string(), p.clone())) } else { None }
    }).collect()
}
fn names(f: &File) -> io::Result<Vec<OsString>> {
    let mut result = fs::read_dir(fd_path(f))?.map(|e| e.map(|x| x.file_name()))
        .collect::<io::Result<Vec<_>>>()?;
    result.sort();
    Ok(result)
}
fn check_entry(parent: &File, name: &std::ffi::OsStr, expected: &Identity) -> io::Result<fs::Metadata> {
    let m = fs::symlink_metadata(fd_path(parent).join(name))?;
    require(identity(&m) == *expected, "Trash entry changed")?;
    Ok(m)
}
fn preflight_purge(parent: &File, name: &std::ffi::OsStr, snapshot: &Snapshot, rel: &Path) -> io::Result<()> {
    let expected = snapshot.get(rel).ok_or_else(|| fail("entry not reviewed"))?;
    let m = check_entry(parent, name, expected)?;
    if m.is_dir() {
        let dir = open_dir(&fd_path(parent).join(name))?;
        require(identity(&dir.metadata()?) == *expected, "directory replaced")?;
        let expected_children = children(snapshot, rel);
        let mut expected_names: Vec<_> = expected_children.iter().map(|(n,_)| n.clone()).collect();
        expected_names.sort();
        require(names(&dir)? == expected_names, "unreviewed Trash children")?;
        for (n, r) in expected_children { preflight_purge(&dir, &n, snapshot, &r)?; }
    } else {
        // Removing one hard link changes sibling inode metadata; conservatively
        // refuse such purges before any entry has been removed.
        require(m.nlink() == 1, "hard-linked purge refused")?;
    }
    Ok(())
}
fn erase(parent: &File, name: &std::ffi::OsStr, snapshot: &Snapshot, rel: &Path, physical: &Path) -> io::Result<()> {
    reject_mounts(physical)?;
    let expected = snapshot.get(rel).ok_or_else(|| fail("entry not reviewed"))?;
    let m = check_entry(parent, name, expected)?;
    if m.is_dir() {
        let dir = open_dir(&fd_path(parent).join(name))?;
        require(identity(&dir.metadata()?) == *expected, "directory replaced")?;
        let expected_children = children(snapshot, rel);
        let mut expected_names: Vec<_> = expected_children.iter().map(|(n,_)| n.clone()).collect();
        expected_names.sort();
        require(names(&dir)? == expected_names, "unreviewed Trash children")?;
        for (n, r) in expected_children {
            erase(&dir, &n, snapshot, &r, &physical.join(&n))?;
        }
        require(names(&dir)?.is_empty(), "new Trash entries detected")?;
        dir.sync_all()?;
        let current = fs::symlink_metadata(fd_path(parent).join(name))?;
        require(current.dev() == m.dev() && current.ino() == m.ino() && current.is_dir(), "Trash directory replaced")?;
        rx::unlinkat(parent, name, AtFlags::REMOVEDIR)?;
    } else {
        require(m.nlink() == 1, "hard-linked purge refused")?;
        rx::unlinkat(parent, name, AtFlags::empty())?;
    }
    parent.sync_all()
}

impl<'a> Trash<'a> {
    pub fn new(engine: &'a Engine, data_home: &Path) -> io::Result<Self> {
        require(fs::symlink_metadata(data_home)?.is_dir(), "data home symlink refused")?;
        let data_home = data_home.canonicalize()?;
        require(data_home.starts_with(&engine.root), "Trash outside experimental workspace")?;
        let root = data_home.join("Trash");
        if let Ok(m) = fs::symlink_metadata(&root) { require(m.is_dir(), "Trash symlink refused")?; }
        Ok(Self { engine, data_home, root })
    }
    fn private_dir(&self, name: &str) -> io::Result<File> {
        require(self.root.canonicalize()? == self.root, "Trash root redirected")?;
        let path = self.root.join(name);
        let f = open_dir(&path)?;
        let m = f.metadata()?;
        require(m.uid() == rustix::process::getuid().as_raw() && m.mode() & 0o077 == 0,
            "private owned Trash directory required")?;
        Ok(f)
    }
    pub fn send(&self, item: &Review) -> io::Result<()> {
        require(!item.path.starts_with(&self.data_home) && !self.data_home.starts_with(&item.path),
            "managed Trash location protected")?;
        let parent = self.engine.check(item)?;
        let id = unique()?;
        self.engine.log(&id, "gio-trash", Some(&item.path), None, None)?;
        self.engine.check(item)?;
        let status = Command::new("gio").arg("trash").arg("--").arg(&item.path)
            .env("XDG_DATA_HOME", &self.data_home).stdin(Stdio::null())
            .stdout(Stdio::null()).stderr(Stdio::piped()).output()?;
        require(status.status.success(), "GIO trash failed; inspect pending journal")?;
        // GIO owns destination selection. No permanent-delete fallback.
        no_conflict(&item.path)?;
        same_parent(item.path.parent().unwrap(), &parent)?;
        parent.sync_all()?;
        self.engine.complete(&id)
    }
    pub fn review_restore(&self, path: &Path) -> io::Result<RestoreReview> {
        let files = self.private_dir("files")?;
        let info_parent = self.private_dir("info")?;
        let item = self.engine.review(path)?;
        require(item.path.parent() == Some(self.root.join("files").as_path())
            && item.parent == dir_id(&files)?, "select a top-level home Trash item")?;
        let mut name = item.path.file_name().unwrap().to_os_string();
        name.push(".trashinfo");
        let (info_identity, info_bytes) = info_file(&info_parent, &name)?;
        let original = parse_info(&info_bytes)?;
        require(!original.starts_with(&self.data_home), "cannot restore into managed data home")?;
        let destination = self.engine.destination(&original)?;
        require(destination.path == original, "redirected restore destination refused")?;
        Ok(RestoreReview { item, destination, info: self.root.join("info").join(name),
            info_identity, info_parent: dir_id(&info_parent)?, info_bytes })
    }
    fn check_info(&self, review: &RestoreReview) -> io::Result<File> {
        let parent = self.private_dir("info")?;
        require(dir_id(&parent)? == review.info_parent, "Trash info directory changed")?;
        let (id, bytes) = info_file(&parent, review.info.file_name().unwrap())?;
        require(id == review.info_identity && bytes == review.info_bytes, "Trash metadata changed")?;
        Ok(parent)
    }
    pub fn restore(&self, review: &RestoreReview) -> io::Result<RestoreOutcome> {
        self.check_info(review)?;
        let id = unique()?;
        self.engine.log(&id, "restore", Some(&review.item.path), Some(&review.destination.path), None)?;
        self.check_info(review)?;
        self.engine.transfer_reviewed(&review.item, &review.destination, Transfer::Move)?;
        let cleanup = (|| -> io::Result<()> {
            let parent = self.check_info(review)?;
            rx::unlinkat(&parent, review.info.file_name().unwrap(), AtFlags::empty())?;
            parent.sync_all()?;
            self.engine.complete(&id)
        })();
        // Data has already moved. Never present a cleanup error as permission to retry.
        Ok(if cleanup.is_ok() { RestoreOutcome::Restored } else { RestoreOutcome::MetadataPending })
    }
    pub fn review_purge(&self, path: &Path) -> io::Result<PurgeReview> {
        let files = self.private_dir("files")?;
        let info_parent = self.private_dir("info")?;
        let item = self.engine.review(path)?;
        require(item.path.parent() == Some(self.root.join("files").as_path())
            && item.parent == dir_id(&files)?, "permanent deletion only in top-level home Trash")?;
        preflight_purge(&files, item.path.file_name().unwrap(), &item.snapshot, Path::new("."))?;
        let mut name = item.path.file_name().unwrap().to_os_string(); name.push(".trashinfo");
        let info = match info_file(&info_parent, &name) {
            Ok((id, bytes)) => Some((self.root.join("info").join(&name), id, bytes)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => return Err(e),
        };
        Ok(PurgeReview { item, info, info_parent: dir_id(&info_parent)? })
    }
    fn check_purge_info(&self, review: &PurgeReview) -> io::Result<File> {
        let dir = self.private_dir("info")?;
        require(dir_id(&dir)? == review.info_parent, "Trash info directory changed")?;
        if let Some((p, id, bytes)) = &review.info {
            let actual = info_file(&dir, p.file_name().unwrap())?;
            require(actual.0 == *id && actual.1 == *bytes, "Trash metadata changed")?;
        } else {
            let mut name = review.item.path.file_name().unwrap().to_os_string(); name.push(".trashinfo");
            no_conflict(&fd_path(&dir).join(name))?;
        }
        Ok(dir)
    }
    pub fn purge(&self, review: &PurgeReview, confirmation: &str) -> io::Result<PurgeOutcome> {
        require(confirmation == "DELETE", "explicit DELETE confirmation required")?;
        let parent = self.engine.check(&review.item)?;
        let current = self.private_dir("files")?;
        require(dir_id(&parent)? == dir_id(&current)?, "Trash files directory changed")?;
        self.check_purge_info(review)?;
        let name = review.item.path.file_name().unwrap();
        preflight_purge(&parent, name, &review.item.snapshot, Path::new("."))?;
        let id = unique()?;
        self.engine.log(&id, "permanent-delete", Some(&review.item.path), None, None)?;
        self.engine.check(&review.item)?;
        self.check_purge_info(review)?;
        erase(&parent, name, &review.item.snapshot, Path::new("."), &review.item.path)?;
        let cleanup = (|| -> io::Result<()> {
            let info_parent = self.check_purge_info(review)?;
            if let Some((p, _, _)) = &review.info {
                rx::unlinkat(&info_parent, p.file_name().unwrap(), AtFlags::empty())?;
                info_parent.sync_all()?;
            }
            self.engine.complete(&id)
        })();
        Ok(if cleanup.is_ok() { PurgeOutcome::Deleted } else { PurgeOutcome::MetadataPending })
    }

}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strict_raw_path_decoding() {
        assert_eq!(parse_info(b"[Trash Info]\nPath=/tmp/a%20b%FF\n").unwrap().as_os_str().as_bytes(), b"/tmp/a b\xff");
        for raw in [b"/tmp/../etc".as_slice(), b"/tmp//x", b"/tmp/%00", b"/tmp/%xx", b"relative", b"/tmp/x/"] {
            assert!(percent_decode(raw).is_err());
        }
        assert!(parse_info(b"[Trash Info]\nPath=/tmp/a\nPath=/tmp/b\n").is_err());
    }
}
