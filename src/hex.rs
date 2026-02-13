//! Efficient hex encoding utilities
//!
//! Centralizes hex encoding to eliminate duplicated implementations
//! and improve performance by avoiding per-byte allocations.

use std::fmt::Write;

/// Efficiently encode bytes as lowercase hexadecimal string
///
/// Uses `write!` macro to avoid allocations, unlike `format!("{:02x}", byte)`.
///
/// # Examples
///
/// ```
/// use cogitator::hex;
///
/// let data = [0xde, 0xad, 0xbe, 0xef];
/// assert_eq!(hex::encode(&data), "deadbeef");
/// ```
#[inline]
pub fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        // write! doesn't allocate, unlike format!
        write!(&mut out, "{:02x}", byte).expect("writing to String cannot fail");
    }
    out
}

/// Efficiently encode bytes as lowercase hexadecimal string (alias for compatibility)
///
/// This is an alias for [`encode`] provided for backward compatibility
/// with code that used local `hex_lower` functions.
#[inline]
pub fn hex_lower(bytes: &[u8]) -> String {
    encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_empty() {
        assert_eq!(encode(&[]), "");
    }

    #[test]
    fn test_encode_basic() {
        assert_eq!(encode(&[0x00, 0xff, 0xab, 0xcd]), "00ffabcd");
    }

    #[test]
    fn test_encode_sha256() {
        let hash = [0u8; 32];
        assert_eq!(encode(&hash).len(), 64);
    }

    #[test]
    fn test_hex_lower_alias() {
        let data = [0xde, 0xad];
        assert_eq!(hex_lower(&data), encode(&data));
    }

    #[test]
    fn test_uppercase_not_produced() {
        let data = [0xab, 0xcd, 0xef];
        let result = encode(&data);
        assert_eq!(result, result.to_lowercase());
    }
}
