//! Explicit recovery copy: journal paths are evidence only. The operator
//! supplies approved paths, an ID and a new destination. Nothing is replayed.
use super::{
    attributes, create_dir_at, dir_id, fd_path, identity, manifest_with, no_conflict, open_dir,
    reject_mounts, require, snapshot, unique, Destination, Engine, Manifest, Review, Snapshot,
};
use crate::recovery_advice::{classify, id_valid, Advice};
use std::{
    ffi::OsStr,
    fs::{self, File},
    io,
    os::unix::{ffi::OsStrExt, fs::MetadataExt},
    path::{Path, PathBuf},
};

#[derive(Debug)]
pub struct RetainedRecovery {
    id: String,
    original: PathBuf,
    retained: PathBuf,
    parent: (u64, u64),
    directory: (u64, u64),
    manifest: Manifest,
    snapshot: Snapshot,
    published: Review,
}

impl RetainedRecovery {
    pub fn retained_path(&self) -> &Path {
        &self.retained
    }
}

fn eligible(file: &File) -> io::Result<()> {
    let m = file.metadata()?;
    require(
        m.is_file() && m.nlink() == 1 && m.mode() & 0o7000 == 0 && attributes(file)?.is_empty(),
        "retained recovery requires one ordinary file without extended attributes",
    )
}

impl Engine {
    fn retention_directory(&self, original: &Path, id: &str) -> io::Result<(File, File, PathBuf)> {
        require(id_valid(id), "invalid retained transaction ID")?;
        require(self.path(original)? == original, "original path changed")?;
        let parent = self.parent_for(original)?;
        let name = format!(".terminal-tree-copy-{id}");
        let path = original
            .parent()
            .ok_or_else(|| super::fail("missing parent"))?
            .join(&name);
        reject_mounts(&path)?;
        let dir = open_dir(&fd_path(&parent).join(&name))?;
        let m = dir.metadata()?;
        require(
            m.uid() == rustix::process::getuid().as_raw()
                && m.mode() & 0o7777 == 0o700
                && m.dev() == parent.metadata()?.dev(),
            "private same-device retention directory required",
        )?;
        self.check_parent(original, &parent)?;
        Ok((parent, dir, path.join("original")))
    }

    fn retained_record(&self, id: &str) -> io::Result<serde_json::Value> {
        require(id_valid(id), "invalid retained transaction ID")?;
        let pending = format!("{id}.pending.json");
        let done = format!("{id}.done.json");
        let a = fs::symlink_metadata(fd_path(&self.journal).join(&pending));
        let b = fs::symlink_metadata(fd_path(&self.journal).join(&done));
        let absent = |m: &io::Result<fs::Metadata>| matches!(m, Err(e) if e.kind() == io::ErrorKind::NotFound);
        let name = if a.is_ok() && absent(&b) {
            pending.as_str()
        } else if b.is_ok() && absent(&a) {
            done.as_str()
        } else {
            return Err(io::Error::other(
                "missing or ambiguous retained move record",
            ));
        };
        let record = self.read_recovery_record(OsStr::new(name))?;
        require(
            classify(OsStr::new(&pending), &record) == Advice::InspectRetainedMove,
            "not a structurally valid retained move record",
        )?;
        Ok(record)
    }

