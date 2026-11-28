//! Command-line arguments and dispatch for the navigator and file tools.
use super::{browse, edition, list, panes_ui, tree_ui};
#[cfg(feature = "native-ops")]
use super::{result_channel, workspace};
use std::io::IsTerminal;
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
use std::{
    env,
    ffi::OsString,
    io::{self, Write},
    path::PathBuf,
};
use terminal_tree::{display_name, scan, Order};

fn path_arg(value: Option<OsString>) -> io::Result<PathBuf> {
    match value {
        Some(p) if p.is_empty() => Err(io::Error::new(io::ErrorKind::InvalidInput, "empty path")),
        Some(p) => {
            let p = PathBuf::from(p);
            if p.is_absolute() {
                Ok(p)
            } else {
                Ok(env::current_dir()?.join(p))
            }
        }
        None => env::current_dir(),
    }
}

fn find(mut args: impl Iterator<Item = OsString>) -> io::Result<()> {
    use std::sync::atomic::AtomicBool;
    use terminal_tree::search::{filenames, SearchLimits};

    let invalid = |message| io::Error::new(io::ErrorKind::InvalidInput, message);
    let root = path_arg(Some(
        args.next().ok_or_else(|| invalid("search path required"))?,
    ))?;
    let query = args
        .next()
        .ok_or_else(|| invalid("search query required"))?
        .into_string()
        .map_err(|_| invalid("search query must be UTF-8"))?;
    if query.is_empty() {
        return Err(invalid("search query must not be empty"));
    }
    let mut hidden = false;
    let mut content = false;
    let mut depth = None;
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--hidden") if !hidden => hidden = true,
            Some("--content") if !content => content = true,
            Some("--depth") if depth.is_none() => {
                depth = Some(
                    args.next()
                        .and_then(|value| value.to_str()?.parse::<usize>().ok())
                        .filter(|value| (1..=16).contains(value))
                        .ok_or_else(|| invalid("search depth must be 1..=16"))?,
                );
            }
            _ => return Err(invalid("unknown or duplicate search option")),
        }
    }
    let limits = SearchLimits {
        entries: 16_384,
        depth: depth.unwrap_or(8),
        matches: 4096,
    };
    let cancel = AtomicBool::new(false);
    if content {
        #[cfg(all(feature = "native-ops", target_os = "linux"))]
        {
            use terminal_tree::search::content::{text, ContentLimits};

            let report = text(
                &root,
                &query,
                hidden,
                ContentLimits {
                    traversal: limits,
                    file_bytes: 1_048_576,
                    total_bytes: 64 * 1_048_576,
                },
                &cancel,
            )?;
            let mut out = io::BufWriter::new(io::stdout().lock());
            for path in &report.matches {
                writeln!(out, "{}", display_name(path.as_os_str()))?;
            }
            out.flush()?;
            eprintln!(
                "Content search: {} matches; {} files read; {} bytes read; skipped: {} large, {} binary/non-UTF-8, {} nonregular",
                report.matches.len(),
                report.files_read,
                report.bytes_read,
                report.skipped_large,
                report.skipped_binary,
                report.skipped_nonregular
            );
            return Ok(());
        }
        #[cfg(not(all(feature = "native-ops", target_os = "linux")))]
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "content search requires the native Linux build",
        ));
    }
    // Produce no result lines if traversal fails or exhausts a budget.
    let entries = filenames(&root, &query, hidden, limits, &cancel)?;
    let mut out = io::BufWriter::new(io::stdout().lock());
    for entry in entries {
        writeln!(out, "{}", display_name(entry.path.as_os_str()))?;
    }
    out.flush()
}

fn end_of_args(args: &mut impl Iterator<Item = OsString>) -> io::Result<()> {
    if args.next().is_some() {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "unexpected argument",
        ))
    } else {
        Ok(())
    }
}

