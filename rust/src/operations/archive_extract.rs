//! Extract a fully validated portable USTAR into a new reviewed directory.
//! Private staging and no-replace publication. No metadata restoration or replay.
use super::{
    create_dir_at, dir_id, fd_path, no_conflict, open_dir, reject_mounts, require, unique,
    Destination, Engine,
};
use crate::archive::{list, portable_layout_preview, Kind};
use crate::archive::native::read_archive_bytes;
use rustix::fs::{self as rx, AtFlags, Mode, OFlags, RenameFlags};
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path};

const BLOCK: usize = 512;

fn descend(root: &File, path: &Path) -> io::Result<File> {
    let mut dir = root.try_clone()?;
    for component in path.components() {
        match component {
            Component::Normal(name) => {
                dir = rx::openat(
                    &dir,
                    name,
                    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                    Mode::empty(),
                )?
                .into();
            }
            _ => return Err(super::fail("invalid staged directory component")),
        }
    }
    Ok(dir)
}

fn file_parent(root: &File, path: &Path) -> io::Result<File> {
    descend(
        root,
        path.parent()
            .ok_or_else(|| super::fail("missing staged parent"))?,
    )
}

impl Engine {
    /// Read one selected archive under the current engine scope. Verify the
    /// complete portable stream and observed source identity before returning
    /// immutable bytes for review; never open an extraction destination here.
    pub fn read_archive_for_extraction(&self, path: &Path) -> io::Result<Vec<u8>> {
        let path = self.path(path)?;
        reject_mounts(&path)?;
        let bytes = read_archive_bytes(&path)?;
        portable_layout_preview(bytes.as_slice())?;
        Ok(bytes)
    }

    /// Extract immutable archive bytes to one absent, reviewed destination.
    /// On failure preserve the pending record and any staging data for manual
    /// inspection. No existing tree is merged, replaced or removed.
    pub fn extract_archive_reviewed(
        &self,
        destination: &Destination,
        bytes: &[u8],
    ) -> io::Result<()> {
        self.extract_archive_reviewed_with(destination, bytes, &mut |_| Ok(()))
    }

    #[doc(hidden)]
    pub fn extract_archive_reviewed_with(
        &self,
        destination: &Destination,
        bytes: &[u8],
        checkpoint: &mut dyn FnMut(&str) -> io::Result<()>,
    ) -> io::Result<()> {
        let layout = portable_layout_preview(bytes)?;
        let members = list(bytes)?;
        // The complete stream has already been validated; derive byte ranges
        // in original member order without rescanning for every file.
        let mut offset = 0usize;
        let mut files = BTreeMap::new();
        for member in &members {
            let size = usize::try_from(member.size).map_err(io::Error::other)?;
            let start = offset + BLOCK;
            let end = start
                .checked_add(size)
                .ok_or_else(|| super::fail("archive payload overflow"))?;
            let data = bytes
                .get(start..end)
                .ok_or_else(|| super::fail("archive payload missing"))?;
            if member.kind == Kind::File {
                files.insert(Path::new(&member.name), data);
            }
            offset = end + (BLOCK - size % BLOCK) % BLOCK;
        }
        let dst = self.path(&destination.path)?;
        require(dst == destination.path, "destination changed")?;
        let parent = self.parent_for(&dst)?;
        require(
            dir_id(&parent)? == destination.parent,
            "reviewed destination replaced",
        )?;
        let name = dst.file_name().unwrap();
        no_conflict(&fd_path(&parent).join(name))?;
        let id = unique()?;
        let stage_name = format!(".terminal-tree-copy-{id}");
        let stage_path = dst.parent().unwrap().join(&stage_name);
        self.log(&id, "archive-extract", None, Some(&dst), Some(&stage_path))?;
        checkpoint("extract-journal")?;
        self.check_parent(&dst, &parent)?;
        self.check_journal()?;
        let stage = create_dir_at(&parent, OsStr::new(&stage_name))?;
        let payload = create_dir_at(&stage, OsStr::new("payload"))?;
        for directory in &layout.directories {
            let path = Path::new(directory);
            let p = file_parent(&payload, path)?;
            create_dir_at(&p, path.file_name().unwrap())?;
            checkpoint("extract-directory")?;
        }
        for (path, data) in &files {
            let p = file_parent(&payload, path)?;
            let mut f: File = rx::openat(
                &p,
                path.file_name().unwrap(),
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::from_bits_truncate(0o600),
            )?
            .into();
            for chunk in data.chunks(1024 * 1024) {
                f.write_all(chunk)?;
                checkpoint("extract-data")?;
            }
            f.sync_all()?;
            p.sync_all()?;
        }
        payload.sync_all()?;
        stage.sync_all()?;
        checkpoint("extract-before-verify")?;
        reject_mounts(&stage_path)?;
        let payload_path = fd_path(&stage).join("payload");
        require(
            dir_id(&open_dir(&payload_path)?)? == dir_id(&payload)?,
            "extraction payload replaced",
        )?;
        let before = super::manifest::snapshot(&payload_path)?;
        require(
            before.len() == 1 + layout.directories.len() + files.len()
                && before.contains_key(Path::new("."))
                && layout
                    .directories
                    .iter()
                    .all(|d| before.contains_key(&Path::new(".").join(d)))
                && files
                    .keys()
                    .all(|p| before.contains_key(&Path::new(".").join(p))),
            "extraction layout differs",
        )?;
        for directory in &layout.directories {
            descend(&payload, Path::new(directory))?;
        }
        for (path, expected) in &files {
            let p = file_parent(&payload, path)?;
            let name = path.file_name().unwrap();
            let stat = rx::statat(&p, name, AtFlags::SYMLINK_NOFOLLOW)?;
            require(
                rx::FileType::from_raw_mode(stat.st_mode) == rx::FileType::RegularFile,
                "staged file type changed",
            )?;
            let file: File = rx::openat(
                &p,
                name,
                OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
                Mode::empty(),
            )?
            .into();
            let m = file.metadata()?;
            require(
                m.is_file()
                    && m.dev() == stat.st_dev
                    && m.ino() == stat.st_ino
                    && m.nlink() == 1
                    && m.len() == expected.len() as u64,
                "staged file identity changed",
            )?;
            let mut actual = Vec::with_capacity(expected.len());
            file.take(expected.len() as u64 + 1)
                .read_to_end(&mut actual)?;
            require(actual.as_slice() == *expected, "staged payload differs")?;
        }
        checkpoint("extract-before-publish")?;
        self.check_journal()?;
        self.check_parent(&dst, &parent)?;
        reject_mounts(&stage_path)?;
        require(
            dir_id(&open_dir(&fd_path(&parent).join(&stage_name))?)? == dir_id(&stage)?
                && dir_id(&open_dir(&payload_path)?)? == dir_id(&payload)?
                && super::manifest::snapshot(&payload_path)? == before,
            "verified extraction changed before publication",
        )?;
        stage.sync_all()?;
        parent.sync_all()?;
        checkpoint("extract-commit")?;
        rx::renameat_with(&stage, "payload", &parent, name, RenameFlags::NOREPLACE)?;
        checkpoint("extract-before-sync")?;
        parent.sync_all()?;
        stage.sync_all()?;
        checkpoint("extract-after-sync")?;
        require(
            dir_id(&open_dir(&fd_path(&parent).join(&stage_name))?)? == dir_id(&stage)?,
            "extraction stage replaced",
        )?;
        rx::unlinkat(&parent, &stage_name, AtFlags::REMOVEDIR)?;
        parent.sync_all()?;
        self.complete(&id)
    }
}
