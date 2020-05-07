use zeroize::Zeroize;

/// Provides a wrapper around a `Vec<u8>` that implements `Zeroize`.
#[derive(Zeroize)]
#[zeroize(drop)]
pub struct SecretBytes(Vec<u8>);

impl SecretBytes {
    /// Instantiates `Self` with an all-zeros byte array of length `len`.
    pub fn zero(len: usize) -> Self {
        Self(vec![0; len])
    }

    /// Returns a mutable reference to the underlying bytes.
    pub fn as_mut_bytes(&mut self) -> &mut [u8] {
        &mut self.0
    }

    /// Returns a reference to the underlying bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}
