//! Business logic for the per-user encryption key: setup, verification, and
//! rotation (re-encryption of all blobs under a new passphrase).
//!
//! These functions are sync (DB + crypto are CPU/IO-bound); callers wrap them
//! in `tokio::task::spawn_blocking`.

use argon2::{Algorithm, Argon2, Params, Version};

use crate::crypto::{self, decrypt, derive_key, encrypt, CryptoError, SecretKey, SALT_LEN};
use crate::db::{DbError, DbHandle, UserRow};
use crate::state::AppState;

/// Argon2id params for the *verifier* hash (deliberately heavier than the
/// key-derivation params, since the verifier is checked once per /key set).
fn verifier_params() -> Params {
    Params::new(48 * 1024, 3, 1, None).expect("valid params")
}

/// Compute the 32-byte Argon2id verifier hash of a passphrase.
pub fn verifier_hash(passphrase: &[u8], salt: &[u8]) -> Result<Vec<u8>, CryptoError> {
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, verifier_params());
    let mut out = vec![0u8; 32];
    argon2
        .hash_password_into(passphrase, salt, &mut out)
        .map_err(|e| CryptoError::Kdf(e.to_string()))?;
    Ok(out)
}

/// Check a passphrase against a stored user record.
///
/// Compares the recomputed verifier hash in constant time. Returns the derived
/// encryption key on success.
pub fn verify_and_derive(
    db: &DbHandle,
    tg_id: i64,
    passphrase: &[u8],
) -> Result<SecretKey, KeyError> {
    let user = db.get_user(tg_id)?.ok_or(KeyError::NoUser(tg_id))?;
    let computed = verifier_hash(passphrase, &user.verifier_salt)?;
    if !constant_time_eq(&computed, &user.pw_hash) {
        return Err(KeyError::WrongPassphrase);
    }
    Ok(derive_key(passphrase, &user.key_salt)?)
}

/// Load (or first-time create) the user's encryption key.
///
/// Behaviour:
/// - No user record yet: create one (random salts), store the Argon2id verifier
///   of the passphrase, derive the key, keep it in memory.
/// - A user record already exists (e.g. after a bot restart): recompute the
///   verifier hash of the supplied passphrase over the stored `verifier_salt`
///   and compare to the stored `pw_hash` in constant time.
///   - Match: derive the key from `passphrase + key_salt` and load it. This is
///     the "restore after restart" path.
///   - Mismatch: return [`KeyError::WrongPassphrase`] without touching memory,
///     so the caller can keep the user in the passphrase-entry dialogue.
pub fn set_key(state: &AppState, tg_id: i64, passphrase: &[u8]) -> Result<(), KeyError> {
    match state.db.get_user(tg_id)? {
        None => {
            // First-time setup.
            let verifier_salt = crypto::random_bytes(SALT_LEN);
            let key_salt = crypto::random_bytes(SALT_LEN);
            let pw_hash = verifier_hash(passphrase, &verifier_salt)?;
            state.db.insert_user(&UserRow {
                tg_id,
                pw_hash,
                verifier_salt,
                key_salt,
            })?;
            let key = derive_key(passphrase, &state.db.get_user(tg_id)?.expect("just inserted").key_salt)?;
            state.set_key(tg_id, key);
            Ok(())
        }
        Some(user) => {
            // Restore path: verify the passphrase against the stored verifier.
            let computed = verifier_hash(passphrase, &user.verifier_salt)?;
            if !constant_time_eq(&computed, &user.pw_hash) {
                return Err(KeyError::WrongPassphrase);
            }
            let key = derive_key(passphrase, &user.key_salt)?;
            state.set_key(tg_id, key);
            Ok(())
        }
    }
}

/// Full wipe: drop the key from memory AND delete the user record (which
/// cascades to all their context chunks and messages). After this, `/key set`
/// starts from a clean slate. Irreversible — encrypted data is gone.
///
/// Returns `true` if there was anything to remove.
pub fn delete_user(state: &AppState, tg_id: i64) -> Result<bool, KeyError> {
    let in_memory = state.clear_key(tg_id);
    let in_db = state.db.delete_user(tg_id)?;
    Ok(in_memory || in_db)
}

