//! Reviewed regular-file transfer across approved devices, retaining the original.
use super::{
    attributes, dir_id, fd_path, identity, no_conflict, require, Destination, Engine, Review,
};
use std::fs;
use std::io;
use std::ffi::OsStr;
use std::path::PathBuf;
use super::{create_dir_at, manifest, manifest_with, reject_mounts, unique, Transfer};
use std::os::unix::fs::MetadataExt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileTransferObservation {
    pub source_device: u64,
    pub destination_device: u64,
    pub bytes: u64,
}

impl FileTransferObservation {
    pub fn crosses_devices(self) -> bool {
        self.source_device != self.destination_device
    }
}

fn eligible(m: &fs::Metadata) -> io::Result<()> {
    require(
        m.is_file(),
        "retained-source preview requires a regular file",
    )?;
    require(m.nlink() == 1, "multiply linked source refused")?;
    require(m.mode() & 0o7000 == 0, "special permission bits refused")
}

impl Engine {
    /// Inspect one already reviewed regular file and unoccupied destination.
    /// Refuse links, special modes and extended attributes for this narrow
    /// candidate profile. This is observational only; no capability to write
    /// is returned, and filesystem support for preserving metadata is unknown.
    pub fn inspect_file_transfer(
        &self,
        review: &Review,
        destination: &Destination,
    ) -> io::Result<FileTransferObservation> {
        self.check_journal()?;
        let src_parent = self.check_identity(review)?;
        let src = fd_path(&src_parent).join(review.path.file_name().unwrap());
        let observed = fs::symlink_metadata(&src)?;
        eligible(&observed)?;
        let dst = self.path(&destination.path)?;
        require(dst == destination.path, "destination path changed")?;
        let dst_parent = self.parent_for(&dst)?;
        require(
            dir_id(&dst_parent)? == destination.parent,
            "reviewed destination replaced",
        )?;
        no_conflict(&fd_path(&dst_parent).join(dst.file_name().unwrap()))?;
        // Use descriptor-relative no-follow acquisition and compare to the
        // reviewed identity before reading attributes or validating content.
        let file: fs::File = super::rx::openat(
            &src_parent,
            review.path.file_name().unwrap(),
            super::OFlags::RDONLY
                | super::OFlags::NOFOLLOW
                | super::OFlags::NONBLOCK
                | super::OFlags::CLOEXEC,
            super::Mode::empty(),
        )?
        .into();
        let opened = file.metadata()?;
        eligible(&opened)?;
        require(
            identity(&opened) == review.identity,
            "reviewed source changed",
        )?;
        require(attributes(&file)?.is_empty(), "extended attributes refused")?;
        self.check(review)?;
        let after = file.metadata()?;
        eligible(&after)?;
        require(
            identity(&after) == identity(&opened),
            "source changed during inspection",
        )?;
        require(attributes(&file)?.is_empty(), "extended attributes changed")?;
        self.check_parent(&review.path, &src_parent)?;
        self.check_parent(&dst, &dst_parent)?;
        no_conflict(&fd_path(&dst_parent).join(dst.file_name().unwrap()))?;
        self.check_journal()?;
        Ok(FileTransferObservation {
            source_device: opened.dev(),
            destination_device: dst_parent.metadata()?.dev(),
            bytes: opened.len(),
        })
    }
}

impl Engine {
    /// Copy and verify on the target filesystem before moving the original
    /// into a private sibling retention directory on its own filesystem.
    /// The original is never unlinked. A pending record on failure is evidence
    /// only: do not blindly retry. This is not an atomic cross-device move.
    pub fn move_retaining_source(
        &self,
        review: &Review,
        destination: &Destination,
        checkpoint: &mut dyn FnMut(&str) -> io::Result<()>,
    ) -> io::Result<PathBuf> {
        self.move_retaining_source_with_time_rounding(review, destination, false, checkpoint)
    }

