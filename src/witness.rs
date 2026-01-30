use anyhow::Result;
use blake3::Hasher;

/// Cryptographic witness accumulator using BLAKE3
///
/// Builds a hash chain over metadata and trace events with domain-separated
/// length-prefix encoding to prevent preimage collisions.
pub struct Witness {
    hash: [u8; 32],
}

impl Witness {
    /// Initialize witness with metadata commitment
    ///
    /// # Arguments
    ///
    /// * `metadata_bytes` - Canonical JSON serialization of witnessed metadata
    pub fn new(metadata_bytes: &[u8]) -> Result<Self> {
        let mut hasher = Hasher::new();
        hasher.update(b"COGITATOR/WITNESS/V1/INIT");
        hasher.update(metadata_bytes);
        let hash = *hasher.finalize().as_bytes();
        Ok(Self { hash })
    }

    /// Update witness with a trace event
    ///
    /// Uses domain-separated length-prefix encoding:
    /// - `|LENGTH|` separator before event length
    /// - `|CONTENT|` separator before event content
    ///
    /// This prevents an attacker from crafting `event_bytes` that could
    /// be interpreted as a valid length prefix, eliminating preimage collision risks.
    ///
    /// # Arguments
    ///
    /// * `event_bytes` - Canonical JSON serialization of a trace event or tool call
    pub fn update(&mut self, event_bytes: &[u8]) -> Result<()> {
        let mut hasher = Hasher::new();
        hasher.update(b"COGITATOR/WITNESS/V1/STEP");
        hasher.update(&self.hash);

        // Domain-separated length-prefix encoding to prevent preimage collisions
        hasher.update(b"|LENGTH|");
        hasher.update(&(event_bytes.len() as u64).to_be_bytes());
        hasher.update(b"|CONTENT|");
        hasher.update(event_bytes);

        self.hash = *hasher.finalize().as_bytes();
        Ok(())
    }

    /// Finalize witness and return hex-encoded root hash
    pub fn finalize_hex(&self) -> String {
        blake3::Hash::from(self.hash).to_hex().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_witness_basic() {
        let metadata = b"test metadata";
        let mut witness = Witness::new(metadata).unwrap();
        witness.update(b"event 1").unwrap();
        witness.update(b"event 2").unwrap();
        let root = witness.finalize_hex();
        assert_eq!(root.len(), 64); // BLAKE3 produces 32 bytes = 64 hex chars
    }

    #[test]
    fn test_witness_determinism() {
        let metadata = b"metadata";
        let events = [b"event1".as_slice(), b"event2".as_slice()];

        let mut w1 = Witness::new(metadata).unwrap();
        for event in &events {
            w1.update(event).unwrap();
        }

        let mut w2 = Witness::new(metadata).unwrap();
        for event in &events {
            w2.update(event).unwrap();
        }

        assert_eq!(w1.finalize_hex(), w2.finalize_hex());
    }

    #[test]
    fn test_witness_order_sensitivity() {
        let metadata = b"metadata";

        let mut w1 = Witness::new(metadata).unwrap();
        w1.update(b"A").unwrap();
        w1.update(b"B").unwrap();

        let mut w2 = Witness::new(metadata).unwrap();
        w2.update(b"B").unwrap();
        w2.update(b"A").unwrap();

        assert_ne!(w1.finalize_hex(), w2.finalize_hex());
    }
}
