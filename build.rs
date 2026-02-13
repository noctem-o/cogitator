fn main() {
    println!("cargo:rerun-if-env-changed=COGITATOR_GIT_SHA");

    if std::env::var("COGITATOR_GIT_SHA").is_ok() {
        return;
    }

    let sha = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|out| {
            if out.status.success() {
                Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
            } else {
                None
            }
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string());

    println!("cargo:rustc-env=COGITATOR_GIT_SHA={sha}");
}
