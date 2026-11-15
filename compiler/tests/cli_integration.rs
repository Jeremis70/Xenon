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

#[cfg(feature = "llvm-backend")]
#[test]
fn backend_runs_arrays_with_copy_equality_and_nested_places() {
    let source = "
        struct Cell { [2]u8 values; }
        impl Cell { fn bump(&self) { self.values[1] += 1; } }
        fn make()->[2]u8 { return [3, 4]; }
        fn get([2]u8 a, u1 i)->u8 { return a[i]; }
        #[entry] fn main()->u8 {
            let [2][2]u8 a = [make(), [5, 6]];
            let [2][2]u8 copy = a;
            a[0][1]++;
            if copy != [[3, 4], [5, 6]] { return 1; }
            if a == copy { return 2; }
            let Cell c = Cell(values = a[0]);
            c.bump();
            let &[2]u8 row = @a[1];
            row[0] = 7;
            let *[2][2]u8 pointer = @a;
            (*pointer)[1][1] = 8;
            let [0]u8 empty = [];
            if empty != [] { return 3; }
            let [2]u0 zeros = [0, 0];
            zeros[1] = 0;
            if zeros != [0, 0] { return 4; }
            let [2]&u8 refs = [@row[0], @row[1]];
            refs[0] = 9;
            return get(c.values, 1) + a[1][0] + a[1][1];
        }";
    for args in [vec![], vec!["-O", "3"]] {
        assert_eq!(run_status_with(source, &args).code(), Some(23));
    }
}

#[cfg(feature = "llvm-backend")]
#[test]
fn array_comparison_loops_preserve_values_and_evaluation_order() {
    let source = "
        struct Cell { [2]u8 values; bool flag; }
        fn id(&[2]u8 a)->&[2]u8 { return @a; }
        fn left(&[2]u8 a, &u32 log)->[2]u8 { log = log * 10 + 1; return a; }
        fn right(&[2]u8 a, &u32 log)->[2]u8 { log = log * 10 + 2; a[0] = 3; return a; }
        #[entry] fn main()->u8 {
            let [2]u8 a = [1, 2];
            let u32 log = 0;
            if left(@a, @log) == right(@a, @log) { return 1; }
            if log != 12 { return 2; }
            a = [1, 2];
            if a == right(@a, @log) { return 3; }
            if log != 122 { return 4; }
            if id(@a)[0] != 3 { return 5; }
            let [2]u8 zero = loop { break; };
            if zero != [0, 0] { return 6; }
            let Cell cell = loop { break; };
            if cell != Cell(values = [0, 0], flag = false) { return 7; }
            let u8 x = 0;
            if [x, 0] != [0, 0] { return 8; }
            return 9;
        }";
    for args in [vec![], vec!["-O", "3"]] {
        assert_eq!(run_status_with(source, &args).code(), Some(9));
    }
}

#[cfg(feature = "llvm-backend")]
#[test]
fn arrays_snapshot_assignment_targets_and_broadcast_values_once() {
    let source = "
        fn base(&[5]u8 a, &u32 log)->*[5]u8 { log = log * 10 + 1; return @a; }
        fn index(&u32 log, i32 i)->i32 { log = log * 10 + 2; return i; }
        fn value(&u32 log)->u8 { log = log * 10 + 3; return 7; }
        fn change(&i32 i)->u8 { i = 1; return 9; }
        #[entry] fn main()->u8 {
            let [5]u8 a = [0, 0, 0, 0, 0];
            let u32 log = 0;
            (*base(@a, @log))[index(@log, 0), index(@log, 2), index(@log, 4)] = value(@log);
            if log != 12223 { return 1; }
            if a != [7, 0, 7, 0, 7] { return 2; }
            let i32 i = 0;
            a[i] = change(@i);
            if a[0] != 9 or a[1] != 0 { return 3; }
            a[0, 0] = value(@log);
            if log != 122233 { return 4; }
            let [2][2]u8 grid = [[0, 0], [0, 0]];
            grid[0, 1] = [1, 2];
            grid[0][0] = 3;
            if grid != [[3, 2], [1, 2]] { return 5; }
            return a[0] + a[2] + a[4];
        }";
    assert_eq!(compile_and_run(source), 21);
}

