use anyhow::{Context, Result};
use clap::ValueEnum;
use serde_json::Value;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::model::NixProvenance;

const MAX_OUTPUT_BYTES: usize = 128 * 1024;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(ValueEnum, Clone, Debug)]
pub enum NixProvenanceMode {
    Auto,
    On,
    Off,
}

pub fn collect_nix_provenance(
    mode: NixProvenanceMode,
    repo_root: &Path,
) -> Result<Option<NixProvenance>> {
    let nix_version = nix_version();
    let mut nix_provenance_resolved = None;
    match mode {
        NixProvenanceMode::Off => return Ok(None),
        NixProvenanceMode::Auto => {
            let nix_store = std::env::var("NIX_STORE").ok();
            if nix_store.is_none() && nix_version.is_none() {
                if cfg!(target_os = "windows") {
                    nix_provenance_resolved = Some("off (windows)".to_string());
                } else {
                    return Ok(None);
                }
            }
        }
        NixProvenanceMode::On => {
            if nix_version.is_none() {
                anyhow::bail!("--nix-provenance=on requires the nix CLI to be available");
            }
        }
    }

    if nix_provenance_resolved.is_some()
        && nix_version.is_none()
        && std::env::var("NIX_STORE").ok().is_none()
    {
        let provenance = NixProvenance {
            nix_provenance_resolved,
            nix_version: None,
            nixos_version: None,
            flake_metadata: None,
            current_system: None,
        };
        return Ok(Some(provenance));
    }

    let nixos_version = command_output("nixos-version", &[]);
    let flake_metadata = flake_metadata(repo_root);
    let current_system = current_system_info();

    let provenance = NixProvenance {
        nix_provenance_resolved,
        nix_version,
        nixos_version,
        flake_metadata,
        current_system,
    };

    if provenance.nix_version.is_none()
        && provenance.nixos_version.is_none()
        && provenance.flake_metadata.is_none()
        && provenance.current_system.is_none()
        && provenance.nix_provenance_resolved.is_none()
    {
        Ok(None)
    } else {
        Ok(Some(provenance))
    }
}

fn nix_version() -> Option<String> {
    command_output("nix", &["--version"])
}

fn flake_metadata(repo_root: &Path) -> Option<Value> {
    let lock_path = repo_root.join("flake.lock");
    if !lock_path.exists() {
        return None;
    }
    command_json("nix", &["flake", "metadata", "--json"], Some(repo_root))
}

fn current_system_info() -> Option<Value> {
    let path = Path::new("/run/current-system");
    if !path.exists() {
        return None;
    }
    command_json("nix", &["path-info", "--json", "/run/current-system"], None)
}

fn command_output(command: &str, args: &[&str]) -> Option<String> {
    let output = run_bounded_command(command, args, None).ok()?;
    if !output.status.success() {
        return None;
    }
    let mut text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.len() > MAX_OUTPUT_BYTES {
        text.truncate(MAX_OUTPUT_BYTES);
    }
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn command_json(command: &str, args: &[&str], cwd: Option<&Path>) -> Option<Value> {
    let output = run_bounded_command(command, args, cwd).ok()?;
    if !output.status.success() || output.stdout.len() > MAX_OUTPUT_BYTES {
        return None;
    }
    serde_json::from_slice(&output.stdout)
        .ok()
        .map(canonicalize_json)
}

fn run_bounded_command(
    command: &str,
    args: &[&str],
    cwd: Option<&Path>,
) -> std::io::Result<std::process::Output> {
    let mut cmd = Command::new(command);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }

    for key in [
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "NO_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
        "no_proxy",
        "AWS_SECRET_ACCESS_KEY",
        "AWS_SESSION_TOKEN",
        "GITHUB_TOKEN",
        "NIX_CONFIG",
    ] {
        cmd.env_remove(key);
    }

    let mut child = cmd.spawn()?;
    let deadline = Instant::now() + COMMAND_TIMEOUT;
    loop {
        if child.try_wait()?.is_some() {
            let mut output = child.wait_with_output()?;
            truncate_output(&mut output.stdout);
            truncate_output(&mut output.stderr);
            return Ok(output);
        }

        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "nix provenance command timed out",
            ));
        }

        thread::sleep(Duration::from_millis(25));
    }
}

fn truncate_output(bytes: &mut Vec<u8>) {
    if bytes.len() > MAX_OUTPUT_BYTES {
        bytes.truncate(MAX_OUTPUT_BYTES);
    }
}

fn canonicalize_json(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut entries: Vec<_> = map.into_iter().collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            let mut out = serde_json::Map::with_capacity(entries.len());
            for (key, value) in entries {
                out.insert(key, canonicalize_json(value));
            }
            Value::Object(out)
        }
        Value::Array(values) => Value::Array(values.into_iter().map(canonicalize_json).collect()),
        other => other,
    }
}

pub fn write_nix_provenance(path: &Path, provenance: &NixProvenance) -> Result<()> {
    crate::canonical_json::write_json(path, provenance, "nix_provenance.json")
        .with_context(|| "failed to write nix_provenance.json")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::run_bounded_command;

    #[test]
    #[cfg(unix)]
    fn bounded_command_times_out() {
        let err = run_bounded_command("sh", &["-c", "sleep 10"], None).expect_err("must timeout");
        assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);
    }
}
