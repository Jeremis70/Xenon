//! CLI smoke tests (`check` exit codes, etc.).

use assert_cmd::Command;
use std::io::Write;
use std::path::PathBuf;

#[test]
fn check_succeeds_on_tests_main_xe() {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("../tests/main.xe");
    Command::cargo_bin("xenonc")
        .expect("cargo_bin xenonc")
        .args(["check", p.to_str().expect("utf8 path")])
        .assert()
        .success();
}

#[test]
fn check_fails_on_semantic_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("bad.xe");
    let mut f = std::fs::File::create(&path).expect("create");
    writeln!(f, "fn f()->u32 {{ while 1 {{ }} return 0; }}").expect("write");
    drop(f);

    Command::cargo_bin("xenonc")
        .expect("cargo_bin xenonc")
        .args(["check", path.to_str().expect("utf8 path")])
        .assert()
        .failure();
}

#[test]
fn check_json_error_format_is_valid_json_line() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("bad.xe");
    std::fs::write(&path, "fn f()->u32 { while 1 { } return 0; }\n").expect("write");

    let out = Command::cargo_bin("xenonc")
        .expect("cargo_bin xenonc")
        .args([
            "check",
            path.to_str().expect("utf8 path"),
            "--error-format",
            "json",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&out.get_output().stderr);
    let line = stderr.lines().next().expect("one json line");
    let v: serde_json::Value = serde_json::from_str(line).expect("stderr is json");
    assert_eq!(v["type"], "error");
}
