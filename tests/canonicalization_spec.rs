use cogitator::canonical_json;

fn independent_canonicalize(value: &serde_json::Value) -> String {
    fn sort_value(value: serde_json::Value) -> serde_json::Value {
        match value {
            serde_json::Value::Object(map) => {
                let mut entries: Vec<(String, serde_json::Value)> =
                    map.into_iter().map(|(k, v)| (k, sort_value(v))).collect();
                entries.sort_by(|(ka, _), (kb, _)| {
                    let mut ia = ka.encode_utf16();
                    let mut ib = kb.encode_utf16();
                    loop {
                        match (ia.next(), ib.next()) {
                            (Some(a), Some(b)) if a == b => continue,
                            (Some(a), Some(b)) => break a.cmp(&b),
                            (None, Some(_)) => break std::cmp::Ordering::Less,
                            (Some(_), None) => break std::cmp::Ordering::Greater,
                            (None, None) => break std::cmp::Ordering::Equal,
                        }
                    }
                });
                let mut out = serde_json::Map::new();
                for (k, v) in entries {
                    out.insert(k, v);
                }
                serde_json::Value::Object(out)
            }
            serde_json::Value::Array(values) => {
                serde_json::Value::Array(values.into_iter().map(sort_value).collect())
            }
            other => other,
        }
    }

    serde_json::to_string(&sort_value(value.clone())).expect("independent canonicalization")
}

#[test]
fn rfc8785_utf16_sort_vector() {
    let value = serde_json::json!({"\u{E000}":1,"😀":2});
    let actual =
        String::from_utf8(canonical_json::to_vec(&value).expect("canonical bytes")).expect("utf8");
    assert_eq!(actual, r#"{"😀":2,"":1}"#);
}

#[test]
fn canonicalization_rejects_float_numbers() {
    let err =
        canonical_json::to_vec(&serde_json::json!({"x": 1.25})).expect_err("floats are rejected");
    assert!(err.to_string().contains("non-integer number"));
}

#[test]
fn differential_canonicalization_matches_independent_encoder() {
    let cases = vec![
        serde_json::json!({"b": [3,2,1], "a": {"k":"v", "n":7}}),
        serde_json::json!([{"z":0,"a":1}, {"nested":{"b":2,"a":1}}]),
    ];

    for case in cases {
        let ours = String::from_utf8(canonical_json::to_vec(&case).expect("ours")).unwrap();
        let independent = independent_canonicalize(&case);
        assert_eq!(ours, independent);
    }
}