#[cfg(feature = "llvm-backend")]
#[test]
fn arrays_trap_on_runtime_bounds_failures() {
    for (ty, value) in [("i8", "-1"), ("u8", "2"), ("u128", "18446744073709551616")] {
        for operation in [
            "return a[i];",
            "a[i] = 3; return 0;",
            "a[0, i] = 3; return 0;",
        ] {
            let source = format!(
                "fn access({ty} i)->u8 {{ let [2]u8 a = [1, 2]; {operation} }}
                 #[entry] fn main()->u8 {{ return access({value}); }}"
            );
            let status = run_status(&source);
            let trapped = if cfg!(windows) {
                // Windows reports LLVM's trap as STATUS_ILLEGAL_INSTRUCTION.
                status.code() == Some(0xC000_001D_u32 as i32)
            } else {
                status.code().is_none()
            };
            assert!(trapped, "{ty}: {operation} should trap: {status:?}");
        }
    }
}

#[test]
fn arrays_require_initialization_and_report_bounds_in_json() {
    for target in ["a[0]", "a[0, 1]"] {
        let diagnostic = single_json_diagnostic(
            "check",
            &format!(
                "#[entry] fn main() {{ }} fn f()->[2]u8 a {{ {target} = 1; a = [1, 2]; return; }}"
            ),
        );
        assert_eq!(diagnostic["code"], "mir-analysis", "{diagnostic}");
    }
    let diagnostic = single_json_diagnostic(
        "check",
        "#[entry] fn main() { let [2]u8 a = [1, 2]; let i32 i = 1 + 1; a[i] = 0; }",
    );
    assert_eq!(diagnostic["code"], "known-panic", "{diagnostic}");
    assert!(
        diagnostic["message"]
            .as_str()
            .expect("message")
            .contains("array index out of bounds")
    );
}

#[test]
fn check_emits_runtime_mir_without_codegen() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("mir.xe");
    std::fs::write(
        &path,
        "#[entry]\nfn main() -> u8 { let u8 value = 40 + 2; return value; }\n",
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
    assert!(mir.contains("fn main() -> u8"), "{mir}");
}

#[test]
fn compile_emits_mir_without_native_codegen() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = dir.path().join("mir.xe");
    let out_dir = dir.path().join("build");
    std::fs::create_dir_all(&out_dir).expect("create out dir");
    std::fs::write(&source, "#[entry]\nfn main() -> u8 { return 7; }\n").expect("write");

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
    std::fs::write(&source, "#[entry]\nfn main() {}\n").expect("write");

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
            "#[entry] fn main() -> u8 { return 1 / 0; }",
            "attempt to divide by zero",
        ),
        (
            "#[entry] fn main() { let u8 x = 200 + 100; }",
            "attempt to add with overflow",
        ),
        (
            "#[entry] fn main() -> u8 { let u8 z = 0; return 7 % z; }",
            "attempt to calculate the remainder with a divisor of zero",
        ),
        (
            "#[entry] fn main() -> u8 { let i32 n = 300; return n as u8; }",
            "attempt to cast a value the target type cannot represent exactly",
        ),
        (
            "#[entry] fn main() { let f64 x = 2.5; let i32 n = x as i32; }",
            "attempt to cast a value the target type cannot represent exactly",
        ),
        (
            "#[entry] fn main() { let i64 big = 16777217; let f32 x = big as f32; }",
            "attempt to cast a value the target type cannot represent exactly",
        ),
        (
            "#[entry] fn main() { let f16 x = 2.5; let i32 n = x as i32; }",
            "attempt to cast a value the target type cannot represent exactly",
        ),
        (
            "#[entry] fn main() { let f64 x = 0.1; let f16 y = x as f16; }",
            "attempt to cast a value the target type cannot represent exactly",
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
            "fn main() {\n",
            "    let i32 x = 42;\n",
            "    let *i32 p = @x;\n",
            "    *p = 10;\n",
            "    let &i32 r = @x;\n",
            "    r = r + 5;\n",
            "    let **i32 pp = @p;\n",
            "    **pp = **pp + 1;\n",
            "    increment(@x);\n",
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
        "#[entry]\nfn main() { let *u32 dev = @0xFFFFFFFFFFFFFFFFFF; }\n",
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
    run_status_with(src, &[])
}

