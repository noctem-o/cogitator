use anyhow::{Context, Result};
use serde::Serialize;
use serde_json::Value;
use std::io::Write;
use std::path::Path;

/// Canonical JSON bytes for witnessed material.
///
/// Contract:
/// - Object keys are sorted lexicographically by UTF-16 code units (RFC 8785 / JCS).
/// - Arrays preserve input order.
/// - Output is compact JSON with no insignificant whitespace.
/// - Numbers are restricted to integers (i64/u64) to enforce deterministic I-JSON-safe subset.
///
/// This is a strict, deterministic subset of RFC 8785 chosen for semantic commitments.
pub fn to_vec<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    let value = serde_json::to_value(value).context("serialize to json value")?;
    ensure_i_json_subset(&value)?;
    let mut out = Vec::new();
    write_canonical_value(&mut out, &value)?;
    Ok(out)
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

fn ensure_i_json_subset(value: &Value) -> Result<()> {
    match value {
        Value::Null | Value::Bool(_) | Value::String(_) => Ok(()),
        Value::Number(number) => {
            if number.is_i64() || number.is_u64() {
                Ok(())
            } else {
                anyhow::bail!(
                    "canonicalization rejected non-integer number in witnessed material; use integers or canonical strings"
                )
            }
        }
        Value::Array(values) => {
            for v in values {
                ensure_i_json_subset(v)?;
            }
            Ok(())
        }
        Value::Object(map) => {
            for (k, v) in map {
                if k.chars().any(|ch| (ch as u32) <= 0x1F) {
                    anyhow::bail!("canonicalization rejected control character in object key");
                }
                ensure_i_json_subset(v)?;
            }
            Ok(())
        }
    }
}

fn write_canonical_value(out: &mut Vec<u8>, value: &Value) -> Result<()> {
    match value {
        Value::Null => out.extend_from_slice(b"null"),
        Value::Bool(true) => out.extend_from_slice(b"true"),
        Value::Bool(false) => out.extend_from_slice(b"false"),
        Value::Number(num) => out.extend_from_slice(num.to_string().as_bytes()),
        Value::String(s) => write_json_string(out, s)?,
        Value::Array(values) => {
            out.push(b'[');
            for (idx, item) in values.iter().enumerate() {
                if idx > 0 {
                    out.push(b',');
                }
                write_canonical_value(out, item)?;
            }
            out.push(b']');
        }
        Value::Object(map) => {
            let mut entries: Vec<(&str, &Value)> =
                map.iter().map(|(k, v)| (k.as_str(), v)).collect();
            entries.sort_by(|(ka, _), (kb, _)| utf16_cmp(ka, kb));

            out.push(b'{');
            for (idx, (key, item)) in entries.iter().enumerate() {
                if idx > 0 {
                    out.push(b',');
                }
                write_json_string(out, key)?;
                out.push(b':');
                write_canonical_value(out, item)?;
            }
            out.push(b'}');
        }
    }
    Ok(())
}

fn utf16_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let mut ia = a.encode_utf16();
    let mut ib = b.encode_utf16();
    loop {
        match (ia.next(), ib.next()) {
            (Some(x), Some(y)) if x == y => continue,
            (Some(x), Some(y)) => return x.cmp(&y),
            (None, Some(_)) => return std::cmp::Ordering::Less,
            (Some(_), None) => return std::cmp::Ordering::Greater,
            (None, None) => return std::cmp::Ordering::Equal,
        }
    }
}

fn write_json_string(out: &mut Vec<u8>, s: &str) -> Result<()> {
    serde_json::to_writer(out, s).context("serialize json string")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorts_object_keys_by_utf16() {
        let value = serde_json::json!({"\u{E000}": 1, "\u{1F600}": 2});
        let bytes = to_vec(&value).expect("canonical bytes");
        assert_eq!(String::from_utf8(bytes).unwrap(), r#"{"😀":2,"":1}"#);
    }
}
