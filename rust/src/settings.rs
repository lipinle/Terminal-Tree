//! Explicitly saved Rust-only view preferences in the approved private journal.
use rustix::fs::{self as rx, Mode, OFlags};
use std::{
    fs::File,
    io::{self, Read, Write},
    os::unix::fs::MetadataExt,
    path::{Component, Path, PathBuf},
};
use terminal_tree::{KindFilter, Model, Order};
const NAME: &str = "rust-view-settings.json";
fn error(s: &str) -> io::Error {
    io::Error::other(s)
}
fn open_dir(path: &Path) -> io::Result<File> {
    if !path.is_absolute() {
        return Err(error("absolute journal required"));
    }
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let mut dir: File = rx::open("/", flags, Mode::empty())?.into();
    for c in path.components() {
        match c {
            Component::RootDir => (),
            Component::Normal(n) => dir = rx::openat(&dir, n, flags, Mode::empty())?.into(),
            _ => return Err(error("non-normal settings directory")),
        }
    }
    let m = dir.metadata()?;
    if m.uid() != rustix::process::getuid().as_raw() || m.mode() & 0o077 != 0 {
        return Err(error("private owned journal required"));
    }
    Ok(dir)
}
pub struct Store {
    dir: File,
    path: PathBuf,
}
impl Store {
    pub fn new(path: &Path) -> io::Result<Self> {
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()?.join(path)
        };
        let dir = open_dir(&path)?;
        Ok(Self { dir, path })
    }
    fn check(&self) -> io::Result<()> {
        let a = open_dir(&self.path)?.metadata()?;
        let b = self.dir.metadata()?;
        if (a.dev(), a.ino()) != (b.dev(), b.ino()) {
            return Err(error("settings directory replaced"));
        }
        Ok(())
    }
    fn read(&self) -> io::Result<Option<serde_json::Value>> {
        self.check()?;
        let f: File = match rx::openat(
            &self.dir,
            NAME,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        ) {
            Ok(f) => f.into(),
            Err(e) if e == rustix::io::Errno::NOENT => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let m = f.metadata()?;
        if !m.is_file()
            || m.nlink() != 1
            || m.uid() != rustix::process::getuid().as_raw()
            || m.mode() & 0o077 != 0
            || m.len() > 4096
        {
            return Err(error("invalid Rust view settings file"));
        }
        let mut bytes = Vec::new();
        f.take(4097).read_to_end(&mut bytes)?;
        if bytes.len() > 4096 {
            return Err(error("settings too large"));
        }
        Ok(Some(
            serde_json::from_slice(&bytes).map_err(io::Error::other)?,
        ))
    }
    pub fn load(&self, m: &mut Model) -> io::Result<()> {
        let Some(v) = self.read()? else { return Ok(()) };
        Self::apply(m, &v)
    }

    fn apply(m: &mut Model, v: &serde_json::Value) -> io::Result<()> {
        if v["version"].as_u64() != Some(1) {
            return Err(error("unsupported Rust view settings version"));
        }
        let hidden = v["hidden"]
            .as_bool()
            .ok_or_else(|| error("invalid hidden setting"))?;
        let order = match v["order"].as_str() {
            Some("Name") => Order::Name,
            Some("Size") => Order::Size,
            Some("Date") => Order::Date,
            Some("Type") => Order::Type,
            _ => return Err(error("invalid order setting")),
        };
        let kind_filter = match v["kind"].as_str() {
            Some("All") => KindFilter::All,
            Some("Directories") => KindFilter::Directories,
            Some("Files") => KindFilter::Files,
            Some("Links") => KindFilter::Links,
            _ => return Err(error("invalid kind setting")),
        };
        let previous = (m.hidden, m.order, m.kind_filter);
        (m.hidden, m.order, m.kind_filter) = (hidden, order, kind_filter);
        // Model::reload scans before replacing its entries and selection.
        if let Err(error) = m.reload() {
            (m.hidden, m.order, m.kind_filter) = previous;
            return Err(error);
        }
        Ok(())
    }
    pub fn save(&self, m: &Model) -> io::Result<()> {
        self.read()?; // refuse preexisting unsafe file before replacement
        let v = serde_json::json!({"version":1,"hidden":m.hidden,"order":format!("{:?}",m.order),"kind":format!("{:?}",m.kind_filter)});
        let mut random = [0u8; 16];
        File::open("/dev/urandom")?.read_exact(&mut random)?;
        let suffix = random
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        let name = format!(".rust-view-{suffix}.tmp");
        let mut file: File = rx::openat(
            &self.dir,
            &name,
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from_bits_truncate(0o600),
        )?
        .into();
        let result = (|| -> io::Result<()> {
            serde_json::to_writer(&mut file, &v).map_err(io::Error::other)?;
            file.write_all(b"\n")?;
            file.sync_all()?;
            self.check()?;
            rx::renameat(&self.dir, &name, &self.dir, NAME)?;
            self.dir.sync_all()
        })();
        // Never unlink a pathname on an error after an uncertain rename.
        // An orphaned private temp is harmless and can be inspected manually.
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_settings_and_failed_scan_preserve_view_state() {
        let mut model = Model::new(std::env::current_dir().unwrap()).unwrap();
        let original = (model.hidden, model.order, model.kind_filter);
        let original_rows = model.entries.len();
        let mut value = serde_json::json!({
            "version": 1, "hidden": true, "order": "Size", "kind": "invalid"
        });
        assert!(Store::apply(&mut model, &value).is_err());
        assert_eq!((model.hidden, model.order, model.kind_filter), original);
        assert_eq!(model.entries.len(), original_rows);
        value["kind"] = serde_json::json!("Files");
        // A regular file cannot be scanned as a directory.
        model.directory = std::env::current_exe().unwrap();
        assert!(Store::apply(&mut model, &value).is_err());
        assert_eq!((model.hidden, model.order, model.kind_filter), original);
        assert_eq!(model.entries.len(), original_rows);
    }
}