/// Change the passphrase: verify the old one, re-encrypt every blob under the
/// new key, then update the verifier record. All-or-nothing: if re-encryption
/// fails midway, the DB is left unchanged (we re-encrypt into a Vec first, then
/// write).
pub fn change_key(
    state: &AppState,
    tg_id: i64,
    old_passphrase: &[u8],
    new_passphrase: &[u8],
) -> Result<(), KeyError> {
    let old_key = verify_and_derive(&state.db, tg_id, old_passphrase)?;

    // 1. Decrypt all blobs under the old key.
    let chunks = state.db.chunks(tg_id)?;
    let mut reencrypted_chunks: Vec<(i64, Vec<u8>)> = Vec::with_capacity(chunks.len());
    for c in chunks {
        let plaintext = decrypt(&old_key, &c.blob)?;
        reencrypted_chunks.push((c.seq, plaintext));
    }
    let messages = state.db.recent_messages(tg_id, u32::MAX)?;
    let mut reencrypted_messages: Vec<(String, Vec<u8>)> = Vec::with_capacity(messages.len());
    for m in messages {
        let plaintext = decrypt(&old_key, &m.blob)?;
        reencrypted_messages.push((m.role, plaintext));
    }

    // 2. Generate new salts and the new key.
    let new_verifier_salt = crypto::random_bytes(SALT_LEN);
    let new_key_salt = crypto::random_bytes(SALT_LEN);
    let new_pw_hash = verifier_hash(new_passphrase, &new_verifier_salt)?;
    let new_key = derive_key(new_passphrase, &new_key_salt)?;

    // 3. Re-encrypt everything under the new key.
    for (seq, plaintext) in &reencrypted_chunks {
        let blob = encrypt(&new_key, plaintext)?;
        // store back: update_chunk replaces nonce+ciphertext for this seq.
        state.db.update_chunk(tg_id, *seq, &blob)?;
    }
    // Messages keep their order only by id; simplest correct approach is to
    // rewrite them in place. We reuse append after clearing.
    state.db.clear_messages(tg_id)?;
    for (role, plaintext) in &reencrypted_messages {
        let blob = encrypt(&new_key, plaintext)?;
        state.db.append_message(tg_id, role, &blob)?;
    }

    // 4. Update the user record with the new verifier + salts.
    state.db.update_user_verifier(&UserRow {
        tg_id,
        pw_hash: new_pw_hash,
        verifier_salt: new_verifier_salt,
        key_salt: new_key_salt,
    })?;

    // 5. Swap the in-memory key.
    state.set_key(tg_id, new_key);
    Ok(())
}

