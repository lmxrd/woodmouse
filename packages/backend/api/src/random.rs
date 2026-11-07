//! Random bytes for secrets (two-factor secrets, recovery codes, API keys), from the
//! operating system's generator through `rand`.

use rand::Rng;

/// `N` random bytes.
pub fn bytes<const N: usize>() -> [u8; N] {
    let mut bytes = [0u8; N];
    rand::rng().fill_bytes(&mut bytes);
    bytes
}

#[cfg(test)]
mod tests {
    #[test]
    fn differs_every_time() {
        assert_ne!(super::bytes::<32>(), super::bytes::<32>());
    }
}
