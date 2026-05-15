use std::fs;
use std::path::Path;

use cogitator::{canonical_json, verify};
use serde_json::Value;
use tempfile::TempDir;

fn fixture(path: &str) -> String {
    fs::read_to_string(Path::new("tests/fixtures").join(path)).expect("read fixture")
}

#[test]
fn canonical_json_orders_keys_deterministically() {
    let input = fixture("canonical_json/valid_sorted_object.json");
    let value: Value = serde_json::from_str(&input).expect("parse json");
    let canonical = canonical_json::to_vec(&value).expect("canonicalize");
    assert_eq!(
        String::from_utf8(canonical).unwrap(),
        r#"{"a":1,"b":2,"nested":{"m":1,"z":0}}"#
    );
}

#[test]
fn canonical_json_rejects_non_integer_numbers() {
    let input = fixture("canonical_json/reject_float.json");
    let value: Value = serde_json::from_str(&input).expect("parse json");
    let err = canonical_json::to_vec(&value).expect_err("float must be rejected");
    assert!(err.to_string().contains("non-integer"));
}

#[test]
fn duplicate_keys_are_parser_dependent_and_currently_last_wins() {
    let input = fixture("canonical_json/duplicate_keys.json");
    let parsed: Value = serde_json::from_str(&input).expect("parse duplicate key json");
    let canonical = canonical_json::to_vec(&parsed).expect("canonicalize");
    assert_eq!(String::from_utf8(canonical).unwrap(), r#"{"dup":2}"#);
}

#[test]
fn verify_recompute_ignores_report_only_file_mutation() {
    let temp = TempDir::new().unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_cogitator"))
        .args([
            "run",
            "--agent",
            "ordeal",
            "--runs",
            "1",
            "--out-dir",
            temp.path().to_str().unwrap(),
            "--clean",
        ])
        .output()
        .expect("run command");
    assert!(
        output.status.success(),
        "cogitator run failed with status {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let bundle = temp.path().join("run_0000");
    assert!(
        bundle.exists(),
        "expected bundle directory to exist at {}",
        bundle.display()
    );
    let report_path = bundle.join("verify_report.json");
    fs::write(&report_path, "{\"tampered\":true}\n").unwrap();

    let receipt = verify::recompute_agent_witness_root_from_bundle(&bundle, None)
        .expect("recompute should ignore report-only artifact");
    assert!(receipt.matched);
}
