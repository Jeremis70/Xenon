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

/// Compiles `src` and runs the resulting binary, returning its exit code.
fn compile_and_run(src: &str) -> i32 {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("prog.xe");
    let out_dir = dir.path().join("build");
    std::fs::create_dir_all(&out_dir).expect("create out dir");
    std::fs::write(&path, src).expect("write");

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

    let status = std::process::Command::new(out_dir.join("out"))
        .status()
        .expect("run compiled binary");
    status.code().expect("process exited with a code")
}

/// Builds a program whose target and right-hand side each append a digit to a
/// log, so the exit code spells out the order they were evaluated in. `1` is
/// the assignment target, `2` is the right-hand side.
fn evaluation_order_program(assignment: &str) -> String {
    format!(
        concat!(
            "#[entry]\n",
            "fn main() -> i32 {{\n",
            "    let i32 log = 0;\n",
            "    let i32 cell = 0;\n",
            "    {assignment}\n",
            "    return log;\n",
            "}}\n",
            "fn target(&i32 log, &i32 cell) -> *i32 {{\n",
            "    log = log * 10 + 1;\n",
            "    return @cell;\n",
            "}}\n",
            "fn bump(&i32 log) -> i32 {{\n",
            "    log = log * 10 + 2;\n",
            "    return 1;\n",
            "}}\n",
        ),
        assignment = assignment
    )
}

/// A compound assignment must evaluate its target before its right-hand side,
/// exactly like the plain assignment below. Lowering the right-hand side first
/// would yield `21`.
#[test]
fn compound_assignment_evaluates_target_before_value() {
    let code = compile_and_run(&evaluation_order_program(
        "*target(@log, @cell) += bump(@log);",
    ));
    assert_eq!(code, 12, "expected target-then-value evaluation order");
}

#[test]
fn plain_assignment_evaluates_target_before_value() {
    let code = compile_and_run(&evaluation_order_program(
        "*target(@log, @cell) = bump(@log);",
    ));
    assert_eq!(code, 12, "expected target-then-value evaluation order");
}