/// Compiles `src` with the extra `compile` arguments `args` and runs the
/// resulting binary, returning how it ended.
#[cfg(feature = "llvm-backend")]
fn run_status_with(src: &str, args: &[&str]) -> std::process::ExitStatus {
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
        .args(args)
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
            "fn main() -> u8 {{\n",
            "    let u8 log = 0;\n",
            "    let u8 cell = 0;\n",
            "    {assignment}\n",
            "    return log;\n",
            "}}\n",
            "fn target(&u8 log, &u8 cell) -> *u8 {{\n",
            "    log = log * 10 + 1;\n",
            "    return @cell;\n",
            "}}\n",
            "fn bump(&u8 log) -> u8 {{\n",
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
        "fn main() -> u8 {\n",
        "    let i128 wide = 18446744073709551616;\n",
        "    let u8 shifted = 1;\n",
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
        "fn start() -> u8 { return 9; }\n",
        "fn main() -> i32 { return 0; }\n",
    ));
    assert_eq!(code, 9);
}

#[cfg(feature = "llvm-backend")]
#[test]
fn backend_compares_pointer_values() {
    let code = compile_and_run(concat!(
        "#[entry]\n",
        "fn main() -> u8 {\n",
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
        "fn main() -> u8 {\n",
        "    let bool yes = true;\n",
        "    let bool no = false;\n",
        "    let u8 total = 0;\n",
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
        "fn main() -> u8 {\n",
        "    let u0 a = 0;\n",
        "    let i0 b = 0;\n",
        "    let u0 c = id(a);\n",
        "    let i0 d = mix(b);\n",
        "    let *u0 p = @c;\n",
        "    *p = a;\n",
        "    let i32 five = 5;\n",
        "    let i8 minus_three = -3;\n",
        "    let i32 x = 3;\n",
        "    let u8 total = 0;\n",
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
         fn main() { let u0 z = div(0, 0); }\n",
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
                 fn main() {{ let {ty} z = f(); }}\n",
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
             fn main() -> u8 {{ let {ty} z = f(); return 7; }}\n",
        ));
        assert_eq!(code, 7, "`~0` as `{ty}`");
    }
}

#[cfg(feature = "llvm-backend")]
#[test]
fn backend_runs_tuples_and_multiple_results() {
    // Each check adds a distinct bit when it holds.
    let code = compile_and_run(concat!(
        "fn divmod(i32 n, i32 d) -> i32 q, i32 r {\n",
        "    q = n / d;\n",
        "    r = n % d;\n",
        "    return;\n",
        "}\n",
        "fn pair() -> i32, bool { return 7, true; }\n",
        "fn nothing() {}\n",
        "#[entry]\n",
        "fn main() -> u8 {\n",
        "    let i32 q, i32 r = divmod(17, 5);\n",
        "    q, r = r, q;\n",
        "    let (i32, i32) p = (12, 13);\n",
        "    p.0, p.1 = p.1, p.0;\n",
        "    let i32 a, (bool b, _) = 1, (true, 5);\n",
        "    let _, bool ok = pair();\n",
        "    nothing();\n",
        "    let u8 total = 0;\n",
        "    total += 1 if (q, r) == (2, 3);\n",
        "    total += 2 if p == (13, 12);\n",
        "    total += 4 if ok and b;\n",
        "    total += 8 if (a, ()) != (2, ());\n",
        "    total += 16 if divmod(9, 4).1 == 1;\n",
        "    total += 32 if pair() == (7, true);\n",
        "    return total;\n",
        "}\n",
    ));
    assert_eq!(code, 63);
}

#[cfg(feature = "llvm-backend")]
#[test]
fn backend_evaluates_tuple_comparisons_in_order() {
    let code = compile_and_run(concat!(
        "fn log(*i32 trace, i32 v) -> i32 {\n",
        "    *trace = *trace * 10 + v;\n",
        "    return v;\n",
        "}\n",
        "#[entry]\n",
        "fn main() -> u8 {\n",
        "    let i32 trace = 0;\n",
        "    let *i32 p = @trace;\n",
        "    let u8 total = 0;\n",
        "    total += 1 unless (log(p, 1), log(p, 2)) == (log(p, 3), log(p, 4));\n",
        "    total += 2 if trace == 1234;\n",
        "    trace = 0;\n",
        "    total += 4 unless (log(p, 5), log(p, 6)) != (log(p, 5), log(p, 6));\n",
        "    total += 8 if trace == 5656;\n",
        "    return total;\n",
        "}\n",
    ));
    assert_eq!(code, 15);
}

