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

/// End-to-end compile of a program exercising pointers, references, double
/// indirection, and reference parameters.
#[test]
fn compile_succeeds_on_pointers_and_references() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("ptr.xe");
    let out_dir = dir.path().join("build");
    std::fs::create_dir_all(&out_dir).expect("create out dir");
    std::fs::write(
        &path,
        concat!(
            "fn increment(&i32 value) -> i32 {\n",
            "    value = value + 1;\n",
            "    return value;\n",
            "}\n",
            "#[entry]\n",
            "fn main() -> i32 {\n",
            "    let i32 x = 42;\n",
            "    let *i32 p = @x;\n",
            "    *p = 10;\n",
            "    let &i32 r = @x;\n",
            "    r = r + 5;\n",
            "    let **i32 pp = @p;\n",
            "    **pp = **pp + 1;\n",
            "    return increment(@x);\n",
            "}\n",
        ),
    )
    .expect("write");

    Command::cargo_bin("xenonc")
        .expect("cargo_bin xenonc")
        .args([
            "compile",
            path.to_str().expect("utf8 path"),
            "--out-dir",
            out_dir.to_str().expect("utf8 path"),
        ])
        .assert()
        .success();
}

/// Address literals wider than the target pointer must be rejected at codegen.
#[test]
fn compile_fails_on_oversized_address_literal() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("big.xe");
    let out_dir = dir.path().join("build");
    std::fs::create_dir_all(&out_dir).expect("create out dir");
    std::fs::write(
        &path,
        "#[entry]\nfn main() -> i32 { let *u32 dev = @0xFFFFFFFFFFFFFFFFFF; return 0; }\n",
    )
    .expect("write");

    Command::cargo_bin("xenonc")
        .expect("cargo_bin xenonc")
        .args([
            "compile",
            path.to_str().expect("utf8 path"),
            "--out-dir",
            out_dir.to_str().expect("utf8 path"),
        ])
        .assert()
        .failure();
}
