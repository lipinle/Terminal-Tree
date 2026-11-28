use terminal_tree::image_preview::ppm_thumbnail;

#[test]
fn renders_dark_and_light_pixels_without_escape_sequences() {
    let mut image = b"P6\n# comment\n2 1\n255\n".to_vec();
    image.extend_from_slice(&[0, 0, 0, 255, 255, 255]);
    let preview = ppm_thumbnail(&image, 80, 40).unwrap();
    assert_eq!((preview.width, preview.height), (2, 1));
    assert_eq!(preview.columns, 2);
    assert_eq!(preview.rows, vec![" @"]);
}

#[test]
fn preserves_first_raster_whitespace_and_hash_bytes() {
    let mut image = b"P6\n1 1\n255\n".to_vec();
    image.extend_from_slice(b" #\n");
    assert!(ppm_thumbnail(&image, 1, 1).is_ok());
    image.push(0);
    assert!(ppm_thumbnail(&image, 1, 1).is_err());
}

#[test]
fn rejects_invalid_headers_and_raster_lengths() {
    for image in [
        b"P3\n1 1\n255\n0 0 0".as_slice(),
        b"P6\n0 1\n255\n",
        b"P6\n4096 4096\n255\n",
        b"P6\n1 1\n65535\n",
        b"P6\n1 1\n255",
        b"P6\n1 1\n255\n\x00\x00",
        b"P6\n1 1\n255\n\x00\x00\x00\x00",
        b"P6\n-1 1\n255\n",
    ] {
        assert!(ppm_thumbnail(image, 80, 40).is_err());
    }
    let mut oversized = b"P6\n#".to_vec();
    oversized.extend(std::iter::repeat(b'x').take(4096));
    oversized.extend_from_slice(b"\n1 1\n255\n\x00\x00\x00");
    assert!(ppm_thumbnail(&oversized, 80, 40).is_err());
}

#[test]
fn bounds_output_for_tall_and_wide_images() {
    for (w, h) in [(1, 100), (100, 1), (200, 200)] {
        let mut image = format!("P6\n{w} {h}\n255\n").into_bytes();
        image.extend(std::iter::repeat(128).take(w * h * 3));
        let preview = ppm_thumbnail(&image, 20, 5).unwrap();
        assert!((1..=20).contains(&preview.columns));
        assert!((1..=5).contains(&preview.rows.len()));
        assert!(preview.rows.iter().all(|row| row.len() == preview.columns));
        assert!(ppm_thumbnail(&image, 0, 5).is_err());
        assert!(ppm_thumbnail(&image, 20, 41).is_err());
    }
}

#[cfg(all(feature = "native-ops", target_os = "linux"))]
#[test]
fn native_image_cli_is_read_only_and_refuses_aliases() {
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::process::Command;
    use terminal_tree::image_preview::native::thumbnail_file;

    let root = std::env::temp_dir().join(format!("tt-image-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(root.clone());
    let path = root.join("image.ppm");
    let bytes = b"P6\n2 1\n255\n\x00\x00\x00\xff\xff\xff";
    fs::write(&path, bytes).unwrap();
    assert_eq!(thumbnail_file(&path, 80, 40).unwrap().rows, [" @"]);
    let run = |path: &std::path::Path| {
        Command::new(env!("CARGO_BIN_EXE_terminal-tree"))
            .arg("--image-preview")
            .arg(path)
            .output()
            .unwrap()
    };
    let output = run(&path);
    assert!(output.status.success());
    assert_eq!(output.stdout, b"PPM 2x1\n @\n");
    symlink(&path, root.join("alias")).unwrap();
    symlink(&root, root.join("parent-alias")).unwrap();
    for bad in [root.join("alias"), root.join("parent-alias/image.ppm"), root.clone()] {
        let output = run(&bad);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
    }
    let bad = root.join("bad.ppm");
    fs::write(&bad, b"P6\n1 1\n255\n").unwrap();
    let output = run(&bad);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert_eq!(fs::read(&path).unwrap(), bytes);
}