#[test]
fn unassigned_named_result_is_a_flow_error() {
    let diagnostic = single_json_diagnostic(
        "check",
        concat!(
            "fn f(bool c) -> i32 q, i32 r { q = 1; if c { r = 2; } return; }\n",
            "#[entry] fn main() { let i32 a, i32 b = f(true); }\n",
        ),
    );
    let message = diagnostic["message"].as_str().expect("message");
    assert!(
        message.contains("named result `r` is not assigned on every path"),
        "{diagnostic}"
    );
}

#[cfg(feature = "llvm-backend")]
#[test]
fn backend_runs_exact_casts() {
    let code = compile_and_run(concat!(
        "#[entry]\n",
        "fn main() -> u8 {\n",
        "    return 1 unless to_u8(255) == 255;\n",
        "    return 2 unless widen(-1) == -1;\n",
        "    return 3 unless to_int(-7.0) == -7;\n",
        "    return 4 unless to_f32(16777216) == 16777216.0;\n",
        "    return 5 unless narrow(0.5) == 0.5;\n",
        "    let u64 x = 12;\n",
        "    let *u8 p = (@x as usize) as *u8;\n",
        "    return 6 unless *(p as *u64) == 12;\n",
        "    let i32 total = 300;\n",
        "    return (total - 100) as u8;\n",
        "}\n",
        "fn to_u8(i32 n) -> u8 { return n as u8; }\n",
        "fn widen(i8 n) -> i64 { return n as i64; }\n",
        "fn to_int(f64 x) -> i32 { return x as i32; }\n",
        "fn to_f32(i64 n) -> f32 { return n as f32; }\n",
        "fn narrow(f64 x) -> f32 { return x as f32; }\n",
    ));
    assert_eq!(code, 200);
}

/// A cast the target type cannot represent exactly traps at runtime, also
/// when its operand is unknown at compile time.
#[cfg(all(feature = "llvm-backend", unix))]
#[test]
fn lossy_casts_trap_at_runtime() {
    let helpers = concat!(
        "fn to_u8(i32 n) -> u8 { return n as u8; }\n",
        "fn to_int(f64 x) -> i32 { return x as i32; }\n",
        "fn to_uint(f64 x) -> u32 { return x as u32; }\n",
        "fn to_f32(i64 n) -> f32 { return n as f32; }\n",
        "fn to_f64(u64 n) -> f64 { return n as f64; }\n",
        "fn narrow(f64 x) -> f32 { return x as f32; }\n",
        "fn nan() -> f64 { let f64 zero = 0.0; return zero / zero; }\n",
        "fn half_to_i32(f16 x) -> i32 { return x as i32; }\n",
        "fn to_i256(f32 x) -> i256 { return x as i256; }\n",
        "fn to_f16(i32 n) -> f16 { return n as f16; }\n",
        "fn neg_inf() -> f16 { let f16 zero = 0.0; return -1.0 / zero; }\n",
        "fn neg_inf32() -> f32 { let f32 zero = 0.0; return -1.0 / zero; }\n",
    );
    for call in [
        "to_u8(-1)",
        "to_u8(256)",
        "to_int(2.5)",
        "to_int(nan())",
        "to_int(2147483648.0)",
        "to_int(-2147483649.0)",
        "to_uint(-1.0)",
        "to_f32(16777217)",
        "to_f64(18446744073709551615)",
        "narrow(0.1)",
        "narrow(1.0e300)",
        // The integer bounds are infinite in the float type.
        "half_to_i32(neg_inf())",
        "to_i256(neg_inf32())",
        // -65536 rounds to `-inf` in `f16`.
        "to_f16(-65536)",
    ] {
        let status = run_status(&format!(
            "#[entry] fn main() -> u8 {{ {call}; return 0; }}\n{helpers}"
        ));
        assert!(status.code().is_none(), "`{call}` should trap: {status:?}");
    }
    for call in [
        "to_int(-2147483648.0)",
        "to_uint(-0.0)",
        "to_uint(4294967295.0)",
        "narrow(nan())",
        "to_f16(-65504)",
    ] {
        let status = run_status(&format!(
            "#[entry] fn main() -> u8 {{ {call}; return 0; }}\n{helpers}"
        ));
        assert_eq!(status.code(), Some(0), "`{call}` should not trap");
    }
}

