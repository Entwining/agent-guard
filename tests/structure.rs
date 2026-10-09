use std::{fs, path::Path, process::Command};

fn root() -> tempfile::TempDir {
    let root = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    for directory in ["src", "tests", "cmd", "examples"] {
        fs::create_dir(root.path().join(directory)).unwrap();
    }
    root
}

fn check(root: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_agent-guard-structure"))
        .arg(root)
        .output()
        .unwrap()
}

#[test]
fn file_cap_counts_code_and_block_comments_but_not_blank_or_line_comments() {
    let root = root();
    let file = root.path().join("src/boundary.rs");
    let source = "const ROW: u8 = 0;\n\n  // comment\n\t\n".repeat(500);
    fs::write(&file, &source).unwrap();
    let output = check(root.path());
    assert!(output.status.success());
    assert!(output.stdout.is_empty() && output.stderr.is_empty());

    fs::write(&file, format!("{source}/* block comment */\n")).unwrap();
    let output = check(root.path());
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "src/boundary.rs: 501 effective lines exceeds 500\n"
    );
}

#[test]
fn file_cap_reports_every_oversized_source_in_all_four_roots() {
    let root = root();
    for (directory, count) in [
        ("src", 501),
        ("tests", 502),
        ("cmd", 503),
        ("examples", 504),
    ] {
        let nested = root.path().join(directory).join("nested");
        fs::create_dir(&nested).unwrap();
        fs::write(
            nested.join("large.rs"),
            "const ROW: u8 = 0;\n".repeat(count),
        )
        .unwrap();
        fs::write(nested.join("ordinary.txt"), "data\n".repeat(600)).unwrap();
    }
    let output = check(root.path());
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        concat!(
            "cmd/nested/large.rs: 503 effective lines exceeds 500\n",
            "examples/nested/large.rs: 504 effective lines exceeds 500\n",
            "src/nested/large.rs: 501 effective lines exceeds 500\n",
            "tests/nested/large.rs: 502 effective lines exceeds 500\n",
        )
    );
}

#[test]
fn file_cap_requires_every_source_root_to_be_readable() {
    let root = root();
    fs::remove_dir(root.path().join("examples")).unwrap();
    let output = check(root.path());
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("examples")
    );
}