    /// Require an independently selected, existing published copy as well as
    /// the retained original. Matching current manifests are not historical
    /// proof of provenance; absent or conflicting evidence is refused.
    pub fn review_retained_recovery(
        &self,
        original: &Path,
        published: &Path,
        id: &str,
        checkpoint: &mut dyn FnMut(&str) -> io::Result<()>,
    ) -> io::Result<RetainedRecovery> {
        let original = self.path(original)?;
        let published = self.review_with(published, checkpoint)?;
        require(
            original != published.path,
            "original and published paths must differ",
        )?;
        let (parent, dir, retained) = self.retention_directory(&original, id)?;
        let record = self.retained_record(id)?;
        for (key, path) in [
            ("source_bytes_hex", original.as_path()),
            ("destination_bytes_hex", published.path()),
            ("stage_bytes_hex", retained.as_path()),
        ] {
            require(
                record[key].as_str() == Some(super::hex_path(path).as_str()),
                "operator paths differ from journal evidence",
            )?;
        }
        let source = fd_path(&dir).join("original");
        let file: File = super::rx::openat(
            &dir,
            "original",
            super::OFlags::RDONLY
                | super::OFlags::NOFOLLOW
                | super::OFlags::NONBLOCK
                | super::OFlags::CLOEXEC,
            super::Mode::empty(),
        )?
        .into();
        eligible(&file)?;
        let before = identity(&file.metadata()?);
        let (manifest, ids) = manifest_with(&source, false, checkpoint)?;
        require(
            ids.get(Path::new(".")) == Some(&before) && identity(&file.metadata()?) == before,
            "retained file changed during review",
        )?;
        eligible(&file)?;
        let rounded = record["operation"].as_str() == Some("retained-move-time-tolerance");
        require(
            manifest.matches_transfer(&published.manifest, rounded),
            "retained and published copies differ",
        )?;
        self.check(&published)?;
        let (parent_after, dir_after, _) = self.retention_directory(&original, id)?;
        require(
            dir_id(&parent)? == dir_id(&parent_after)? && dir_id(&dir)? == dir_id(&dir_after)?,
            "retention directory replaced",
        )?;
        self.check_journal()?;
        Ok(RetainedRecovery {
            id: id.into(),
            original,
            retained,
            parent: dir_id(&parent)?,
            directory: dir_id(&dir)?,
            manifest,
            snapshot: ids,
            published,
        })
    }

    fn check_retained_recovery(
        &self,
        review: &RetainedRecovery,
        checkpoint: &mut dyn FnMut(&str) -> io::Result<()>,
    ) -> io::Result<File> {
        let fresh = self.review_retained_recovery(
            &review.original,
            review.published.path(),
            &review.id,
            checkpoint,
        )?;
        require(
            fresh.parent == review.parent
                && fresh.directory == review.directory
                && fresh.manifest == review.manifest
                && fresh.snapshot == review.snapshot
                && fresh.published.identity == review.published.identity
                && fresh.published.parent == review.published.parent
                && fresh.published.manifest == review.published.manifest
                && fresh.published.snapshot == review.published.snapshot,
            "recovery evidence changed after review",
        )?;
        let (parent, dir, _) = self.retention_directory(&review.original, &review.id)?;
        require(
            dir_id(&parent)? == review.parent && dir_id(&dir)? == review.directory,
            "retention replaced",
        )?;
        Ok(dir)
    }