/// Every relocation model links and runs, including code LLVM lowers to
/// absolute addressing in non-PIC mode (the wide `fptosi` of `f32 as i256`
/// reads a constant-pool table), which cannot go into a PIE.
#[cfg(all(feature = "llvm-backend", target_os = "linux"))]
#[test]
fn every_relocation_model_links_and_runs() {
    let src = concat!(
        "#[entry] fn main() -> u8 { return (to_i256(three()) as u8) + 4; }\n",
        "fn to_i256(f32 x) -> i256 { return x as i256; }\n",
        "fn three() -> f32 { let f32 t = 3.0; return t; }\n",
    );
    for model in ["static", "pic", "pie", "dynamic-no-pic"] {
        for opt in ["0", "3"] {
            let status = run_status_with(src, &["--relocation-model", model, "-O", opt]);
            assert_eq!(status.code(), Some(7), "{model} at -O{opt}");
        }
    }
    assert_eq!(run_status(src).code(), Some(7), "default relocation model");
}

/// Casts between floats and integers wider than 128 bits compile and run on
/// every platform. LLVM 22 could not lower them on AArch64 (macOS) and
/// aborted; LLVM 23 expands them on every target. The values are exact, so
/// no cast traps, and they pass through parameters so `-O0` keeps every
/// conversion.
#[cfg(feature = "llvm-backend")]
#[test]
fn wide_float_int_casts_compile_and_run() {
    let src = concat!(
        "#[entry]\n",
        "fn main() -> u8\n",
        "{\n",
        "    let f64 big = pow2(200);\n",
        "    let i256 one = 1;\n",
        "    let i256 n = f64_to_i256(-big);\n",
        "    return 1 unless n == -(one << 200);\n",
        "    return 2 unless i256_to_f64(n) == -big;\n",
        "    let u256 unsigned_one = 1;\n",
        "    return 3 unless f64_to_u256(big) == unsigned_one << 200;\n",
        "    return 4 unless u256_to_f64(f64_to_u256(big)) == big;\n",
        "    return 5 unless f32_to_i256(-3.0) == -3;\n",
        "    return 6 unless i256_to_f32(-3) == -3.0;\n",
        "    let i129 small_one = 1;\n",
        "    return 7 unless f64_to_i129(pow2(127)) == small_one << 127;\n",
        "    return 8 unless i129_to_f64(small_one << 127) == pow2(127);\n",
        "    return 0;\n",
        "}\n",
        "fn pow2(i32 k) -> f64\n",
        "{\n",
        "    let f64 r = 1.0;\n",
        "    let i32 i = 0;\n",
        "    while i < k { r *= 2.0; i++; }\n",
        "    return r;\n",
        "}\n",
        "fn f64_to_i256(f64 x) -> i256 { return x as i256; }\n",
        "fn i256_to_f64(i256 n) -> f64 { return n as f64; }\n",
        "fn f64_to_u256(f64 x) -> u256 { return x as u256; }\n",
        "fn u256_to_f64(u256 n) -> f64 { return n as f64; }\n",
        "fn f32_to_i256(f32 x) -> i256 { return x as i256; }\n",
        "fn i256_to_f32(i256 n) -> f32 { return n as f32; }\n",
        "fn f64_to_i129(f64 x) -> i129 { return x as i129; }\n",
        "fn i129_to_f64(i129 n) -> f64 { return n as f64; }\n",
    );
    for opt in ["0", "3"] {
        let status = run_status_with(src, &["-O", opt]);
        assert_eq!(status.code(), Some(0), "-O{opt}: failed check (exit code)");
    }
}

