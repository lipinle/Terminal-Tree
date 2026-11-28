//! Narrow read-only preflight for a future retained-source transfer protocol.
//! It does not enable cross-device move, create a journal, or reserve names.
use super::{
    attributes, dir_id, fd_path, identity, no_conflict, require, Destination, Engine, Review,
};
use std::fs;
use std::io;
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
