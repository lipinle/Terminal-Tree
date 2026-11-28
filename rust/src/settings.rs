//! Explicitly saved Rust-only view preferences in the approved private journal.
use rustix::fs::{self as rx, Mode, OFlags};
use std::{
    fs::File,
    io::{self, Read, Write},
    os::unix::fs::MetadataExt,
    path::{Component, Path, PathBuf},
};
use terminal_tree::{keymap::Keymap, theme::Theme, KindFilter, Model, Order};
const NAME: &str = "rust-view-settings.json";
const PANE_NAME: &str = "rust-pane-settings.json";
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
        self.read_named(NAME)
    }
    fn read_named(&self, name: &str) -> io::Result<Option<serde_json::Value>> {
        self.check()?;
        let f: File = match rx::openat(
            &self.dir,
            name,
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
        m.set_view(hidden, order, kind_filter, m.query.clone())
    }
    pub fn save(&self, m: &Model) -> io::Result<()> {
        let v = serde_json::json!({"version":1,"hidden":m.hidden,"order":format!("{:?}",m.order),"kind":format!("{:?}",m.kind_filter)});
        self.save_named(NAME, &v)
    }

    fn decode_panes(v: &serde_json::Value) -> io::Result<(Theme, Keymap)> {
        if v["version"].as_u64() != Some(1) {
            return Err(error("unsupported pane settings version"));
        }
        let theme = v["theme"]
            .as_str()
            .and_then(Theme::parse)
            .ok_or_else(|| error("invalid pane theme"))?;
        let keys = v["keys"]
            .as_str()
            .ok_or_else(|| error("invalid pane keys"))?;
        let keymap =
            Keymap::from_spec(keys).map_err(|_| error("invalid pane navigation keymap"))?;
        Ok((theme, keymap))
    }

    pub fn load_panes(&self) -> io::Result<Option<(Theme, Keymap)>> {
        self.read_named(PANE_NAME)?
            .as_ref()
            .map(Self::decode_panes)
            .transpose()
    }

    pub fn save_panes(&self, theme: Theme, keymap: &Keymap) -> io::Result<()> {
        self.load_panes()?; // refuse malformed settings as well as unsafe files
        let spec = keymap.to_spec().map_err(|_| error("invalid pane keymap"))?;
        let v = serde_json::json!({"version":1,"theme":theme.name(),"keys":spec});
        self.save_named(PANE_NAME, &v)
    }

    fn save_named(&self, target: &str, v: &serde_json::Value) -> io::Result<()> {
        self.read_named(target)?; // refuse preexisting unsafe file before replacement
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
            serde_json::to_writer(&mut file, v).map_err(io::Error::other)?;
            file.write_all(b"\n")?;
            file.sync_all()?;
            self.check()?;
            rx::renameat(&self.dir, &name, &self.dir, target)?;
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
    fn pane_settings_round_trip_and_refuse_unsafe_files() {
        use std::os::unix::fs::{symlink, DirBuilderExt};
        let dir = std::env::temp_dir().join(format!("tt-pane-settings-{}", std::process::id()));
        std::fs::DirBuilder::new().mode(0o700).create(&dir).unwrap();
        struct Cleanup(PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(dir.clone());
        let store = Store::new(&dir).unwrap();
        assert!(store.load_panes().unwrap().is_none());
        let keys = Keymap::from_spec("up=down,down=up,refresh=f12").unwrap();
        store.save_panes(Theme::Dark, &keys).unwrap();
        let (theme, decoded) = store.load_panes().unwrap().unwrap();
        assert_eq!(theme, Theme::Dark);
        assert_eq!(decoded.to_spec().unwrap(), keys.to_spec().unwrap());
        assert!(!dir.join(NAME).exists());
        store.save_panes(Theme::Light, &keys).unwrap();
        assert_eq!(store.load_panes().unwrap().unwrap().0, Theme::Light);
        std::fs::remove_file(dir.join(PANE_NAME)).unwrap();
        symlink(dir.join("unoccupied"), dir.join(PANE_NAME)).unwrap();
        assert!(store.load_panes().is_err());
        assert!(store.save_panes(Theme::Terminal, &keys).is_err());
        assert!(!dir.join("unoccupied").exists());
        assert!(Store::decode_panes(&serde_json::json!({
            "version":1,"theme":"dark","keys":"up=down"
        }))
        .is_err());
    }

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