#[cfg(feature = "llvm-backend")]
#[test]
fn backend_runs_structs() {
    // Each check adds a distinct bit when it holds.
    let code = compile_and_run(concat!(
        "struct Point { i32 x; i32 y; }\n",
        "struct Line { Point from; (Point, i32) to; }\n",
        "struct Node { i32 value; *Node next; }\n",
        "struct Empty {}\n",
        "fn flip(Point p) -> Point { return Point(y = p.x, x = p.y); }\n",
        "fn bump(&Point p) { p.x += 10; }\n",
        "fn log(*i32 trace, i32 v) -> i32 {\n",
        "    *trace = *trace * 10 + v;\n",
        "    return v;\n",
        "}\n",
        "#[entry]\n",
        "fn main() -> u8 {\n",
        "    let u8 total = 0;\n",
        "    // Fields may be given in any order.\n",
        "    let Point p = Point(y = 2, x = 1);\n",
        "    total += 1 if p.x == 1 and p.y == 2;\n",
        "    // A struct is copied when passed and returned.\n",
        "    let Point q = flip(p);\n",
        "    total += 2 if q == Point(x = 2, y = 1) and p != q and p == Point(x = 1, y = 2);\n",
        "    // Fields are places, also nested and through a reference.\n",
        "    let Line l = Line(from = p, to = (q, 7));\n",
        "    l.from.x = 5;\n",
        "    l.to.0.y += 4;\n",
        "    l.to.1++;\n",
        "    bump(@l.from);\n",
        "    total += 4 if l.from.x == 15 and l.to.0.y == 5 and l.to.1 == 8 and p.x == 1;\n",
        "    // A struct may point to its own type.\n",
        "    let Node last = Node(value = 30, next = @0x0);\n",
        "    let Node first = Node(value = 12, next = @last);\n",
        "    total += 8 if (*first.next).value + first.value == 42;\n",
        "    // Values are computed in the order they are written.\n",
        "    let i32 trace = 0;\n",
        "    let Point t = Point(y = log(@trace, 1), x = log(@trace, 2));\n",
        "    total += 16 if trace == 12 and t.x == 2 and t.y == 1;\n",
        "    // Fields swap in parallel, and empty structs are all equal.\n",
        "    t.x, t.y = t.y, t.x;\n",
        "    let Empty e = Empty();\n",
        "    total += 32 if t == Point(x = 1, y = 2) and e == Empty();\n",
        "    return total;\n",
        "}\n",
    ));
    assert_eq!(code, 63);
}

#[cfg(feature = "llvm-backend")]
#[test]
fn backend_runs_methods() {
    // Each check adds a distinct bit when it holds.
    let code = compile_and_run(concat!(
        "struct Counter { i32 n; }\n",
        "impl Counter {\n",
        "    fn zero() -> Counter { return Counter(n = 0); }\n",
        "    fn bump(&self) { self.n += 1; }\n",
        "    fn add(&self, i32 by) { self.n += by; }\n",
        "    fn bumped(self) -> Counter { self.n += 1; return self; }\n",
        "    fn get(self) -> i32 { return self.n; }\n",
        "    fn twice(&self) { self.bump(); self.bump(); }\n",
        "}\n",
        "impl Counter {\n",
        "    fn area(self) -> i32 { return self.n * self.n; }\n",
        "}\n",
        "fn area(i32 n) -> i32 { return n + 1; }\n",
        "fn bump_through(&Counter c) { c.bump(); }\n",
        "#[entry]\n",
        "fn main() -> u8 {\n",
        "    let u8 total = 0;\n",
        "    // A `&self` method changes the value it is called on.\n",
        "    let Counter c = Counter.zero();\n",
        "    c.bump();\n",
        "    c.twice();\n",
        "    total += 1 if c.get() == 3;\n",
        "    // A `self` method works on a copy.\n",
        "    let Counter d = c.bumped();\n",
        "    total += 2 if c.n == 3 and d.n == 4;\n",
        "    // The argument is read before the method changes the receiver.\n",
        "    c.add(c.n);\n",
        "    total += 4 if c.n == 6;\n",
        "    // Through a reference and a pointer.\n",
        "    bump_through(@c);\n",
        "    let *Counter p = @c;\n",
        "    (*p).bump();\n",
        "    total += 8 if c.n == 8;\n",
        "    // On a value that is no variable: the change is to a temporary.\n",
        "    Counter.zero().bump();\n",
        "    total += 16 if Counter.zero().bumped().bumped().get() == 2;\n",
        "    // A method and a function may share a name.\n",
        "    total += 32 if c.area() == 64 and area(8) == 9;\n",
        "    return total;\n",
        "}\n",
    ));
    assert_eq!(code, 63);
}

