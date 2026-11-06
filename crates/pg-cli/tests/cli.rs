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
