//! Experimental home Trash adapter for an explicit workspace.
//! GIO controls trash placement; it is not a guarantee of power-loss durability.
use super::{dir_id, fail, fd_path, identity, no_conflict, open_dir, require, same_parent,
    unique, reject_mounts, Destination, Engine, Identity, Review, Snapshot, Transfer};
use rustix::fs::{self as rx, AtFlags, Mode, OFlags};
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

pub struct Trash<'a> {
    engine: &'a Engine,
    data_home: PathBuf,
    root: PathBuf,
    pinned: Option<(File,File,File)>,
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
impl RestoreReview {
    pub fn source(&self) -> &Path { self.item.path() }
    pub fn destination(&self) -> &Path { self.destination.path() }
}
#[derive(Debug)]
pub struct PurgeReview {
    item: Review,
    info: Option<(PathBuf, Identity, Vec<u8>)>,
    info_parent: (u64,u64),
}
impl PurgeReview {
    pub fn source(&self) -> &Path { self.item.path() }
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
        && m.nlink()==1 && m.len() <= 65536, "invalid Trash info file")?;
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
fn erase(parent: &File, name: &std::ffi::OsStr, snapshot: &Snapshot, rel: &Path, physical: &Path, checkpoint:&mut dyn FnMut(&str)->io::Result<()>) -> io::Result<()> {
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
            erase(&dir, &n, snapshot, &r, &physical.join(&n),checkpoint)?;
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
    checkpoint("purge-entry-before-sync")?;
    parent.sync_all()?;
    checkpoint("purge-entry-after-sync")
}

