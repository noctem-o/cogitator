//! The plain bundle check (`verify --witness <dir>` without
//! `--recompute-witness-root`) must not silently ignore `--expect`. A supplied
//! anchor that is never compared would give a false sense of verification, so
//! the CLI fails closed and tells the caller to add `--recompute-witness-root`.

use std::process::Command;
use tempfile::TempDir;

fn make_bundle(dir: &std::path::Path) -> std::path::PathBuf {
    let bin = env!("CARGO_BIN_EXE_cogitator");
    let output = Command::new(bin)
        .args([
            "run",
            "--agent",
            "ordeal",
            "--runs",
            "1",
            "--out-dir",
            dir.to_str().unwrap(),
            "--clean",
            "--no-tui",
        ])
        .output()
        .expect("run command");
    assert!(
        output.status.success(),
        "cogitator run failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    dir.join("run_0000")
}

#[test]
fn plain_bundle_verify_rejects_expect_without_recompute() {
    let temp = TempDir::new().unwrap();
    let bundle = make_bundle(temp.path());
    let bin = env!("CARGO_BIN_EXE_cogitator");

    let output = Command::new(bin)
        .args([
            "verify",
            "--witness",
            bundle.to_str().unwrap(),
            "--expect",
            "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
        ])
        .output()
        .expect("verify command");

    assert!(
        !output.status.success(),
        "verify with a bogus --expect and no --recompute-witness-root must fail closed"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--recompute-witness-root"),
        "error should point the caller at --recompute-witness-root; got: {stderr}"
    );
}

#[test]
fn anchored_recompute_with_correct_root_succeeds() {
    let temp = TempDir::new().unwrap();
    let bundle = make_bundle(temp.path());
    let bin = env!("CARGO_BIN_EXE_cogitator");

    let expected = std::fs::read_to_string(bundle.join("witness_root.txt"))
        .expect("read witness root")
        .trim()
        .to_string();

    let output = Command::new(bin)
        .args([
            "verify",
            "--witness",
            bundle.to_str().unwrap(),
            "--recompute-witness-root",
            "--expect",
            &expected,
        ])
        .output()
        .expect("verify command");

    assert!(
        output.status.success(),
        "anchored recompute against the correct root must pass; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