    /// Strict verified copy into a fresh ordinary destination. Preserve both
    /// pieces of evidence and every original journal entry; no deletion or
    /// replay authority comes from a journal record.
    pub fn copy_retained_recovery(
        &self,
        review: &RetainedRecovery,
        destination: &Destination,
        checkpoint: &mut dyn FnMut(&str) -> io::Result<()>,
    ) -> io::Result<()> {
        let dir = self.check_retained_recovery(review, checkpoint)?;
        let source = fd_path(&dir).join("original");
        let dst = self.path(destination.path())?;
        require(dst == destination.path, "recovery destination changed")?;
        require(
            dst != review.original && dst != review.published.path,
            "choose a new recovery path",
        )?;
        require(
            !dst.components().any(|part| {
                part.as_os_str()
                    .as_bytes()
                    .starts_with(b".terminal-tree-copy-")
            }),
            "managed recovery destination refused",
        )?;
        let parent = self.parent_for(&dst)?;
        require(
            dir_id(&parent)? == destination.parent,
            "recovery destination replaced",
        )?;
        let name = dst
            .file_name()
            .ok_or_else(|| super::fail("missing filename"))?;
        no_conflict(&fd_path(&parent).join(name))?;
        let id = unique()?;
        let stage_name = format!(".terminal-tree-copy-{id}");
        let stage_path = dst
            .parent()
            .ok_or_else(|| super::fail("missing parent"))?
            .join(&stage_name);
        checkpoint("recovery-prepare")?;
        self.log(
            &id,
            "retained-recovery-copy",
            Some(&review.retained),
            Some(&dst),
            Some(&stage_path),
        )?;
        let stage = create_dir_at(&parent, OsStr::new(&stage_name))?;
        let payload = fd_path(&stage).join("payload");
        let staged = super::copy_tree(
            &source,
            &payload,
            &review.manifest,
            &review.snapshot,
            false,
            checkpoint,
        )?;
        self.check_retained_recovery(review, checkpoint)?;
        checkpoint("copy-before-publish")?;
        self.check_parent(&dst, &parent)?;
        self.check_journal()?;
        reject_mounts(&stage_path)?;
        require(
            dir_id(&open_dir(&fd_path(&parent).join(&stage_name))?)? == dir_id(&stage)?,
            "recovery stage replaced",
        )?;
        require(
            snapshot(&payload)? == staged,
            "verified recovery payload changed",
        )?;
        stage.sync_all()?;
        parent.sync_all()?;
        checkpoint("recovery-commit")?;
        super::rx::renameat_with(
            &stage,
            "payload",
            &parent,
            name,
            super::RenameFlags::NOREPLACE,
        )?;
        checkpoint("recovery-before-sync")?;
        parent.sync_all()?;
        stage.sync_all()?;
        checkpoint("recovery-after-sync")?;
        require(
            dir_id(&open_dir(&fd_path(&parent).join(&stage_name))?)? == dir_id(&stage)?,
            "recovery stage replaced",
        )?;
        super::rx::unlinkat(&parent, &stage_name, super::AtFlags::REMOVEDIR)?;
        parent.sync_all()?;
        self.complete(&id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{symlink, DirBuilderExt};
    #[test]
    fn retained_recovery_requires_matching_evidence_and_never_overwrites() {
        let root =
            std::env::temp_dir().join(format!("tt-retained-recovery-{}", std::process::id()));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        struct Cleanup(PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(root.clone());
        let work = root.join("work");
        let journal = root.join("journal");
        fs::DirBuilder::new().mode(0o700).create(&work).unwrap();
        fs::DirBuilder::new().mode(0o700).create(&journal).unwrap();
        let engine = Engine::new(&work, &journal).unwrap();
        let original = work.join("source");
        let published = work.join("published");
        let id = unique().unwrap();
        let stage = work.join(format!(".terminal-tree-copy-{id}"));
        fs::DirBuilder::new().mode(0o700).create(&stage).unwrap();
        let retained = stage.join("original");
        fs::write(&retained, b"preserved").unwrap();
        fs::copy(&retained, &published).unwrap();
        File::open(&published)
            .unwrap()
            .set_times(
                fs::FileTimes::new()
                    .set_modified(fs::metadata(&retained).unwrap().modified().unwrap()),
            )
            .unwrap();
        engine
            .log(
                &id,
                "retained-move",
                Some(&original),
                Some(&published),
                Some(&retained),
            )
            .unwrap();
        engine.complete(&id).unwrap();
        let mut hook = |_: &str| Ok(());
        let review = engine
            .review_retained_recovery(&original, &published, &id, &mut hook)
            .unwrap();
        let output = work.join("recovered");
        let dst = engine.destination(&output).unwrap();
        fs::write(&output, b"occupied").unwrap();
        assert!(engine
            .copy_retained_recovery(&review, &dst, &mut hook)
            .is_err());
        assert_eq!(fs::read(&output).unwrap(), b"occupied");
        fs::remove_file(&output).unwrap();
        engine
            .copy_retained_recovery(&review, &dst, &mut hook)
            .unwrap();
        for path in [&output, &retained, &published] {
            assert_eq!(fs::read(path).unwrap(), b"preserved");
        }
        assert!(!original.exists());
        assert!(engine.pending().unwrap().is_empty());
        for (index, point) in [
            "copy-before-publish",
            "recovery-before-sync",
            "recovery-after-sync",
        ]
        .into_iter()
        .enumerate()
        {
            let path = work.join(format!("interrupted-{index}"));
            let destination = engine.destination(&path).unwrap();
            let result = engine.copy_retained_recovery(&review, &destination, &mut |checkpoint| {
                if checkpoint == point {
                    Err(io::Error::other("injected interruption"))
                } else {
                    Ok(())
                }
            });
            assert!(result.is_err());
            if index == 0 {
                assert!(!path.exists());
            } else {
                assert_eq!(fs::read(&path).unwrap(), b"preserved");
            }
            assert_eq!(fs::read(&retained).unwrap(), b"preserved");
            assert_eq!(fs::read(&published).unwrap(), b"preserved");
            assert!(!original.exists());
            assert_eq!(engine.pending().unwrap().len(), index + 1);
        }
        fs::write(&published, b"changed").unwrap();
        assert!(engine
            .review_retained_recovery(&original, &published, &id, &mut hook)
            .is_err());
        fs::rename(&stage, work.join("saved-stage")).unwrap();
        symlink(work.join("saved-stage"), &stage).unwrap();
        assert!(engine
            .review_retained_recovery(&original, &published, &id, &mut hook)
            .is_err());
    }
}
