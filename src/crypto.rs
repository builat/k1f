//! Symmetric crypto for per-user blobs stored in SQLite.
//!
//! - Key derivation: Argon2id (memory-hard KDF).
//! - Encryption: ChaCha20-Poly1305 (authenticated).
//!
//! The 32-byte master key is derived from a user passphrase and held only in
//! memory (see [`crate::state`]). For each blob we generate a fresh random
//! 12-byte nonce and store `nonce || ciphertext` (ciphertext already includes
//! the 16-byte Poly1305 tag).

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Key, Nonce,
};
use rand::RngCore;
use thiserror::Error;
use zeroize::Zeroizing;

/// Bytes of the per-blob random nonce.
pub const NONCE_LEN: usize = 12;
/// Bytes of the Argon2id salt (for key derivation).
pub const SALT_LEN: usize = 16;
/// Bytes of the derived symmetric key (ChaCha20 = 256 bits).
pub const KEY_LEN: usize = 32;

#[derive(Debug, Error)]
pub enum CryptoError {
    #[error("Decryption failed (wrong key or corrupted data)")]
    Decrypt,
    #[error("Key derivation failed: {0}")]
    Kdf(String),
    #[error("Invalid nonce length: expected {expected}, got {actual}")]
    NonceLen { expected: usize, actual: usize },
}

/// A 32-byte symmetric key, zeroized on drop.
pub type SecretKey = Zeroizing<[u8; KEY_LEN]>;

/// Derive a 32-byte key from a passphrase and a salt using Argon2id.
///
/// The salt MUST be unique per user; it is stored alongside the user record.
/// The same `(passphrase, salt)` pair always yields the same key, so the key
/// can be re-derived on every bot restart after the user re-enters the
/// passphrase.
pub fn derive_key(passphrase: &[u8], salt: &[u8]) -> Result<SecretKey, CryptoError> {
    // Slightly above Argon2 defaults (19 MiB / 2 iters / 1 lane) to raise the
    // cost of offline guessing without making interactive /key set sluggish.
    let params = Params::new(32 * 1024, 3, 1, None).map_err(|e| CryptoError::Kdf(e.to_string()))?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);

    let mut key = Zeroizing::new([0u8; KEY_LEN]);
    argon2
        .hash_password_into(passphrase, salt, key.as_mut())
        .map_err(|e| CryptoError::Kdf(e.to_string()))?;
    Ok(key)
}

/// Generate `len` cryptographically secure random bytes.
pub fn random_bytes(len: usize) -> Vec<u8> {
    let mut buf = vec![0u8; len];
    rand::rng().fill_bytes(&mut buf);
    buf
}

/// Encrypt `plaintext` with `key`. Returns `nonce(12) || ciphertext+tag`.
///
/// Each call uses a fresh random nonce, so encrypting the same plaintext twice
/// yields different ciphertexts.
pub fn encrypt(key: &SecretKey, plaintext: &[u8]) -> Result<Vec<u8>, CryptoError> {
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key.as_ref()));
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .map_err(|_| CryptoError::Decrypt)?;

    let mut out = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

/// Decrypt a blob produced by [`encrypt`].
pub fn decrypt(key: &SecretKey, blob: &[u8]) -> Result<Vec<u8>, CryptoError> {
    if blob.len() < NONCE_LEN {
        return Err(CryptoError::NonceLen {
            expected: NONCE_LEN,
            actual: blob.len(),
        });
    }
    let (nonce_bytes, ciphertext) = blob.split_at(NONCE_LEN);
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key.as_ref()));
    cipher
        .decrypt(Nonce::from_slice(nonce_bytes), ciphertext)
        .map_err(|_| CryptoError::Decrypt)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key_from(pass: &str, salt: &[u8]) -> SecretKey {
        derive_key(pass.as_bytes(), salt).unwrap()
    }

    #[test]
    fn round_trip() {
        let key = key_from("correct horse battery staple", &[0u8; SALT_LEN]);
        let pt = b"hello, context";
        let blob = encrypt(&key, pt).unwrap();
        let recovered = decrypt(&key, &blob).unwrap();
        assert_eq!(recovered, pt);
    }

    #[test]
    fn wrong_passphrase_fails_to_decrypt() {
        let salt = random_bytes(SALT_LEN);
        let key_a = key_from("passphrase-one", &salt);
        let key_b = key_from("passphrase-two", &salt);

        let blob = encrypt(&key_a, b"secret").unwrap();
        assert!(decrypt(&key_b, &blob).is_err());
    }

    #[test]
    fn same_plaintext_yields_different_ciphertext() {
        let key = key_from("pass", &[1u8; SALT_LEN]);
        let a = encrypt(&key, b"identical").unwrap();
        let b = encrypt(&key, b"identical").unwrap();
        // Different nonces => different blobs.
        assert_ne!(a, b);
        // Both decrypt back to the same plaintext.
        assert_eq!(decrypt(&key, &a).unwrap(), b"identical");
        assert_eq!(decrypt(&key, &b).unwrap(), b"identical");
    }

    #[test]
    fn derive_key_is_deterministic_for_same_inputs() {
        let salt = random_bytes(SALT_LEN);
        let k1 = derive_key(b"same-pass", &salt).unwrap();
        let k2 = derive_key(b"same-pass", &salt).unwrap();
        assert_eq!(k1.as_ref(), k2.as_ref());
    }

    #[test]
    fn corrupted_blob_fails() {
        let key = key_from("p", &[2u8; SALT_LEN]);
        let mut blob = encrypt(&key, b"data").unwrap();
        // Flip a byte in the ciphertext region (after the nonce).
        let last = blob.len() - 1;
        blob[last] ^= 0xff;
        assert!(decrypt(&key, &blob).is_err());
    }
}
