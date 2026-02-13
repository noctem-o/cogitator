use anyhow::{Context, Result};
use serde::de::{DeserializeOwned, MapAccess, SeqAccess, Visitor};
use serde::Deserialize;
use serde_json::{Number, Value};
use std::collections::HashSet;
use std::fmt;
use std::path::Path;

#[derive(Debug)]
enum StrictJsonValue {
    Null,
    Bool(bool),
    Number(Number),
    String(String),
    Array(Vec<StrictJsonValue>),
    Object(StrictJsonMap),
}

impl<'de> Deserialize<'de> for StrictJsonValue {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct StrictValueVisitor;

        impl<'de> Visitor<'de> for StrictValueVisitor {
            type Value = StrictJsonValue;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("valid strict JSON value")
            }

            fn visit_bool<E>(self, v: bool) -> std::result::Result<Self::Value, E> {
                Ok(StrictJsonValue::Bool(v))
            }

            fn visit_i64<E>(self, v: i64) -> std::result::Result<Self::Value, E> {
                Ok(StrictJsonValue::Number(Number::from(v)))
            }

            fn visit_u64<E>(self, v: u64) -> std::result::Result<Self::Value, E> {
                Ok(StrictJsonValue::Number(Number::from(v)))
            }

            fn visit_f64<E>(self, v: f64) -> std::result::Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Number::from_f64(v)
                    .map(StrictJsonValue::Number)
                    .ok_or_else(|| E::custom("invalid f64 number"))
            }

            fn visit_str<E>(self, v: &str) -> std::result::Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(StrictJsonValue::String(v.to_string()))
            }

            fn visit_string<E>(self, v: String) -> std::result::Result<Self::Value, E> {
                Ok(StrictJsonValue::String(v))
            }

            fn visit_none<E>(self) -> std::result::Result<Self::Value, E> {
                Ok(StrictJsonValue::Null)
            }

            fn visit_unit<E>(self) -> std::result::Result<Self::Value, E> {
                Ok(StrictJsonValue::Null)
            }

            fn visit_seq<A>(self, mut access: A) -> std::result::Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let mut out = Vec::new();
                while let Some(v) = access.next_element::<StrictJsonValue>()? {
                    out.push(v);
                }
                Ok(StrictJsonValue::Array(out))
            }

            fn visit_map<A>(self, access: A) -> std::result::Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let map = StrictJsonMap::from_map_access(access)?;
                Ok(StrictJsonValue::Object(map))
            }
        }

        deserializer.deserialize_any(StrictValueVisitor)
    }
}

#[derive(Debug)]
struct StrictJsonMap(Vec<(String, StrictJsonValue)>);

impl StrictJsonMap {
    fn from_map_access<'de, A>(mut access: A) -> std::result::Result<Self, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut out = Vec::new();
        let mut keys = HashSet::new();
        while let Some((key, value)) = access.next_entry::<String, StrictJsonValue>()? {
            if !keys.insert(key.clone()) {
                return Err(serde::de::Error::custom(format!(
                    "duplicate JSON object member name: {key}"
                )));
            }
            out.push((key, value));
        }
        Ok(StrictJsonMap(out))
    }
}

impl StrictJsonValue {
    fn into_value(self) -> Value {
        match self {
            StrictJsonValue::Null => Value::Null,
            StrictJsonValue::Bool(v) => Value::Bool(v),
            StrictJsonValue::Number(v) => Value::Number(v),
            StrictJsonValue::String(v) => Value::String(v),
            StrictJsonValue::Array(values) => Value::Array(
                values
                    .into_iter()
                    .map(StrictJsonValue::into_value)
                    .collect(),
            ),
            StrictJsonValue::Object(StrictJsonMap(entries)) => {
                let mut map = serde_json::Map::with_capacity(entries.len());
                for (k, v) in entries {
                    map.insert(k, v.into_value());
                }
                Value::Object(map)
            }
        }
    }
}

pub fn from_path<T: DeserializeOwned>(path: &Path, label: &str) -> Result<T> {
    let bytes =
        std::fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    from_slice(&bytes, label)
}

pub fn from_slice<T: DeserializeOwned>(bytes: &[u8], label: &str) -> Result<T> {
    let strict: StrictJsonValue = serde_json::from_slice(bytes)
        .map_err(|e| anyhow::anyhow!("failed to parse {} as strict JSON: {}", label, e))?;
    validate_i_json_subset(&strict)?;
    let value = strict.into_value();
    let parsed = serde_json::from_value(value)
        .with_context(|| format!("failed to decode {} from strict JSON", label))?;
    Ok(parsed)
}

fn validate_i_json_subset(value: &StrictJsonValue) -> Result<()> {
    match value {
        StrictJsonValue::Null | StrictJsonValue::Bool(_) | StrictJsonValue::String(_) => Ok(()),
        StrictJsonValue::Number(number) => {
            if number.is_i64() || number.is_u64() {
                Ok(())
            } else {
                anyhow::bail!("non-integer number rejected by strict I-JSON subset")
            }
        }
        StrictJsonValue::Array(values) => {
            for v in values {
                validate_i_json_subset(v)?;
            }
            Ok(())
        }
        StrictJsonValue::Object(StrictJsonMap(entries)) => {
            for (_, value) in entries {
                validate_i_json_subset(value)?;
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_duplicate_keys() {
        let err = from_slice::<serde_json::Value>(br#"{"a":1,"a":2}"#, "dup").unwrap_err();
        assert!(err
            .to_string()
            .contains("duplicate JSON object member name"));
    }

    #[test]
    fn rejects_floats() {
        let err = from_slice::<serde_json::Value>(br#"{"a":1.5}"#, "float").unwrap_err();
        assert!(err
            .to_string()
            .contains("non-integer number rejected by strict I-JSON subset"));
    }
}
