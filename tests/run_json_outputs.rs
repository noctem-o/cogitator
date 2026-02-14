use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::Command;

#[test]
fn run_writes_report_json_with_floats_and_keeps_meta_canonical() {
    let temp = tempfile::tempdir().expect("tempdir");
    let out_dir = temp.path().join("out");
    let bin = env!("CARGO_BIN_EXE_cogitator");

    let output = Command::new(bin)
        .current_dir(temp.path())
        .arg("run")
        .arg("--runs")
        .arg("1")
        .arg("--seed")
        .arg("42")
        .arg("--out-dir")
        .arg(out_dir.as_os_str())
        .arg("--clean")
        .arg("--no-tui")
        .arg("--nix-provenance")
        .arg("off")
        .output()
        .expect("run cogitator run");

    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert_exists(&out_dir.join("meta.json"));
    assert_exists(&out_dir.join("trace.jsonl"));
    assert_exists(&out_dir.join("witness_root.txt"));
    assert_exists(&out_dir.join("results.json"));
    assert_exists(&out_dir.join("summary.json"));
    assert_exists(&out_dir.join("analysis.json"));

    let results: Value =
        serde_json::from_slice(&fs::read(out_dir.join("results.json")).expect("read results.json"))
            .expect("parse results.json");
    let first = results
        .as_array()
        .and_then(|rows| rows.first())
        .expect("results has at least one row");
    assert!(
        first["difficulty"].is_number() && first["difficulty"].as_i64().is_none(),
        "difficulty should be encoded as a non-integer JSON number"
    );
    assert!(
        first["score"].is_number() && first["score"].as_i64().is_none(),
        "score should be encoded as a non-integer JSON number"
    );

    let summary: Value =
        serde_json::from_slice(&fs::read(out_dir.join("summary.json")).expect("read summary.json"))
            .expect("parse summary.json");
    assert!(summary["pass_rate"].is_number() && summary["pass_rate"].as_i64().is_none());
    assert!(summary["avg_score"].is_number() && summary["avg_score"].as_i64().is_none());

    let analysis: Value = serde_json::from_slice(
        &fs::read(out_dir.join("analysis.json")).expect("read analysis.json"),
    )
    .expect("parse analysis.json");
    assert!(
        analysis["summary"]["pass_rate"].is_number()
            && analysis["summary"]["pass_rate"].as_i64().is_none()
    );

    let meta_bytes = fs::read(out_dir.join("meta.json")).expect("read meta.json");
    let meta_json: Value = serde_json::from_slice(&meta_bytes).expect("parse meta.json");
    let canonical_meta = cogitator::canonical_json::to_vec(&meta_json).expect("canonical meta");
    assert_eq!(meta_bytes, [canonical_meta, b"\n".to_vec()].concat());
}

fn assert_exists(path: &Path) {
    assert!(path.exists(), "missing {}", path.display());
}
