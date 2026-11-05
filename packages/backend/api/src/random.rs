//! Random bytes for secrets (two-factor secrets, recovery codes, API keys).
//!
//! No random-number crate is a direct dependency: `uuid`'s v4 ids come from the operating system's
//! generator, and SHA-256 spreads the 244 random bits of two of them evenly over its 256-bit output.

use sha2::{Digest, Sha256};

/// `N` random bytes, `N` at most 32.
pub fn bytes<const N: usize>() -> [u8; N] {
    const { assert!(N <= 32, "at most 32 bytes at a time") };
    let hash = Sha256::new()
        .chain_update(uuid::Uuid::new_v4().as_bytes())
        .chain_update(uuid::Uuid::new_v4().as_bytes())
        .finalize();
    let mut bytes = [0u8; N];
    bytes.copy_from_slice(&hash[..N]);
    bytes
}

#[cfg(test)]
mod tests {
    #[test]
    fn differs_every_time() {
        assert_ne!(super::bytes::<32>(), super::bytes::<32>());
    }
}
