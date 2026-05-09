use cogitator::model::{TRACE_SCHEMA_VERSION, WITNESS_MANIFEST_SCHEMA_VERSION};
use cogitator::tooling::TOOL_TRANSCRIPT_SCHEMA_VERSION;

#[test]
fn protocol_spec_mentions_current_schema_constants_and_omits_stale_markers() {
    let spec =
        std::fs::read_to_string("spec/COGITATOR_WITNESS_PROTOCOL.md").expect("read protocol spec");

    assert!(
        spec.contains(&format!(
            "Witnessed trace schema version: **{}**",
            TRACE_SCHEMA_VERSION
        )),
        "spec must mention current trace schema version"
    );
    assert!(spec.contains(&format!(
        "Witness manifest schema version: **{}**",
        WITNESS_MANIFEST_SCHEMA_VERSION
    )));
    assert!(spec.contains(&format!(
        "Tool transcript schema version: **{}**",
        TOOL_TRANSCRIPT_SCHEMA_VERSION
    )));

    for stale in [
        "schema_version\": 4",
        "BLAKE3(file_bytes)",
        "BLAKE3(RFC8785(witness_manifest))",
        "call_hash",
        "entry_hash",
    ] {
        assert!(
            !spec.contains(stale),
            "stale phrase should not appear in spec: {stale}"
        );
    }
}
