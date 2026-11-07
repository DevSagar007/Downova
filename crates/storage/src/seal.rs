//! Sealing of protected URL material in SQLite (SEC-01, ADR 0027): AES-256-GCM (`ring`) under a
//! data key that lives only in the OS credential store. The service passes the key in after it
//! read it from the store; without it nothing is sealed or unsealed and protected material stays
//! in memory only.
//!
//! Layout: `0x01 || nonce (12) || ciphertext || tag (16)`. The AAD binds a ciphertext to its job and
//! its column, so it cannot be moved to another job or field.

use ring::aead::{AES_256_GCM, Aad, LessSafeKey, NONCE_LEN, Nonce, UnboundKey};
use ring::rand::{SecureRandom, SystemRandom};

const VERSION: u8 = 1;
/// Bytes a sealed value adds.
pub const OVERHEAD: usize = 1 + NONCE_LEN + 16;

/// The data key, held only in memory (and in the OS store).
pub struct Sealer {
    key: LessSafeKey,
    rng: SystemRandom,
}

impl std::fmt::Debug for Sealer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Sealer(REDACTED)")
    }
}

/// Why sealed material could not be opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SealError {
    /// Wrong key, wrong job or field, or tampered bytes.
    Invalid,
    /// No randomness for a nonce.
    Random,
}

impl Sealer {
    /// A sealer for a 32-byte key.
    pub fn new(key: &[u8; 32]) -> Self {
        // A 32-byte key is always valid for AES-256-GCM.
        let unbound = UnboundKey::new(&AES_256_GCM, key).unwrap_or_else(|_| unreachable!());
        Self {
            key: LessSafeKey::new(unbound),
            rng: SystemRandom::new(),
        }
    }

    /// A fresh random key.
    pub fn generate_key() -> Result<[u8; 32], SealError> {
        let mut key = [0u8; 32];
        SystemRandom::new()
            .fill(&mut key)
            .map_err(|_| SealError::Random)?;
        Ok(key)
    }

    pub fn seal(&self, aad: &str, plaintext: &[u8]) -> Result<Vec<u8>, SealError> {
        let mut nonce = [0u8; NONCE_LEN];
        self.rng.fill(&mut nonce).map_err(|_| SealError::Random)?;
        let mut buf = plaintext.to_vec();
        self.key
            .seal_in_place_append_tag(
                Nonce::assume_unique_for_key(nonce),
                Aad::from(aad.as_bytes()),
                &mut buf,
            )
            .map_err(|_| SealError::Invalid)?;
        let mut out = Vec::with_capacity(OVERHEAD.saturating_add(plaintext.len()));
        out.push(VERSION);
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&buf);
        Ok(out)
    }

    pub fn open(&self, aad: &str, sealed: &[u8]) -> Result<Vec<u8>, SealError> {
        let (&version, rest) = sealed.split_first().ok_or(SealError::Invalid)?;
        if version != VERSION || rest.len() < NONCE_LEN.saturating_add(16) {
            return Err(SealError::Invalid);
        }
        let (nonce, body) = rest.split_at(NONCE_LEN);
        let nonce = Nonce::try_assume_unique_for_key(nonce).map_err(|_| SealError::Invalid)?;
        let mut buf = body.to_vec();
        let plain = self
            .key
            .open_in_place(nonce, Aad::from(aad.as_bytes()), &mut buf)
            .map_err(|_| SealError::Invalid)?;
        Ok(plain.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_binds_aad_and_detects_tampering() {
        let s = Sealer::new(&Sealer::generate_key().unwrap());
        let a = s.seal("job_1|url", b"https://h/f?token=SYNTH").unwrap();
        assert!(
            !a.windows(5).any(|w| w == b"SYNTH"),
            "no plaintext in the blob"
        );
        assert_eq!(s.open("job_1|url", &a).unwrap(), b"https://h/f?token=SYNTH");
        assert_eq!(
            s.open("job_2|url", &a),
            Err(SealError::Invalid),
            "bound to its job"
        );
        let mut t = a.clone();
        *t.last_mut().unwrap() ^= 1;
        assert_eq!(s.open("job_1|url", &t), Err(SealError::Invalid));
        let other = Sealer::new(&Sealer::generate_key().unwrap());
        assert_eq!(
            other.open("job_1|url", &a),
            Err(SealError::Invalid),
            "another key"
        );
        assert_ne!(
            s.seal("x", b"same").unwrap(),
            s.seal("x", b"same").unwrap(),
            "random nonces"
        );
        assert_eq!(a.len(), OVERHEAD + 23);
    }
}