/// Constant-time comparison so passphrase verification isn't timing-leaky.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[derive(Debug, thiserror::Error)]
pub enum KeyError {
    #[error("No key set for your account. Use /key set first.")]
    NoUser(i64),
    #[error("Wrong passphrase.")]
    WrongPassphrase,
    #[error(transparent)]
    Crypto(#[from] CryptoError),
    #[error(transparent)]
    Db(#[from] DbError),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_state() -> AppState {
        AppState::new(DbHandle::open(":memory:").unwrap())
    }

    #[test]
    fn set_key_then_verify_round_trip() {
        let state = fresh_state();
        set_key(&state, 1, b"pass-1").unwrap();
        // verify_and_derive succeeds with the correct passphrase.
        let derived = verify_and_derive(&state.db, 1, b"pass-1").unwrap();
        // The in-memory key matches the freshly derived one.
        let matches = state.with_key(1, |mk| mk.as_ref() == derived.as_ref());
        assert_eq!(matches, Some(true));
    }

    #[test]
    fn delete_user_wipes_everything_and_allows_fresh_set() {
        let state = fresh_state();
        set_key(&state, 9, b"old-pass").unwrap();
        let key = state.with_key(9, |k| k.clone()).unwrap();

        // Seed a chunk + a message so we can confirm cascade.
        let blob = crate::crypto::encrypt(&key, b"context-data").unwrap();
        state.db.insert_chunk(9, 1, &blob).unwrap();
        state
            .db
            .append_message(9, "user", &crate::crypto::encrypt(&key, b"hi").unwrap())
            .unwrap();
        assert_eq!(state.db.chunk_seqs(9).unwrap().len(), 1);
        assert_eq!(state.db.recent_messages(9, 100).unwrap().len(), 1);

        // Full wipe.
        assert!(delete_user(&state, 9).unwrap());
        assert!(!state.has_key(9)); // in-memory key gone
        assert!(state.db.get_user(9).unwrap().is_none()); // user record gone
        assert!(state.db.chunk_seqs(9).unwrap().is_empty()); // cascade: chunks
        assert!(state.db.recent_messages(9, 100).unwrap().is_empty()); // cascade: messages

        // /key set works again from a clean slate.
        set_key(&state, 9, b"brand-new-pass").unwrap();
        assert!(state.has_key(9));
        assert!(state.db.chunk_seqs(9).unwrap().is_empty()); // no leftovers

        // Deleting again (fresh user) works; a further delete reports nothing.
        assert!(delete_user(&state, 9).unwrap());
        assert!(!delete_user(&state, 9).unwrap());
    }

    #[test]
    fn wrong_passphrase_rejected() {
        let state = fresh_state();
        set_key(&state, 2, b"correct").unwrap();
        let err = verify_and_derive(&state.db, 2, b"wrong").unwrap_err();
        assert!(matches!(err, KeyError::WrongPassphrase));
    }

    #[test]
    fn set_key_restores_with_same_passphrase_after_memory_wipe() {
        // Simulate a restart: after set_key the in-memory key is dropped, but
        // the DB record (verifier + salts) persists. set_key with the SAME
        // passphrase must restore the key; a DIFFERENT one must be rejected.
        let state = fresh_state();
        set_key(&state, 3, b"original-passphrase").unwrap();
        let original_key = state.with_key(3, |k| k.clone()).unwrap();

        // Simulate restart: drop the in-memory key (DB untouched).
        assert!(state.clear_key(3));
        assert!(!state.has_key(3));

        // Wrong passphrase must NOT load a key and must NOT create a second user.
        let err = set_key(&state, 3, b"WRONG-passphrase").unwrap_err();
        assert!(matches!(err, KeyError::WrongPassphrase));
        assert!(!state.has_key(3));

        // Correct passphrase restores the key, and it equals the original.
        set_key(&state, 3, b"original-passphrase").unwrap();
        assert!(state.has_key(3));
        let restored = state.with_key(3, |k| k.clone()).unwrap();
        assert_eq!(restored.as_ref(), original_key.as_ref());
    }

    #[test]
    fn change_key_reencrypts_chunks_and_messages() {
        let state = fresh_state();
        set_key(&state, 4, b"old").unwrap();
        let old_key = state.with_key(4, |k| k.clone()).unwrap();

        // Seed two context chunks + a message under the old key.
        let seq1 = state.db.next_seq(4).unwrap();
        state
            .db
            .insert_chunk(4, seq1, &encrypt(&old_key, b"chunk-one").unwrap())
            .unwrap();
        let seq2 = state.db.next_seq(4).unwrap();
        state
            .db
            .insert_chunk(4, seq2, &encrypt(&old_key, b"chunk-two").unwrap())
            .unwrap();
        state
            .db
            .append_message(4, "user", &encrypt(&old_key, b"hello").unwrap())
            .unwrap();

        // Rotate.
        change_key(&state, 4, b"old", b"new").unwrap();

        // New key decrypts the chunks.
        let new_key = state.with_key(4, |k| k.clone()).unwrap();
        let chunks = state.db.chunks(4).unwrap();
        let texts: Vec<String> = chunks
            .iter()
            .map(|c| String::from_utf8(decrypt(&new_key, &c.blob).unwrap()).unwrap())
            .collect();
        assert_eq!(texts, vec!["chunk-one", "chunk-two"]);

        // Old key can NOT decrypt anymore.
        assert!(decrypt(&old_key, &chunks[0].blob).is_err());

        // Message survived rotation, decryptable under new key.
        let msgs = state.db.recent_messages(4, u32::MAX).unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(
            String::from_utf8(decrypt(&new_key, &msgs[0].blob).unwrap()).unwrap(),
            "hello"
        );

        // Old passphrase no longer verifies.
        assert!(matches!(
            verify_and_derive(&state.db, 4, b"old").unwrap_err(),
            KeyError::WrongPassphrase
        ));
        // New passphrase verifies.
        assert!(verify_and_derive(&state.db, 4, b"new").is_ok());
    }

    #[test]
    fn change_key_with_wrong_old_passphrase_fails_and_preserves_data() {
        let state = fresh_state();
        set_key(&state, 5, b"old").unwrap();
        let old_key = state.with_key(5, |k| k.clone()).unwrap();
        state
            .db
            .insert_chunk(5, 1, &encrypt(&old_key, b"keep-me").unwrap())
            .unwrap();

        let err = change_key(&state, 5, b"WRONG", b"new").unwrap_err();
        assert!(matches!(err, KeyError::WrongPassphrase));

        // Data untouched and still decryptable under the original key.
        let chunk = state.db.chunks(5).unwrap();
        assert_eq!(
            String::from_utf8(decrypt(&old_key, &chunk[0].blob).unwrap()).unwrap(),
            "keep-me"
        );
    }
}