impl<'a> Trash<'a> {
    pub fn new(engine: &'a Engine, data_home: &Path) -> io::Result<Self> {
        if let Some(policy)=&engine.personal{
            require(data_home==policy.data_home(),"configured data home required")?;
            let root=data_home.join("Trash");
            let pinned=(policy.trash_dir(&root)?,policy.trash_dir(&root.join("files"))?,policy.trash_dir(&root.join("info"))?);
            return Ok(Self{engine,data_home:data_home.to_path_buf(),root,pinned:Some(pinned)});
        }
        require(fs::symlink_metadata(data_home)?.is_dir(), "data home symlink refused")?;
        let data_home = data_home.canonicalize()?;
        require(data_home.starts_with(&engine.root), "Trash outside experimental workspace")?;
        let root = data_home.join("Trash");
        if let Ok(m) = fs::symlink_metadata(&root) { require(m.is_dir(), "Trash symlink refused")?; }
        Ok(Self { engine, data_home, root, pinned:None })
    }
    fn private_dir(&self, name: &str) -> io::Result<File> {
        if let Some((root,files,info))=&self.pinned{
            let policy=self.engine.personal.as_ref().unwrap();
            require(dir_id(&policy.trash_dir(&self.root)?)?==dir_id(root)?,"Trash root replaced")?;
            let expected=match name{"files"=>files,"info"=>info,_=>return Err(fail("invalid Trash directory"))};
            let dir=policy.trash_dir(&self.root.join(name))?;
            require(dir_id(&dir)?==dir_id(expected)?,"Trash directory replaced")?;
            Ok(dir)
        }else{
            self.workspace_dir(name)
        }
    }
    fn workspace_dir(&self,name:&str)->io::Result<File>{
        require(self.root.canonicalize()? == self.root, "Trash root redirected")?;
        let path = self.root.join(name);
        let f = open_dir(&path)?;
        let m = f.metadata()?;
        require(m.uid() == rustix::process::getuid().as_raw() && m.mode() & 0o077 == 0,
            "private owned Trash directory required")?;
        Ok(f)
    }
    fn review_item(&self,path:&Path)->io::Result<Review>{
        if self.pinned.is_none(){return self.engine.review(path);}
        let p=super::policy::normal_absolute(path)?;
        require(p.parent()==Some(self.root.join("files").as_path()),"only top-level home Trash items allowed")?;
        reject_mounts(&p)?;
        let parent=self.private_dir("files")?;
        let entry=fd_path(&parent).join(p.file_name().ok_or_else(||fail("missing Trash filename"))?);
        let m=fs::symlink_metadata(&entry)?;
        let (manifest,snapshot)=super::manifest(&entry,false)?;
        let item=Review{path:p,identity:identity(&m),parent:dir_id(&parent)?,snapshot,manifest};
        self.check_item(&item)?;
        Ok(item)
    }
    fn check_item(&self,item:&Review)->io::Result<File>{
        if self.pinned.is_none(){return self.engine.check(item);}
        require(item.path.parent()==Some(self.root.join("files").as_path()),"not a home Trash item")?;
        reject_mounts(&item.path)?;
        let parent=self.private_dir("files")?;
        require(dir_id(&parent)?==item.parent,"Trash parent changed")?;
        let entry=fd_path(&parent).join(item.path.file_name().unwrap());
        require(identity(&fs::symlink_metadata(&entry)?)==item.identity,"Trash item changed")?;
        let (manifest,snapshot)=super::manifest(&entry,false)?;
        require(manifest==item.manifest && snapshot==item.snapshot,"Trash content changed")?;
        Ok(parent)
    }
    fn send_personal(&self,item:&Review,checkpoint:&mut dyn FnMut(&str)->io::Result<()>)->io::Result<()>{
        // Native same-filesystem move, not GIO: persist the recovery record
        // and freedesktop metadata before publishing the data in Trash.
        let parent=self.engine.check(item)?;
        let files=self.private_dir("files")?;
        let info=self.private_dir("info")?;
        let source=fd_path(&parent).join(item.path.file_name().unwrap());
        require(fs::symlink_metadata(&source)?.dev()==files.metadata()?.dev(),"cross-device Trash refused; use Python for external Trash")?;
        let id=unique()?;
        let mut name=item.path.file_name().unwrap().as_bytes().to_vec();
        name.truncate(180);
        name.extend_from_slice(format!("-tt-{id}").as_bytes());
        let name=OsString::from_vec(name);
        let mut info_name=name.clone();info_name.push(".trashinfo");
        let target=self.root.join("files").join(&name);
        no_conflict(&fd_path(&files).join(&name))?;
        no_conflict(&fd_path(&info).join(&info_name))?;
        let encoded=item.path.as_os_str().as_bytes().iter().map(|&b|{
            if b.is_ascii_alphanumeric() || b"-._~/".contains(&b){(b as char).to_string()}
            else{format!("%{b:02X}")}
        }).collect::<String>();
        let date=Command::new("/usr/bin/date").arg("+%Y-%m-%dT%H:%M:%S").output()?;
        require(date.status.success(),"Trash deletion date unavailable")?;
        let date=std::str::from_utf8(&date.stdout).map_err(io::Error::other)?.trim();
        require(date.len()==19 && date.bytes().all(|b|b.is_ascii_digit()||b"-T:".contains(&b)),"invalid deletion date")?;
        let metadata=format!("[Trash Info]\nPath={encoded}\nDeletionDate={date}\n");
        let (content,ids)=super::manifest(&source,true)?;
        require(content==item.manifest && ids==item.snapshot,"source changed while syncing")?;
        checkpoint("trash-prepare")?;
        self.engine.log(&id,"native-trash",Some(&item.path),Some(&target),Some(&self.root.join("info").join(&info_name)))?;
        checkpoint("trash-journal")?;
        let mut record:File=rx::openat(&info,&info_name,OFlags::WRONLY|OFlags::CREATE|OFlags::EXCL|OFlags::NOFOLLOW|OFlags::CLOEXEC,Mode::from_bits_truncate(0o600))?.into();
        record.write_all(metadata.as_bytes())?;record.sync_all()?;info.sync_all()?;
        let info_id=identity(&record.metadata()?);
        checkpoint("trash-info-synced")?;
        self.engine.check_identity(item)?;
        self.engine.check_parent(&item.path,&parent)?;
        require(dir_id(&self.private_dir("files")?)?==dir_id(&files)? && dir_id(&self.private_dir("info")?)?==dir_id(&info)?,"Trash replaced")?;
        require(info_file(&info,&info_name)?==(info_id,metadata.as_bytes().to_vec()),"Trash metadata replaced")?;
        parent.sync_all()?;files.sync_all()?;
        self.engine.check_journal()?;
        checkpoint("trash-commit")?;
        rx::renameat_with(&parent,item.path.file_name().unwrap(),&files,&name,rx::RenameFlags::NOREPLACE)?;
        let finish=(||->io::Result<()>{
            checkpoint("trash-before-sync")?;
            files.sync_all()?;parent.sync_all()?;
            checkpoint("trash-after-sync")?;
            let after=self.review_item(&target)?;
            require(after.manifest==item.manifest && after.identity.0==item.identity.0 && after.identity.1==item.identity.1,"trashed result changed")?;
            require(info_file(&self.private_dir("info")?,&info_name)?==(info_id,metadata.as_bytes().to_vec()),"Trash metadata changed after move")?;
            self.engine.complete(&id)?;
            checkpoint("trash-done")
        })();
        finish.map_err(|e|io::Error::other(format!("item moved to Trash; inspect pending journal before retrying: {e}")))
    }
    pub fn validate_send(&self,item:&Review)->io::Result<()>{
        self.engine.check_journal()?;
        self.engine.check(item)?;
        if self.pinned.is_some(){
            let files=self.private_dir("files")?;
            self.private_dir("info")?;
            require(item.identity.0==files.metadata()?.dev(),"cross-device Trash refused")?;
        }else{
            require(!item.path.starts_with(&self.data_home)&&!self.data_home.starts_with(&item.path),"managed Trash location protected")?;
        }
        Ok(())
    }
    pub fn validate_restore(&self,review:&RestoreReview)->io::Result<()>{
        self.engine.check_journal()?;
        self.check_item(&review.item)?;
        self.check_info(review)?;
        let dst=self.engine.parent_for(review.destination.path())?;
        require(dir_id(&dst)?==review.destination.parent,"restore destination replaced")?;
        no_conflict(&fd_path(&dst).join(review.destination.path().file_name().unwrap()))?;
        require(review.item.identity.0==dst.metadata()?.dev(),"cross-device restore refused")?;
        Ok(())
    }
    pub fn validate_purge(&self,review:&PurgeReview)->io::Result<()>{
        self.engine.check_journal()?;
        let parent=self.check_item(&review.item)?;
        self.check_purge_info(review)?;
        preflight_purge(&parent,review.item.path.file_name().unwrap(),&review.item.snapshot,Path::new("."))
    }
    pub fn send(&self, item: &Review) -> io::Result<()> {
        self.send_with(item,&mut |_|Ok(()))
    }
    #[doc(hidden)]
    pub fn send_with(&self,item:&Review,checkpoint:&mut dyn FnMut(&str)->io::Result<()>)->io::Result<()>{
        if self.pinned.is_some(){return self.send_personal(item,checkpoint);}

        require(!item.path.starts_with(&self.data_home) && !self.data_home.starts_with(&item.path),
            "managed Trash location protected")?;
        let parent = self.engine.check(item)?;
        let id = unique()?;
        self.engine.log(&id, "gio-trash", Some(&item.path), None, None)?;
        self.engine.check(item)?;
        checkpoint("trash-commit")?;
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
        let item = self.review_item(path)?;
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
        self.restore_with(review,&mut |_|Ok(()))
    }
    #[doc(hidden)]
    pub fn restore_with(&self,review:&RestoreReview,checkpoint:&mut dyn FnMut(&str)->io::Result<()>)->io::Result<RestoreOutcome>{
        self.check_info(review)?;
        if self.pinned.is_some(){
            self.check_item(&review.item)?;
            let dst=self.engine.parent_for(review.destination.path())?;
            require(dir_id(&dst)?==review.destination.parent,"restore destination replaced")?;
            no_conflict(&fd_path(&dst).join(review.destination.path().file_name().unwrap()))?;
        }
        let id = unique()?;
        self.engine.log(&id, "restore", Some(&review.item.path), Some(&review.destination.path), Some(&review.info))?;
        checkpoint("restore-journal")?;
        self.check_info(review)?;
        if self.pinned.is_some(){
            let src=self.check_item(&review.item)?;
            let dst=self.engine.parent_for(review.destination.path())?;
            require(dir_id(&dst)?==review.destination.parent,"restore destination replaced")?;
            let name=review.item.path.file_name().unwrap();
            let target=review.destination.path().file_name().unwrap();
            require(fs::symlink_metadata(fd_path(&src).join(name))?.dev()==dst.metadata()?.dev(),"cross-device restore refused")?;
            no_conflict(&fd_path(&dst).join(target))?;
            let (content,ids)=super::manifest(&fd_path(&src).join(name),true)?;
            require(content==review.item.manifest && ids==review.item.snapshot,"Trash content changed while syncing")?;
            self.check_item(&review.item)?;
            self.check_info(review)?;
            self.engine.check_parent(review.destination.path(),&dst)?;
            src.sync_all()?;dst.sync_all()?;
            self.engine.check_journal()?;
            checkpoint("restore-commit")?;
            rx::renameat_with(&src,name,&dst,target,rx::RenameFlags::NOREPLACE)?;
            let finish=(||->io::Result<()>{
                checkpoint("restore-before-sync")?;
                src.sync_all()?;dst.sync_all()?;
                checkpoint("restore-after-sync")?;
                let after=self.engine.review(review.destination.path())?;
                require(after.manifest==review.item.manifest && after.identity.0==review.item.identity.0 && after.identity.1==review.item.identity.1,"restored result changed")?;
                Ok(())
            })();
            if finish.is_err(){return Ok(RestoreOutcome::MetadataPending);}
        }else{self.engine.transfer_reviewed_with(&review.item, &review.destination, Transfer::Move,checkpoint)?;}
        let cleanup = (|| -> io::Result<()> {
            let parent = self.check_info(review)?;
            rx::unlinkat(&parent, review.info.file_name().unwrap(), AtFlags::empty())?;
            checkpoint("restore-info-before-sync")?;
            parent.sync_all()?;
            checkpoint("restore-info-after-sync")?;
            self.engine.complete(&id)?;
            checkpoint("restore-done")
        })();
        // Data has already moved. Never present a cleanup error as permission to retry.
        Ok(if cleanup.is_ok() { RestoreOutcome::Restored } else { RestoreOutcome::MetadataPending })
    }
    pub fn review_purge(&self, path: &Path) -> io::Result<PurgeReview> {
        let files = self.private_dir("files")?;
        let info_parent = self.private_dir("info")?;
        let item = self.review_item(path)?;
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
        self.purge_with(review,confirmation,&mut |_|Ok(()))
    }
    #[doc(hidden)]
    pub fn purge_with(&self,review:&PurgeReview,confirmation:&str,checkpoint:&mut dyn FnMut(&str)->io::Result<()>)->io::Result<PurgeOutcome>{
        require(confirmation == "DELETE", "explicit DELETE confirmation required")?;
        let parent = self.check_item(&review.item)?;
        let current = self.private_dir("files")?;
        require(dir_id(&parent)? == dir_id(&current)?, "Trash files directory changed")?;
        self.check_purge_info(review)?;
        let name = review.item.path.file_name().unwrap();
        preflight_purge(&parent, name, &review.item.snapshot, Path::new("."))?;
        let id = unique()?;
        self.engine.log(&id, "permanent-delete", Some(&review.item.path), None, review.info.as_ref().map(|(p,_,_)|p.as_path()))?;
        checkpoint("purge-journal")?;
        self.check_item(&review.item)?;
        self.check_purge_info(review)?;
        self.engine.check_journal()?;
        require(dir_id(&self.private_dir("files")?)?==dir_id(&parent)?,"Trash files replaced")?;
        checkpoint("purge-commit")?;
        erase(&parent, name, &review.item.snapshot, Path::new("."), &review.item.path,checkpoint)?;
        checkpoint("purge-data-synced")?;
        let cleanup = (|| -> io::Result<()> {
            let info_parent = self.check_purge_info(review)?;
            if let Some((p, _, _)) = &review.info {
                rx::unlinkat(&info_parent, p.file_name().unwrap(), AtFlags::empty())?;
                checkpoint("purge-info-before-sync")?;
                info_parent.sync_all()?;
                checkpoint("purge-info-after-sync")?;
            }
            self.engine.complete(&id)?;
            checkpoint("purge-done")
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