fn print_help() -> io::Result<()> {
    let mut out = io::stdout().lock();
    writeln!(
        out,
        "Terminal Tree — terminal file navigator

Usage: terminal-tree COMMAND [ARGUMENTS]

  --browse [PATH]                  Browse files and folders (F1 for controls)
  --list [-a] [PATH]               Show a directory summary
  --tree [PATH]                    Explore a directory tree
  --panes [LEFT] [RIGHT]           Open two directory panes
  --find PATH QUERY [--hidden] [--depth N]
                                   Search filenames, up to 16 levels deep

Pane options: --theme NAME, --keys SPEC"
    )?;
    #[cfg(feature = "native-ops")]
    writeln!(
        out,
        "
Native Linux options
  --find PATH QUERY --content      Search inside text files
  --workspace ROOT JOURNAL DATA    Manage files inside a workspace
  --personal JOURNAL [MOUNT ...]   Manage files in explicitly approved locations
  --panes options: --preferences DIR, --bookmarks-in PATH, --bookmarks-out PATH

File tools
  --image-preview PATH
  --archive-list PATH
  --archive-layout PATH [--portable]
  --archive-member PATH NAME --stdout
  --archive-build [--gzip] --stdout NAME PATH ...
  --rename-plan DIRECTORY NEEDLE REPLACEMENT NAME ...
  --suggest-name DIRECTORY NAME
  --tools                          Check companion availability
  --eza [PATH]                     Detailed directory listing
  --7zip-list PATH                 Read-only archive inspection
  --git-status [PATH]              Trusted repositories only; local helpers may run"
    )?;
    writeln!(
        out,
        "
  --version    --edition    --help"
    )
}

