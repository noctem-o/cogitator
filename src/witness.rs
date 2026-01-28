use anyhow::Result;
use blake3::Hasher;

use crate::model::{TraceEvent, WitnessedMetadata};
use crate::trace::{encode_event, encode_witnessed_metadata};

pub struct Witness {
    hash: [u8; 32],
}

impl Witness {
    pub fn new(metadata_bytes: &[u8]) -> Result<Self> {
        let mut hasher = Hasher::new();
        hasher.update(b"COGITATOR");
        hasher.update(metadata_bytes);
        let hash = *hasher.finalize().as_bytes();
        Ok(Self { hash })
    }

    pub fn update(&mut self, event_bytes: &[u8]) -> Result<()> {
        let mut hasher = Hasher::new();
        hasher.update(&self.hash);
        hasher.update(event_bytes);
        self.hash = *hasher.finalize().as_bytes();
        Ok(())
    }

    pub fn finalize_hex(&self) -> String {
        blake3::Hash::from(self.hash).to_hex().to_string()
    }
}

pub fn compute_witness_root(metadata: &WitnessedMetadata, events: &[TraceEvent]) -> Result<String> {
    let metadata_bytes = encode_witnessed_metadata(metadata)?;
    let mut witness = Witness::new(&metadata_bytes)?;

    for event in events {
        let event_bytes = encode_event(event)?;
        witness.update(&event_bytes)?;
    }

    Ok(witness.finalize_hex())
}
