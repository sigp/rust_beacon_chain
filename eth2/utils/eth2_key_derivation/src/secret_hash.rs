use crate::derived_key::HASH_SIZE;
use zeroize::Zeroize;

/// Provides a wrapper around a `[u8; HASH_SIZE]` that implements `Zeroize`.
#[derive(Zeroize)]
#[zeroize(drop)]
pub struct SecretHash([u8; HASH_SIZE]);

impl SecretHash {
    /// Instantiates `Self` with all zeros.
    pub fn zero() -> Self {
        Self([0; HASH_SIZE])
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn as_mut_bytes(&mut self) -> &mut [u8] {
        &mut self.0
    }
}
