use anyhow::{Context, Result};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

pub fn to_vec<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    let value = serde_json::to_value(value).context("serialize to json value")?;
    ensure_no_floats(&value)?;
    let canonical = canonicalize_value(value);
    let mut buffer = Vec::new();
    let formatter = serde_json::ser::CompactFormatter;
    let mut serializer = serde_json::Serializer::with_formatter(&mut buffer, formatter);
    canonical
        .serialize(&mut serializer)
        .context("serialize canonical json")?;
    Ok(buffer)
}

pub fn to_value<T: Serialize>(value: &T) -> Result<Value> {
    let bytes = to_vec(value)?;
    let value: Value =
        serde_json::from_slice(&bytes).context("deserialize canonical json value")?;
    Ok(value)
}

pub fn write_json<T: Serialize>(path: &Path, value: &T, label: &str) -> Result<()> {
    let bytes = to_vec(value)?;
    crate::io_utils::write_atomic(path, label, |file| {
        file.write_all(&bytes)
            .with_context(|| format!("failed to write {}", label))?;
        file.write_all(b"\n")
            .with_context(|| format!("failed to write newline for {}", label))?;
        Ok(())
    })
}

fn canonicalize_value(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut ordered = BTreeMap::new();
            for (key, value) in map {
                ordered.insert(key, canonicalize_value(value));
            }
            let mut output = serde_json::Map::with_capacity(ordered.len());
            for (key, value) in ordered {
                output.insert(key, value);
            }
            Value::Object(output)
        }
        Value::Array(values) => Value::Array(values.into_iter().map(canonicalize_value).collect()),
        other => other,
    }
}

fn ensure_no_floats(value: &Value) -> Result<()> {
    if contains_float(value) {
        anyhow::bail!(
            "canonicalization rejected floating-point number in witnessed material; use integers or canonical strings"
        );
    }
    Ok(())
}

fn contains_float(value: &Value) -> bool {
    match value {
        Value::Number(num) => num.is_f64(),
        Value::Array(values) => values.iter().any(contains_float),
        Value::Object(map) => map.values().any(contains_float),
        _ => false,
    }
}