pub(super) fn run() -> io::Result<()> {
    let mut args = env::args_os().skip(1);
    match args.next().as_deref().and_then(|x| x.to_str()) {
        Some("--help" | "-h") => {
            end_of_args(&mut args)?;
            print_help()
        }
        Some("--edition") => {
            if args.next().is_some() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "unexpected argument",
                ));
            }
            println!("{}", edition::NAME);
            Ok(())
        }
        Some("--version") => {
            end_of_args(&mut args)?;
            println!("terminal-tree Rust {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some("--list") => {
            let first = args.next();
            let hidden = first.as_deref() == Some(std::ffi::OsStr::new("-a"));
            let path = path_arg(if hidden { args.next() } else { first })?;
            end_of_args(&mut args)?;
            list(&path, hidden)
        }
        Some("--find") => find(args),
        #[cfg(feature = "native-ops")]
        Some("--tools") => {
            end_of_args(&mut args)?;
            for line in terminal_tree::external_tools::availability() {
                println!("{line}");
            }
            Ok(())
        }
        #[cfg(feature = "native-ops")]
        Some(command @ ("--eza" | "--7zip-list" | "--git-status")) => {
            let argument = args.next();
            if command == "--7zip-list" && argument.is_none() {
                return Err(io::Error::other("archive path required"));
            }
            let path = path_arg(argument)?;
            end_of_args(&mut args)?;
            let lines = match command {
                "--eza" => terminal_tree::external_tools::eza(&path)?,
                "--7zip-list" => terminal_tree::external_tools::archive(&path)?,
                _ => terminal_tree::external_tools::git_status(&path)?,
            };
            for line in lines {
                println!("{line}");
            }
            Ok(())
        }
        Some("--rename-plan") => {
            #[cfg(all(feature = "native-ops", target_os = "linux"))]
            {
                let path = path_arg(Some(
                    args.next()
                        .ok_or_else(|| io::Error::other("rename directory required"))?,
                ))?;
                let needle = args
                    .next()
                    .ok_or_else(|| io::Error::other("rename search text required"))?;
                let replacement = args
                    .next()
                    .ok_or_else(|| io::Error::other("rename replacement required"))?;
                let selected: Vec<_> = args.take(1001).collect();
                let plan = terminal_tree::rename_plan::native::preview(
                    &path,
                    &selected,
                    &needle,
                    &replacement,
                )?;
                let mut out = io::BufWriter::new(io::stdout().lock());
                for item in plan {
                    writeln!(
                        out,
                        "{}\t{}",
                        display_name(&item.source),
                        display_name(&item.destination)
                    )?;
                }
                out.flush()
            }
            #[cfg(not(all(feature = "native-ops", target_os = "linux")))]
            {
                Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "rename preview requires the native Linux build",
                ))
            }
        }
        Some("--image-preview") => {
            let path = path_arg(Some(
                args.next()
                    .ok_or_else(|| io::Error::other("image path required"))?,
            ))?;
            if args.next().is_some() {
                return Err(io::Error::other("unexpected image argument"));
            }
            #[cfg(all(feature = "native-ops", target_os = "linux"))]
            {
                let thumbnail =
                    terminal_tree::image_preview::native::thumbnail_file(&path, 80, 40)?;
                let mut out = io::BufWriter::new(io::stdout().lock());
                writeln!(out, "PPM {}x{}", thumbnail.width, thumbnail.height)?;
                for row in thumbnail.rows {
                    writeln!(out, "{row}")?;
                }
                out.flush()
            }
            #[cfg(not(all(feature = "native-ops", target_os = "linux")))]
            {
                let _ = path;
                Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "image file preview requires the native Linux build",
                ))
            }
        }
        Some("--archive-list") => {
            let path = path_arg(Some(
                args.next()
                    .ok_or_else(|| io::Error::other("archive path required"))?,
            ))?;
            if args.next().is_some() {
                return Err(io::Error::other("unexpected archive argument"));
            }
            #[cfg(all(feature = "native-ops", target_os = "linux"))]
            {
                use terminal_tree::archive::{native::list_file, Kind};

                let members = list_file(&path)?;
                let mut out = io::BufWriter::new(io::stdout().lock());
                for member in members {
                    let kind = match member.kind {
                        Kind::File => "file",
                        Kind::Directory => "directory",
                    };
                    writeln!(out, "{}\t{}\t{}", kind, member.size, member.name)?;
                }
                out.flush()
            }
            #[cfg(not(all(feature = "native-ops", target_os = "linux")))]
            {
                let _ = path;
                Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "archive listing requires the native Linux build",
                ))
            }
        }
        Some("--suggest-name") => {
            let directory = path_arg(Some(
                args.next()
                    .ok_or_else(|| io::Error::other("directory required"))?,
            ))?;
            let name = args
                .next()
                .ok_or_else(|| io::Error::other("requested filename required"))?;
            if args.next().is_some() {
                return Err(io::Error::other("unexpected name argument"));
            }
            #[cfg(all(feature = "native-ops", target_os = "linux"))]
            {
                let candidate = terminal_tree::rename_plan::native::suggest(&directory, &name)?;
                println!("{}", display_name(&candidate));
                Ok(())
            }
            #[cfg(not(all(feature = "native-ops", target_os = "linux")))]
            {
                let _ = (directory, name);
                Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "name suggestion requires the native Linux build",
                ))
            }
        }
        Some("--archive-build") => {
            #[cfg(all(feature = "native-ops", target_os = "linux"))]
            {
                let first = args.next();
                let gzip = first.as_deref() == Some(std::ffi::OsStr::new("--gzip"));
                let output_flag = if gzip { args.next() } else { first };
                if output_flag.as_deref() != Some(std::ffi::OsStr::new("--stdout"))
                    || io::stdout().is_terminal()
                {
                    return Err(io::Error::other(
                        "archive build requires --stdout and non-terminal output",
                    ));
                }
                let mut files = Vec::new();
                while let Some(name) = args.next() {
                    if files.len() >= 4096 {
                        return Err(io::Error::other("archive input count exceeded"));
                    }
                    let name = name
                        .into_string()
                        .map_err(|_| io::Error::other("archive member name must be UTF-8"))?;
                    let path =
                        path_arg(Some(args.next().ok_or_else(|| {
                            io::Error::other("archive input path required")
                        })?))?;
                    files.push((name, path));
                }
                let archive = terminal_tree::archive_build::native::build_files(&files)?;
                let archive = if gzip {
                    terminal_tree::archive::native::compress_archive(&archive)?
                } else {
                    archive
                };
                let mut out = io::stdout().lock();
                out.write_all(&archive)?;
                out.flush()
            }
            #[cfg(not(all(feature = "native-ops", target_os = "linux")))]
            {
                Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "archive construction requires the native Linux build",
                ))
            }
        }
        Some("--archive-member") => {
            let path = path_arg(Some(
                args.next()
                    .ok_or_else(|| io::Error::other("archive path required"))?,
            ))?;
            let name = args
                .next()
                .and_then(|name| name.into_string().ok())
                .ok_or_else(|| io::Error::other("UTF-8 archive member name required"))?;
            if args.next().as_deref() != Some(std::ffi::OsStr::new("--stdout"))
                || args.next().is_some()
                || io::stdout().is_terminal()
            {
                return Err(io::Error::other(
                    "archive member requires --stdout and non-terminal output",
                ));
            }
            #[cfg(all(feature = "native-ops", target_os = "linux"))]
            {
                let data = terminal_tree::archive::native::read_member(&path, &name)?;
                let mut out = io::stdout().lock();
                out.write_all(&data)?;
                out.flush()
            }
            #[cfg(not(all(feature = "native-ops", target_os = "linux")))]
            {
                let _ = (path, name);
                Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "archive member access requires the native Linux build",
                ))
            }
        }
        Some("--archive-layout") => {
            let path = path_arg(Some(
                args.next()
                    .ok_or_else(|| io::Error::other("archive path required"))?,
            ))?;
            let portable = match args.next() {
                None => false,
                Some(flag) if flag == "--portable" => true,
                _ => return Err(io::Error::other("unexpected archive argument")),
            };
            if args.next().is_some() {
                return Err(io::Error::other("unexpected archive argument"));
            }
            #[cfg(all(feature = "native-ops", target_os = "linux"))]
            {
                let layout = if portable {
                    terminal_tree::archive::native::portable_layout_file(&path)?
                } else {
                    terminal_tree::archive::native::layout_file(&path)?
                };
                let mut out = io::BufWriter::new(io::stdout().lock());
                writeln!(
                    out,
                    "Layout (read-only): {} directories, {} files, {} payload bytes",
                    layout.directories.len(),
                    layout.files.len(),
                    layout.payload_bytes
                )?;
                for directory in layout.directories {
                    writeln!(out, "directory\t0\t{directory}")?;
                }
                for member in layout.files {
                    writeln!(out, "file\t{}\t{}", member.size, member.name)?;
                }
                out.flush()
            }
            #[cfg(not(all(feature = "native-ops", target_os = "linux")))]
            {
                let _ = (path, portable);
                Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "archive layout requires the native Linux build",
                ))
            }
        }
        Some("--panes") => {
            use terminal_tree::keymap::Keymap;
            let mut paths = Vec::new();
            let mut keymap = None;
            let mut theme = None;
            let mut preferences = None;
            let mut snapshot_in = None;
            let mut snapshot_out = None;
            while let Some(arg) = args.next() {
                if arg == "--keys" {
                    if keymap.is_some() {
                        return Err(io::Error::other("duplicate --keys"));
                    }
                    let spec = args
                        .next()
                        .and_then(|value| value.into_string().ok())
                        .ok_or_else(|| io::Error::other("--keys requires a UTF-8 specification"))?;
                    keymap = Some(Keymap::from_spec(&spec).map_err(|error| {
                        io::Error::new(
                            io::ErrorKind::InvalidInput,
                            format!("invalid navigation keys: {error:?}"),
                        )
                    })?);
                } else if arg == "--theme" {
                    if theme.is_some() {
                        return Err(io::Error::other("duplicate --theme"));
                    }
                    theme = Some(
                        args.next()
                            .and_then(|value| value.into_string().ok())
                            .and_then(|value| terminal_tree::theme::Theme::parse(&value))
                            .ok_or_else(|| {
                                io::Error::other(
                                    "theme must be terminal, dark, light or monochrome",
                                )
                            })?,
                    );
                } else if arg == "--preferences" {
                    if preferences.is_some() {
                        return Err(io::Error::other("duplicate --preferences"));
                    }
                    preferences = Some(path_arg(Some(args.next().ok_or_else(|| {
                        io::Error::other("private existing preferences directory required")
                    })?))?);
                } else if arg == "--bookmarks-in" || arg == "--bookmarks-out" {
                    let slot = if arg == "--bookmarks-in" {
                        &mut snapshot_in
                    } else {
                        &mut snapshot_out
                    };
                    if slot.is_some() {
                        return Err(io::Error::other("duplicate bookmark snapshot option"));
                    }
                    let path = args
                        .next()
                        .ok_or_else(|| io::Error::other("bookmark snapshot path required"))?;
                    *slot = Some(path_arg(Some(path))?);
                } else if paths.len() < 2 {
                    paths.push(arg);
                } else {
                    return Err(io::Error::other("unexpected argument"));
                }
            }
            let mut paths = paths.into_iter();
            let left = path_arg(paths.next())?;
            let right = match paths.next() {
                Some(path) => path_arg(Some(path))?,
                None => left.clone(),
            };
            panes_ui::browse(
                &left,
                &right,
                keymap,
                theme,
                preferences.as_deref(),
                snapshot_in.as_deref(),
                snapshot_out.as_deref(),
            )
        }
        Some("--tree") => {
            let path = path_arg(args.next())?;
            if args.next().is_some() {
                return Err(io::Error::other("unexpected argument"));
            }
            tree_ui::browse(&path)
        }
        Some("--browse") => {
            let p = path_arg(args.next())?;
            end_of_args(&mut args)?;
            #[cfg(feature = "native-ops")]
            {
                browse(p, false, None).map(|_| ())
            }
            #[cfg(not(feature = "native-ops"))]
            {
                browse(p, false).map(|_| ())
            }
        }
        #[cfg(feature = "native-ops")]
        Some("--personal") => {
            // Never infer a broader scope from an arbitrary HOME override.
            // Resolve the real account's home from NSS via getent, then require
            // the environment HOME to agree. This mode is explicitly opt-in.
            let uid = rustix::process::getuid().as_raw();
            let output = std::process::Command::new("/usr/bin/getent")
                .args(["passwd", &uid.to_string()])
                .output()?;
            if !output.status.success() {
                return Err(io::Error::other("account home unavailable"));
            }
            let raw = output.stdout.strip_suffix(b"\n").unwrap_or(&output.stdout);
            let fields: Vec<_> = raw.split(|b| *b == b':').collect();
            if fields.len() != 7 || fields[2] != uid.to_string().as_bytes() || raw.contains(&b'\n')
            {
                return Err(io::Error::other("ambiguous account record"));
            }
            let home = PathBuf::from(std::ffi::OsStr::from_bytes(fields[5]));
            if env::var_os("HOME").as_deref() != Some(home.as_os_str()) {
                return Err(io::Error::other("HOME differs from account home"));
            }
            let journal =
                path_arg(Some(args.next().ok_or_else(|| {
                    io::Error::other("private existing journal path required")
                })?))?;
            let extras = args
                .map(|p| path_arg(Some(p)))
                .collect::<io::Result<Vec<_>>>()?;
            let w = workspace::Workspace::personal(&home, &journal, &extras)?;
            browse(home, false, Some(w)).map(|_| ())
        }
        #[cfg(feature = "native-ops")]
        Some("--workspace") => {
            let root = path_arg(args.next())?;
            let journal = path_arg(Some(
                args.next()
                    .ok_or_else(|| io::Error::other("journal path required"))?,
            ))?;
            let data = path_arg(Some(
                args.next()
                    .ok_or_else(|| io::Error::other("data path required"))?,
            ))?;
            if args.next().is_some() {
                return Err(io::Error::other("unexpected argument"));
            }
            let w = workspace::Workspace::new(&root, &journal, &data)?;
            browse(w.root().to_path_buf(), false, Some(w)).map(|_| ())
        }
        #[cfg(feature = "native-ops")]
        Some("--result") => {
            let result = path_arg(Some(
                args.next()
                    .ok_or_else(|| io::Error::other("result path required"))?,
            ))?;
            if args.next().is_some() {
                return Err(io::Error::other("unexpected argument"));
            }
            result_channel(result)
        }
        #[cfg(unix)]
        Some("--scan-paths0") => {
            let p = path_arg(args.next())?;
            let order = match args.next().as_deref().and_then(|s| s.to_str()) {
                Some("name") => Order::Name,
                Some("size") => Order::Size,
                Some("date") => Order::Date,
                Some("type") => Order::Type,
                _ => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "invalid sort order",
                    ))
                }
            };
            let hidden = match args.next().as_deref().and_then(|s| s.to_str()) {
                Some("0") => false,
                Some("1") => true,
                _ => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "hidden must be 0 or 1",
                    ))
                }
            };
            let query =
                args.next().unwrap_or_default().into_string().map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidInput, "invalid UTF-8 query")
                })?;
            end_of_args(&mut args)?;
            let mut out = io::BufWriter::new(io::stdout().lock());
            for entry in scan(&p, hidden, order, &query)? {
                out.write_all(entry.path.as_os_str().as_bytes())?;
                out.write_all(&[0])?;
            }
            out.flush()
        }
        Some("--scan-count") => {
            let p = path_arg(args.next())?;
            let repeats = match args.next() {
                None => 1,
                Some(value) => value
                    .to_str()
                    .and_then(|text| text.parse::<usize>().ok())
                    .filter(|n| (1..=1000).contains(n))
                    .ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidInput, "repeat count must be 1..1000")
                    })?,
            };
            end_of_args(&mut args)?;
            let mut count = 0;
            for _ in 0..repeats {
                count += std::hint::black_box(scan(&p, false, Order::Name, "")?).len();
            }
            println!("{count}");
            Ok(())
        }
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "unsupported or missing command. Run terminal-tree --help for usage",
        )),
    }
}
