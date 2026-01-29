use anyhow::{Context, Result};
use clap::ValueEnum;
use serde_json::Value;
use std::path::Path;
use std::process::Command;

use crate::model::NixProvenance;

const MAX_OUTPUT_BYTES: usize = 128 * 1024;

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
    let output = Command::new(command).args(args).output().ok()?;
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
    let mut cmd = Command::new(command);
    cmd.args(args);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    let output = cmd.output().ok()?;
    if !output.status.success() || output.stdout.len() > MAX_OUTPUT_BYTES {
        return None;
    }
    serde_json::from_slice(&output.stdout)
        .ok()
        .map(canonicalize_json)
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
