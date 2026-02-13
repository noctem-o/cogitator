use std::fs;
use std::process::Command;

use tempfile::tempdir;

fn run_and_read_root(extra_flags: &[&str]) -> String {
    let temp = tempdir().expect("tempdir");
    let out_dir = temp.path().join("out");

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_cogitator"));
    cmd.args([
        "run",
        "--agent",
        "ordeal",
        "--seed",
        "123",
        "--runs",
        "1",
        "--threads",
        "1",
        "--out-dir",
        out_dir.to_str().expect("utf8 path"),
        "--clean",
        "--no-tui",
    ]);
    cmd.args(extra_flags);

    let status = cmd.status().expect("invoke cogitator");
    assert!(status.success(), "run command failed");

    fs::read_to_string(out_dir.join("run_0000").join("witness_root.txt"))
        .expect("read witness root")
        .trim()
        .to_string()
}

#[test]
fn witness_root_invariant_under_theme_and_no_color_flags() {
    let baseline = run_and_read_root(&["--theme", "neon"]);
    let cyan = run_and_read_root(&["--theme", "cyan"]);
    let mono_no_color = run_and_read_root(&["--theme", "mono", "--no-color"]);

    assert_eq!(baseline, cyan, "theme must not affect witness root");
    assert_eq!(
        baseline, mono_no_color,
        "no-color and mono theme must not affect witness root"
    );
}
