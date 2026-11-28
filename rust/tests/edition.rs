use std::process::Command;

#[test]
fn edition_matches_build_configuration() {
    let output = Command::new(env!("CARGO_BIN_EXE_terminal-tree"))
        .arg("--edition")
        .output()
        .expect("run edition query");
    let expected: &[u8] = if cfg!(feature = "paranoid-preview") {
        b"paranoid-preview\n"
    } else {
        b"standard\n"
    };
    assert!(output.status.success());
    assert_eq!(output.stdout, expected);
    assert!(output.stderr.is_empty());
}

#[test]
fn edition_query_rejects_extra_arguments() {
    let output = Command::new(env!("CARGO_BIN_EXE_terminal-tree"))
        .args(["--edition", "unexpected"])
        .output()
        .expect("run invalid edition query");
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
}
