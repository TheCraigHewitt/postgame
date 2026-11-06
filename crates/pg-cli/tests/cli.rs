use std::process::Command;

#[test]
fn version_includes_the_swift_shim() {
    let out = Command::new(env!("CARGO_BIN_EXE_pg"))
        .arg("--version")
        .output()
        .unwrap();
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    assert_eq!(text.trim(), "pg 0.1.0 (PGAudio 0.1.0)");
}

#[test]
fn record_requires_exactly_one_target() {
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_pg"))
            .args(["record", "--out", "/nonexistent/pg-test"])
            .args(args)
            .output()
            .unwrap()
    };
    assert!(!run(&[]).status.success());
    let both = run(&["--all-system", "--pid", "1"]);
    assert!(String::from_utf8_lossy(&both.stderr).contains("cannot be used with"));
}

#[test]
fn sync_check_reports_a_missing_recording() {
    let out = Command::new(env!("CARGO_BIN_EXE_pg"))
        .args(["sync-check", "/nonexistent/pg-test"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).starts_with("error:"));
}
