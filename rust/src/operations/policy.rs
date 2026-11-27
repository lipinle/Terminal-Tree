//! Fail-closed path policy for native personal-file operations.
use super::{dir_id, fail, require};
use rustix::fs::{self as rx, Mode, OFlags};
use std::fs::{self, File};
use std::io;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};

const SYSTEM: &[&str] = &[
    "/bin",
    "/boot",
    "/dev",
    "/etc",
    "/lib",
    "/lib32",
    "/lib64",
    "/libx32",
    "/proc",
    "/root",
    "/run",
    "/sbin",
    "/sys",
    "/usr",
    "/var",
    "/snap",
    "/opt",
    "/lost+found",
];

pub(super) fn normal_absolute(p: &Path) -> io::Result<PathBuf> {
    require(p.is_absolute(), "absolute path required")?;
    let mut out = PathBuf::from("/");
    for c in p.components() {
        match c {
            Component::RootDir => (),
            Component::Normal(n) => out.push(n),
            _ => return Err(fail("non-normal path refused")),
        }
    }
    require(
        out.as_os_str().as_bytes() == p.as_os_str().as_bytes(),
        "non-normal path refused",
    )?;
    Ok(out)
}

// Open every component relative to the preceding pinned descriptor. No
// symlinked ancestors, magic links or path-based canonicalize/open gap.
pub(super) fn open_absolute_dir(p: &Path) -> io::Result<File> {
    let p = normal_absolute(p)?;
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let mut dir: File = rx::open("/", flags, Mode::empty())?.into();
    for c in p.components() {
        if let Component::Normal(n) = c {
            dir = rx::openat(&dir, n, flags, Mode::empty())?.into();
        }
    }
    Ok(dir)
}

fn overlap(a: &Path, b: &Path) -> bool {
    a.starts_with(b) || b.starts_with(a)
}
fn system(p: &Path) -> bool {
    SYSTEM.iter().any(|x| overlap(p, Path::new(x)))
}
fn staged(p: &Path) -> bool {
    p.components().any(|c| {
        c.as_os_str()
            .as_bytes()
            .starts_with(b".terminal-tree-copy-")
    })
}

