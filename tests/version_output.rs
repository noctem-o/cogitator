use std::process::Command;

fn is_hex(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_hexdigit())
}

#[test]
fn version_output_has_semver_and_sha_or_unknown() {
    let exe = env!("CARGO_BIN_EXE_cogitator");
    let output = Command::new(exe)
        .arg("--version")
        .output()
        .expect("run cogitator --version");

    assert!(output.status.success(), "--version must succeed");
    let stdout = String::from_utf8(output.stdout).expect("utf8 --version output");
    let line = stdout.trim();

    let Some(rest) = line.strip_prefix("cogitator ") else {
        panic!("unexpected --version prefix: {line}");
    };
    let Some((semver, rev_part)) = rest.split_once(" (") else {
        panic!("missing revision tuple in --version output: {line}");
    };

    let semver_parts: Vec<_> = semver.split('.').collect();
    assert_eq!(semver_parts.len(), 3, "semver must have 3 components");
    assert!(
        semver_parts
            .iter()
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit())),
        "semver components must be decimal: {semver}"
    );

    let Some(rev) = rev_part.strip_suffix(')') else {
        panic!("revision tuple must end with ')': {line}");
    };
    assert!(!rev.is_empty(), "revision token must never be empty");
    assert!(
        rev == "unknown" || (is_hex(rev) && (7..=40).contains(&rev.len())),
        "revision token must be unknown or short/full hex sha: {rev}"
    );
}
