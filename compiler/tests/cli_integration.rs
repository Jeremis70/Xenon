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
fn check_emits_runtime_mir_without_codegen() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("mir.xe");
    std::fs::write(
        &path,
        "#[entry]\nfn main() -> i32 { let i32 value = 40 + 2; return value; }\n",
    )
    .expect("write");

    let output = Command::cargo_bin("xenonc")
        .expect("cargo_bin xenonc")
        .args(["check", path.to_str().expect("utf8 path"), "--emit", "mir"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let mir = String::from_utf8(output).expect("MIR output is UTF-8");
    assert!(mir.contains("phase: runtime"), "{mir}");
    assert!(mir.contains("fn main() -> i32"), "{mir}");
}

#[test]
fn compile_emits_mir_without_native_codegen() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = dir.path().join("mir.xe");
    let out_dir = dir.path().join("build");
    std::fs::create_dir_all(&out_dir).expect("create out dir");
    std::fs::write(&source, "#[entry]\nfn main() -> i32 { return 7; }\n").expect("write");

    Command::cargo_bin("xenonc")
        .expect("cargo_bin xenonc")
        .args([
            "compile",
            source.to_str().expect("utf8 path"),
            "--emit",
            "mir",
            "--out-dir",
            out_dir.to_str().expect("utf8 path"),
        ])
        .assert()
        .success();

    let mir = std::fs::read_to_string(out_dir.join("out.mir")).expect("read MIR output");
    assert!(mir.contains("phase: runtime"), "{mir}");
    assert!(!out_dir.join("out.o").exists());
}

#[cfg(not(feature = "llvm-backend"))]
#[test]
fn native_codegen_requires_the_llvm_feature() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = dir.path().join("program.xe");
    std::fs::write(&source, "#[entry]\nfn main() -> i32 { return 0; }\n").expect("write");

    let output = Command::cargo_bin("xenonc")
        .expect("cargo_bin xenonc")
        .args(["compile", source.to_str().expect("utf8 path")])
        .assert()
        .failure()
        .get_output()
        .stderr
        .clone();
    let stderr = String::from_utf8(output).expect("stderr is UTF-8");
    assert!(stderr.contains("llvm-backend"), "{stderr}");
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

/// Runs `command` on `source`, expecting it to fail with exactly one JSON
/// diagnostic, and returns that diagnostic.
fn single_json_diagnostic(command: &str, source: &str) -> serde_json::Value {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("bad.xe");
    std::fs::write(&path, source).expect("write");
    let output = Command::cargo_bin("xenonc")
        .expect("cargo_bin xenonc")
        .args([
            command,
            path.to_str().expect("utf8 path"),
            "--emit",
            "mir",
            "--error-format",
            "json",
        ])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&output.get_output().stderr);
    serde_json::from_str(stderr.trim()).unwrap_or_else(|_| panic!("JSON: {stderr}"))
}

#[test]
fn semantic_errors_precede_entry_point_errors() {
    for source in [
        "fn f()->i32 { return missing; }",
        "#[entry] fn f()->u32 { return missing; }",
        "#[entry] fn f(i32 value)->i32 { return missing; }",
    ] {
        for command in ["check", "compile"] {
            let diagnostic = single_json_diagnostic(command, source);
            assert_eq!(diagnostic["code"], "semantic", "{source}");
            assert_eq!(
                diagnostic["message"], "undefined variable `missing`",
                "{source}"
            );
        }
    }
}

/// Attributes are checked while collecting items, before any body.
#[test]
fn attribute_errors_precede_body_errors() {
    for (source, message) in [
        (
            "#[unknown] fn f()->i32 { return missing; }",
            "unknown attribute `unknown`",
        ),
        (
            "#[entry] fn f()->i32 { return missing; } #[entry] fn g()->i32 { return 0; }",
            "multiple `#[entry]` functions",
        ),
    ] {
        for command in ["check", "compile"] {
            let diagnostic = single_json_diagnostic(command, source);
            assert_eq!(diagnostic["code"], "semantic", "{source}");
            let found = diagnostic["message"].as_str().expect("message");
            assert!(found.starts_with(message), "{source}: {found}");
        }
    }
}