#[cfg(feature = "llvm-backend")]
#[test]
fn backend_runs_properties() {
    // Each check adds a distinct bit when it holds.
    let code = compile_and_run(concat!(
        "struct Rect { i32 width; i32 height; }\n",
        "impl Rect {\n",
        "    i32 area { return self.width * self.height; }\n",
        "    i32 perimeter { return 2 * (self.width + self.height); }\n",
        "    bool square { return self.width == self.height; }\n",
        "    Rect flipped { return Rect(width = self.height, height = self.width); }\n",
        "    i32 double_area { return self.area * 2; }\n",
        "    fn scaled(self, i32 k) -> Rect {\n",
        "        return Rect(width = self.width * k, height = self.height * k);\n",
        "    }\n",
        "    fn scale(&self, i32 k) { self.width *= k; self.height *= k; }\n",
        "    fn unit() -> Rect { return Rect(width = 1, height = 1); }\n",
        "}\n",
        "fn area_of(&Rect r) -> i32 { return r.area; }\n",
        "#[entry]\n",
        "fn main() -> u8 {\n",
        "    let u8 total = 0;\n",
        "    let Rect r = Rect(width = 3, height = 4);\n",
        "    total += 1 if r.area == 12 and r.perimeter == 14 and not r.square;\n",
        "    // A property follows the fields it reads.\n",
        "    r.width = 5;\n",
        "    total += 2 if r.area == 20 and r.double_area == 40;\n",
        "    // On a value that is no variable, and chained.\n",
        "    total += 4 if Rect.unit().area == 1 and Rect.unit().scaled(3).area == 9;\n",
        "    total += 8 if r.flipped.width == 4 and r.flipped.flipped == r;\n",
        "    // The argument is read before the method changes the receiver.\n",
        "    r.scale(r.area);\n",
        "    total += 16 if r.width == 100 and r.height == 80;\n",
        "    // Through a reference and a pointer.\n",
        "    let *Rect p = @r;\n",
        "    total += 32 if area_of(@r) == 8000 and (*p).area == 8000 and Rect.unit().square;\n",
        "    return total;\n",
        "}\n",
    ));
    assert_eq!(code, 63);
}

#[cfg(feature = "llvm-backend")]
#[test]
fn backend_runs_deep_trees_of_empty_structs() {
    // `S40` has no data, but its fields written out are a tree of 2^40
    // leaves. Building, passing, comparing, and reading it must not cost
    // one step per leaf in the compiler. Each `make` calls the one below
    // once, so that the program itself runs 40 calls, not 2^40.
    let mut source = String::from("struct S0 {}\nimpl S0 { fn make() -> S0 { return S0(); } }\n");
    for level in 1..=40 {
        let inner = level - 1;
        source.push_str(&format!(
            "struct S{level} {{ S{inner} a; S{inner} b; }}\n\
             impl S{level} {{ fn make() -> S{level} {{ \
                 let S{inner} half = S{inner}.make(); \
                 return S{level}(a = half, b = half); }} }}\n"
        ));
    }
    source.push_str(concat!(
        "struct Tagged { S40 nothing; i32 tag; }\n",
        "fn same(S40 a, S40 b) -> bool { return a == b; }\n",
        "#[entry]\n",
        "fn main() -> u8 {\n",
        "    let S40 x = S40.make();\n",
        "    return 1 unless same(x, x);\n",
        "    let S39 inner = x.b;\n",
        "    x.a = inner;\n",
        "    return 2 unless x.a.b == inner.b;\n",
        "    let Tagged t = Tagged(nothing = x, tag = 7);\n",
        "    t.nothing.a.a = x.b.b;\n",
        "    return 3 unless t.tag == 7 and t == Tagged(nothing = S40.make(), tag = 7);\n",
        "    return 0;\n",
        "}\n",
    ));
    assert_eq!(compile_and_run(&source), 0);
}

#[cfg(feature = "llvm-backend")]
#[test]
fn backend_runs_shared_type_declarations() {
    let code = compile_and_run(concat!(
        "fn pair() -> i32, i32 { return 3, 4; }\n",
        "#[entry]\n",
        "fn main() -> u8 {\n",
        "    let u8 total = 0;\n",
        "    let i32 a, b = 1, 2;\n",
        "    total += 1 if a + b == 3;\n",
        "    let i32 x, y = pair();\n",
        "    total += 2 if x == 3 and y == 4;\n",
        "    let u8 small, (i64 big, wide), _, last = 1, (2, 3), 9, 4;\n",
        "    total += 4 if small + last == 5 and big + wide == 5;\n",
        "    let [2]u8 row, copy = [1, 2], [3, 4];\n",
        "    row[0] = 9;\n",
        "    total += 8 if row == [9, 2] and copy == [3, 4];\n",
        "    return total;\n",
        "}\n",
    ));
    assert_eq!(code, 15);
}
