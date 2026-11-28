use std::process::{Command, Output};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_terminal-tree"))
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn help_describes_the_installed_build() {
    for flag in ["--help", "-h"] {
        let output = run(&[flag]);
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        let help = String::from_utf8(output.stdout).unwrap();
        assert!(help.contains("--browse"));
        assert_eq!(help.contains("--workspace"), cfg!(feature = "native-ops"));
    }
}

#[test]
fn invalid_arguments_fail_before_running_the_command() {
    for args in [
        &["--version", "extra"][..],
        &["--help", "extra"],
        &["--list", ".", "extra"],
        &["--list", ""],
        &["--browse", ".", "extra"],
        &["--scan-count", ".", "ten"],
        &["--scan-count", ".", "2", "extra"],
    ] {
        let output = run(args);
        assert!(!output.status.success(), "{args:?}");
        assert!(output.stdout.is_empty(), "{args:?}");
        assert!(!output.stderr.is_empty(), "{args:?}");
    }
}