#[test]
fn known_panics_are_compile_errors() {
    for (source, message) in [
        (
            "#[entry] fn main() -> i32 { return 1 / 0; }",
            "attempt to divide by zero",
        ),
        (
            "#[entry] fn main() -> i32 { let u8 x = 200 + 100; return 0; }",
            "attempt to add with overflow",
        ),
        (
            "#[entry] fn main() -> i32 { let i32 z = 0; return 7 % z; }",
            "attempt to calculate the remainder with a divisor of zero",
        ),
    ] {
        for command in ["check", "compile"] {
            let diagnostic = single_json_diagnostic(command, source);
            assert_eq!(diagnostic["code"], "known-panic", "{source}");
            let found = diagnostic["message"].as_str().expect("message");
            assert!(found.ends_with(message), "{source}: {found}");
        }
    }
}

#[test]
fn entry_point_is_still_validated_after_successful_typecheck() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("no_entry.xe");
    std::fs::write(&path, "fn f()->i32 { return 0; }").expect("write");
    Command::cargo_bin("xenonc")
        .expect("cargo_bin xenonc")
        .args([
            "check",
            path.to_str().expect("utf8 path"),
            "--error-format",
            "json",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("no `#[entry]` function found"));
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
#[cfg(feature = "llvm-backend")]
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
#[cfg(feature = "llvm-backend")]
fn compile_and_run(src: &str) -> i32 {
    run_status(src).code().expect("process exited with a code")
}

/// Compiles `src` and runs the resulting binary, returning how it ended.
#[cfg(feature = "llvm-backend")]
fn run_status(src: &str) -> std::process::ExitStatus {
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

    std::process::Command::new(out_dir.join("out"))
        .status()
        .expect("run compiled binary")
}

/// Builds a program whose target and right-hand side each append a digit to a
/// log, so the exit code spells out the order they were evaluated in. `1` is
/// the assignment target, `2` is the right-hand side.
#[cfg(feature = "llvm-backend")]
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
#[cfg(feature = "llvm-backend")]
fn compound_assignment_evaluates_target_before_value() {
    let code = compile_and_run(&evaluation_order_program(
        "*target(@log, @cell) += bump(@log);",
    ));
    assert_eq!(code, 12, "expected target-then-value evaluation order");
}

#[test]
#[cfg(feature = "llvm-backend")]
fn plain_assignment_evaluates_target_before_value() {
    let code = compile_and_run(&evaluation_order_program(
        "*target(@log, @cell) = bump(@log);",
    ));
    assert_eq!(code, 12, "expected target-then-value evaluation order");
}

#[cfg(feature = "llvm-backend")]
#[test]
fn backend_runs_wide_integer_shift_float_and_branch_operations() {
    let code = compile_and_run(concat!(
        "#[entry]\n",
        "fn main() -> i32 {\n",
        "    let i128 wide = 18446744073709551616;\n",
        "    let i32 shifted = 1;\n",
        "    shifted = shifted << 2;\n",
        "    let f64 fraction = 1.5;\n",
        "    if fraction > 1.0 and wide > 0 { return shifted + 1; }\n",
        "    return 0;\n",
        "}\n",
    ));
    assert_eq!(code, 5);
}

#[cfg(feature = "llvm-backend")]
#[test]
fn backend_runs_custom_entry_with_user_main_collision() {
    let code = compile_and_run(concat!(
        "#[entry]\n",
        "fn start() -> i32 { return 9; }\n",
        "fn main() -> i32 { return 0; }\n",
    ));
    assert_eq!(code, 9);
}

#[cfg(feature = "llvm-backend")]
#[test]
fn backend_compares_pointer_values() {
    let code = compile_and_run(concat!(
        "#[entry]\n",
        "fn main() -> i32 {\n",
        "    let i32 value = 1;\n",
        "    let *i32 pointer = @value;\n",
        "    if pointer == @value { return 3; }\n",
        "    return 0;\n",
        "}\n",
    ));
    assert_eq!(code, 3);
}

#[cfg(feature = "llvm-backend")]
#[test]
fn backend_runs_conditional_forms() {
    // Each form adds a distinct digit when it takes the expected path.
    let code = compile_and_run(concat!(
        "#[entry]\n",
        "fn main() -> i32 {\n",
        "    let bool yes = true;\n",
        "    let bool no = false;\n",
        "    let i32 total = 0;\n",
        "    if no { total += 100; } else if yes { total += 1; } else { total += 100; }\n",
        "    unless no { total += 2; }\n",
        "    unless yes { total += 100; }\n",
        "    total += 4 if yes;\n",
        "    total += 100 if no;\n",
        "    total += 8 unless no;\n",
        "    total += 100 unless yes;\n",
        "    total += 16 if no else 32;\n",
        "    return total;\n",
        "}\n",
    ));
    assert_eq!(code, 47);
}

#[cfg(feature = "llvm-backend")]
#[test]
fn backend_runs_zero_sized_integers() {
    // Each check adds a distinct bit when it holds.
    let code = compile_and_run(concat!(
        "fn id(u0 x) -> u0 { return x; }\n",
        "fn mix(i0 x) -> i0 { return -x + x * x - ~x; }\n",
        "#[entry]\n",
        "fn main() -> i32 {\n",
        "    let u0 a = 0;\n",
        "    let i0 b = 0;\n",
        "    let u0 c = id(a);\n",
        "    let i0 d = mix(b);\n",
        "    let *u0 p = @c;\n",
        "    *p = a;\n",
        "    let i32 five = 5;\n",
        "    let i8 minus_three = -3;\n",
        "    let i32 x = 3;\n",
        "    let i32 total = 0;\n",
        "    total += 1 if c == a;\n",
        "    total += 2 if d >= b;\n",
        "    total += 4 unless d != b;\n",
        "    total += 8 if a < five;\n",
        "    total += 16 if b > minus_three;\n",
        "    total += 32 if x << a == 3;\n",
        "    return total;\n",
        "}\n",
    ));
    assert_eq!(code, 63);
}

/// A zero-sized divisor always holds `0`, so the division always traps.
#[test]
fn zero_sized_division_is_a_known_panic() {
    let diagnostic = single_json_diagnostic(
        "check",
        "fn div(u0 a, u0 b) -> u0 { return a / b; }\n\
         #[entry]\n\
         fn main() -> i32 { let u0 z = div(0, 0); return 0; }\n",
    );
    assert_eq!(diagnostic["code"], "known-panic");
}

/// No shift amount is below the bit width `0`.
#[test]
fn zero_sized_shifts_are_known_panics() {
    for (ty, op) in [("u0", "<<"), ("u0", ">>"), ("i0", "<<"), ("i0", ">>")] {
        let diagnostic = single_json_diagnostic(
            "check",
            &format!(
                "fn f() -> {ty} {{ return 0 {op} 0; }}\n\
                 #[entry]\n\
                 fn main() -> i32 {{ let {ty} z = f(); return 0; }}\n",
            ),
        );
        assert_eq!(diagnostic["code"], "known-panic", "`0 {op} 0` as `{ty}`");
        assert!(
            diagnostic["message"]
                .as_str()
                .is_some_and(|message| message.ends_with("attempt to shift out of range")),
            "{diagnostic}"
        );
    }
}

#[cfg(feature = "llvm-backend")]
#[test]
fn backend_complements_literal_zero_sized_integers() {
    for ty in ["u0", "i0"] {
        let code = compile_and_run(&format!(
            "fn f() -> {ty} {{ return ~0; }}\n\
             #[entry]\n\
             fn main() -> i32 {{ let {ty} z = f(); return 7; }}\n",
        ));
        assert_eq!(code, 7, "`~0` as `{ty}`");
    }
}