    /// Explicit opt-in permits only up to two seconds of modification-time
    /// rounding. Content, permissions, ownership and attributes remain strict.
    pub fn move_retaining_source_with_time_rounding(
        &self,
        review: &Review,
        destination: &Destination,
        allow_time_rounding: bool,
        checkpoint: &mut dyn FnMut(&str) -> io::Result<()>,
    ) -> io::Result<PathBuf> {
        let observed = self.inspect_file_transfer(review, destination)?;
        require(observed.crosses_devices(), "different devices required")?;
        let source_parent = self.check(review)?;
        let target_parent = self.parent_for(destination.path())?;
        let source_name = review.path.file_name().unwrap();
        let target_name = destination.path.file_name().unwrap();
        let src = fd_path(&source_parent).join(source_name);
        let dst = fd_path(&target_parent).join(target_name);
        let id = unique()?;
        let retention_name = format!(".terminal-tree-copy-{id}");
        let retention = review.path.parent().unwrap().join(&retention_name);
        let retained = retention.join("original");
        no_conflict(&fd_path(&source_parent).join(&retention_name))?;
        checkpoint("retained-prepare")?;
        self.log(
            &id,
            if allow_time_rounding { "retained-move-time-tolerance" } else { "retained-move" },
            Some(&review.path),
            Some(&destination.path),
            Some(&retained),
        )?;
        // Reuse the independently journalled, verified, synced no-replace copy.
        // Any error up to this point leaves the original at its initial name.
        self.transfer_with_metadata_policy(
            review, destination, Transfer::Copy, allow_time_rounding, checkpoint,
        )?;
        checkpoint("retained-verify")?;
        let target = self.review(destination.path())?;
        require(
            review.manifest.matches_transfer(&target.manifest, allow_time_rounding),
            "published copy differs",
        )?;
        require(
            target.parent == destination.parent,
            "published destination parent changed",
        )?;
        let (content, ids) = manifest_with(&src, true, checkpoint)?;
        require(
            content == review.manifest && ids == review.snapshot,
            "original changed after copy",
        )?;
        self.check_identity(review)?;
        self.check(&target)?;
        self.check_parent(&review.path, &source_parent)?;
        self.check_parent(destination.path(), &target_parent)?;
        self.check_journal()?;
        let stage = create_dir_at(&source_parent, OsStr::new(&retention_name))?;
        reject_mounts(&retention)?;
        require(
            dir_id(&super::open_dir(&fd_path(&source_parent).join(&retention_name))?)?
                == dir_id(&stage)?,
            "retention directory replaced",
        )?;
        checkpoint("retained-before-publish")?;
        self.check_identity(review)?;
        self.check_identity(&target)?;
        self.check_parent(&review.path, &source_parent)?;
        self.check_parent(destination.path(), &target_parent)?;
        require(
            identity(&fs::symlink_metadata(&dst)?) == target.identity,
            "published target replaced",
        )?;
        reject_mounts(&retention)?;
        require(
            dir_id(&super::open_dir(&fd_path(&source_parent).join(&retention_name))?)?
                == dir_id(&stage)?,
            "retention directory replaced",
        )?;
        self.check_journal()?;
        source_parent.sync_all()?;
        stage.sync_all()?;
        checkpoint("retained-commit")?;
        super::rx::renameat_with(
            &source_parent,
            source_name,
            &stage,
            "original",
            super::RenameFlags::NOREPLACE,
        )?;
        checkpoint("retained-before-sync")?;
        stage.sync_all()?;
        source_parent.sync_all()?;
        checkpoint("retained-after-sync")?;
        let retained_fd_path = fd_path(&stage).join("original");
        let (retained_content, _) = manifest(&retained_fd_path, false)?;
        let actual = fs::symlink_metadata(&retained_fd_path)?;
        eligible(&actual)?;
        require(
            retained_content == review.manifest
                && (actual.dev(), actual.ino()) == (review.identity.0, review.identity.1),
            "retained original differs",
        )?;
        self.check(&target)?;
        self.check_parent(&review.path, &source_parent)?;
        require(
            dir_id(&super::open_dir(&fd_path(&source_parent).join(&retention_name))?)?
                == dir_id(&stage)?,
            "retention directory replaced",
        )?;
        self.complete(&id)?;
        Ok(retained)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};

    #[test]
    fn reviewed_file_observation_never_creates_destination_or_journal() {
        let root = std::env::temp_dir().join(format!("tt-transfer-observe-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(root.clone());
        let workspace = root.join("workspace");
        let journal = root.join("journal");
        fs::create_dir(&workspace).unwrap();
        fs::create_dir(&journal).unwrap();
        fs::set_permissions(&journal, fs::Permissions::from_mode(0o700)).unwrap();
        let source = workspace.join("source");
        let target = workspace.join("target");
        fs::write(&source, b"keep").unwrap();
        let engine = Engine::new(&workspace, &journal).unwrap();
        let review = engine.review(&source).unwrap();
        let destination = engine.destination(&target).unwrap();
        let observed = engine.inspect_file_transfer(&review, &destination).unwrap();
        assert_eq!(observed.bytes, 4);
        assert!(!observed.crosses_devices());
        assert!(!target.exists());
        assert_eq!(fs::read(&source).unwrap(), b"keep");
        assert_eq!(fs::read_dir(&journal).unwrap().count(), 0);
        fs::write(&target, b"other").unwrap();
        assert!(engine.inspect_file_transfer(&review, &destination).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"other");
        fs::remove_file(&target).unwrap();
        fs::hard_link(&source, workspace.join("hard")).unwrap();
        let reviewed = engine.review(&source).unwrap();
        assert!(engine
            .inspect_file_transfer(&reviewed, &destination)
            .is_err());
        symlink(&source, workspace.join("link")).unwrap();
        let link = engine.review(&workspace.join("link")).unwrap();
        assert!(engine.inspect_file_transfer(&link, &destination).is_err());
        assert_eq!(fs::read_dir(&journal).unwrap().count(), 0);
    }
}