fn unescape(raw: &[u8]) -> io::Result<PathBuf> {
    let mut result = Vec::new();
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'\\' {
            require(
                i + 3 < raw.len() && raw[i + 1..i + 4].iter().all(|b| (b'0'..=b'7').contains(b)),
                "invalid mount escape",
            )?;
            let n = u16::from(raw[i + 1] - b'0') * 64
                + u16::from(raw[i + 2] - b'0') * 8
                + u16::from(raw[i + 3] - b'0');
            require(n > 0 && n <= 255, "invalid mount byte")?;
            result.push(n as u8);
            i += 4;
        } else {
            result.push(raw[i]);
            i += 1;
        }
    }
    require(
        !result.is_empty() && !result.contains(&0),
        "invalid mount field",
    )?;
    Ok(PathBuf::from(std::ffi::OsString::from_vec(result)))
}
#[derive(Debug)]
struct Mount {
    id: u64,
    root: PathBuf,
    target: PathBuf,
}
fn mounts() -> io::Result<Vec<Mount>> {
    parse_mounts(&fs::read("/proc/self/mountinfo")?)
}
fn parse_mounts(bytes: &[u8]) -> io::Result<Vec<Mount>> {
    let mut out = Vec::new();
    for line in bytes.split(|b| *b == b'\n').filter(|x| !x.is_empty()) {
        let fields: Vec<_> = line.split(|b| *b == b' ').collect();
        require(fields.len() >= 10, "invalid mountinfo")?;
        let id = std::str::from_utf8(fields[0])
            .map_err(io::Error::other)?
            .parse::<u64>()
            .map_err(io::Error::other)?;
        // Root can be an opaque kernel name for special mounts (e.g. nsfs).
        // Only the selected approved mount may have root exactly "/".
        // Every mount target must still be a normal absolute path.
        out.push(Mount {
            id,
            root: unescape(fields[3])?,
            target: normal_absolute(&unescape(fields[4])?)?,
        });
    }
    require(!out.is_empty(), "mount table unavailable")?;
    Ok(out)
}
fn mount_id(f: &File) -> io::Result<u64> {
    use std::os::fd::AsRawFd;
    let info = fs::read_to_string(format!("/proc/self/fdinfo/{}", f.as_raw_fd()))?;
    info.lines()
        .find_map(|l| {
            l.strip_prefix("mnt_id:")
                .and_then(|s| s.trim().parse().ok())
        })
        .ok_or_else(|| fail("descriptor mount ID unavailable"))
}
struct Root {
    path: PathBuf,
    dir: File,
    mount: u64,
}
impl Root {
    fn new(path: &Path) -> io::Result<Self> {
        let path = normal_absolute(path)?;
        let dir = open_absolute_dir(&path)?;
        require(
            dir.metadata()?.uid() == rustix::process::getuid().as_raw(),
            "owned root required",
        )?;
        let mount = mount_id(&dir)?;
        Ok(Self { path, dir, mount })
    }
    fn check(&self) -> io::Result<()> {
        let actual = open_absolute_dir(&self.path)?;
        require(
            dir_id(&actual)? == dir_id(&self.dir)? && mount_id(&actual)? == self.mount,
            "approved root replaced or remounted",
        )
    }
}
pub(super) struct PersonalPolicy {
    roots: Vec<Root>,
    protected: Vec<PathBuf>,
    data_home: PathBuf,
}
impl PersonalPolicy {
    pub(super) fn new(home: &Path, extra: &[PathBuf], protected: &[PathBuf]) -> io::Result<Self> {
        require(
            !rustix::process::geteuid().is_root()
                && rustix::process::geteuid() == rustix::process::getuid(),
            "normal unprivileged user required",
        )?;
        let home = normal_absolute(home)?;
        require(
            home != Path::new("/") && !system(&home),
            "system home refused",
        )?;
        let h = Root::new(&home)?;
        let table = mounts()?;
        let hm = table
            .iter()
            .find(|m| m.id == h.mount)
            .ok_or_else(|| fail("home mount missing"))?;
        require(
            hm.root.as_os_str().as_bytes() == b"/",
            "home on subdirectory/bind mount not supported yet",
        )?;
        let mut roots = vec![h];
        for path in extra {
            let path = normal_absolute(path)?;
            require(
                ["/mnt", "/media", "/run/media"]
                    .iter()
                    .any(|p| path.starts_with(p) && path != Path::new(p)),
                "external root must be under a media mount location",
            )?;
            require(
                !roots.iter().any(|r| overlap(&r.path, &path)),
                "overlapping approved roots refused",
            )?;
            let r = Root::new(&path)?;
            require(
                r.dir.metadata()?.dev() != fs::metadata("/")?.dev(),
                "system root filesystem alias refused",
            )?;
            let m = table
                .iter()
                .find(|m| m.id == r.mount)
                .ok_or_else(|| fail("external mount missing"))?;
            require(
                m.target == path
                    && m.root.as_os_str().as_bytes() == b"/"
                    && !roots.iter().any(|r0| {
                        r0.dir.metadata().map_or(true, |a| {
                            r.dir.metadata().map_or(true, |b| a.dev() == b.dev())
                        })
                    }),
                "external root must be a distinct whole-filesystem mount",
            )?;
            roots.push(r);
        }
        let data_home = normal_absolute(
            &std::env::var_os("XDG_DATA_HOME")
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".local/share")),
        )?;
        let mut protected: Vec<PathBuf> = protected
            .iter()
            .map(|p| normal_absolute(p))
            .collect::<io::Result<_>>()?;
        for suffix in [
            ".bashrc",
            ".profile",
            ".bash_profile",
            ".ssh",
            ".gnupg",
            ".local/share/Trash",
            ".local/share/terminal-tree",
            ".local/state/terminal-tree",
            ".config/terminal-tree",
        ] {
            protected.push(home.join(suffix));
        }
        // Also protect configured XDG locations and their canonical aliases,
        // even if not under the ordinary home hierarchy.
        for (var, suffix) in [
            ("XDG_DATA_HOME", "Trash"),
            ("XDG_DATA_HOME", "terminal-tree"),
            ("XDG_STATE_HOME", "terminal-tree"),
            ("XDG_CONFIG_HOME", "terminal-tree"),
        ] {
            if let Some(base) = std::env::var_os(var).filter(|v| !v.is_empty()) {
                let p = normal_absolute(&PathBuf::from(base))?.join(suffix);
                protected.push(p);
            }
        }
        // Resolve the existing prefix, even if the final managed directory
        // does not exist yet. Otherwise a symlinked XDG base can bypass a guard.
        let aliases: Vec<_> = protected
            .iter()
            .map(|p| {
                let mut prefix = p.as_path();
                let mut tail = Vec::new();
                loop {
                    match prefix.canonicalize() {
                        Ok(mut real) => {
                            for n in tail.iter().rev() {
                                real.push(n);
                            }
                            return Ok(real);
                        }
                        Err(e) if e.kind() == io::ErrorKind::NotFound => {
                            tail.push(
                                prefix
                                    .file_name()
                                    .ok_or_else(|| fail("invalid protected path"))?
                                    .to_os_string(),
                            );
                            prefix = prefix
                                .parent()
                                .ok_or_else(|| fail("invalid protected parent"))?;
                        }
                        Err(e) => return Err(e),
                    }
                }
            })
            .collect::<io::Result<_>>()?;
        protected.extend(aliases);
        Ok(Self {
            roots,
            protected,
            data_home,
        })
    }
    pub(super) fn data_home(&self) -> &Path {
        &self.data_home
    }
    // Only the Trash adapter can use this narrow bypass. Ordinary review,
    // destination, move, copy and create continue to reject the entire Trash.
    pub(super) fn trash_dir(&self, dir: &Path) -> io::Result<File> {
        let root = &self.roots[0];
        root.check()?;
        let trash = self.data_home.join("Trash");
        require(
            self.data_home.starts_with(&root.path) && self.data_home != root.path,
            "home Trash must be inside approved home",
        )?;
        require(
            dir == trash || dir == trash.join("files") || dir == trash.join("info"),
            "not an approved Trash directory",
        )?;
        require(
            !self
                .protected
                .iter()
                .any(|p| p != &trash && overlap(p, &trash)),
            "Trash overlaps other protected state",
        )?;
        let table = mounts()?;
        require(
            !table.iter().any(|m| {
                m.target != root.path
                    && m.target.starts_with(&root.path)
                    && (trash.starts_with(&m.target) || m.target.starts_with(&trash))
            }),
            "nested or separate Trash mount refused",
        )?;
        let f = open_absolute_dir(dir)?;
        let m = f.metadata()?;
        require(
            mount_id(&f)? == root.mount
                && m.uid() == rustix::process::getuid().as_raw()
                && m.mode() & 0o077 == 0,
            "private owned same-filesystem Trash required",
        )?;
        Ok(f)
    }
    pub(super) fn path(&self, p: &Path) -> io::Result<PathBuf> {
        let p = normal_absolute(p)?;
        let root = self
            .roots
            .iter()
            .find(|r| p.starts_with(&r.path) && p != r.path)
            .ok_or_else(|| fail("outside approved personal roots"))?;
        root.check()?;
        require(
            !staged(&p) && !self.protected.iter().any(|x| overlap(x, &p)),
            "protected runtime, personal configuration or recovery location",
        )?;
        if !root.path.starts_with("/run/media") {
            require(!system(&p), "system location refused")?;
        }
        // The final entry itself must not be a mount point (including a
        // same-device bind mount). Also reject an ancestor mount under root.
        let table = mounts()?;
        require(
            !table.iter().any(|m| {
                m.target.starts_with(&root.path)
                    && m.target != root.path
                    && (p.starts_with(&m.target) || m.target.starts_with(&p))
            }),
            "unapproved nested mount refused",
        )?;
        let parent = open_absolute_dir(p.parent().unwrap())?;
        require(
            mount_id(&parent)? == root.mount,
            "unapproved nested mount refused",
        )?;
        Ok(p)
    }
    pub(super) fn parent(&self, p: &Path) -> io::Result<File> {
        let p = self.path(p)?;
        let root = self.roots.iter().find(|r| p.starts_with(&r.path)).unwrap();
        let dir = open_absolute_dir(p.parent().unwrap())?;
        require(mount_id(&dir)? == root.mount, "parent mount changed")?;
        Ok(dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn opaque_mount_roots_do_not_invalidate_other_mounts_or_grant_access() {
        let table=parse_mounts(b"1 0 8:1 / / rw - ext4 /dev/sda rw\n2 1 0:4 net:[1234] /run/netns/test rw - nsfs nsfs rw\n3 1 8:2 /subvol /mnt/btrfs rw - btrfs /dev/sdb rw\n").unwrap();
        assert_eq!(table[0].root.as_os_str().as_bytes(), b"/");
        assert_ne!(table[1].root.as_os_str().as_bytes(), b"/");
        assert_ne!(table[2].root.as_os_str().as_bytes(), b"/");
        assert!(parse_mounts(b"1 0 8:1 / relative rw - ext4 /dev/sda rw\n").is_err());
        assert!(parse_mounts(b"1 0 8:1 / /mnt/\\000 rw - ext4 /dev/sda rw\n").is_err());
    }
}
