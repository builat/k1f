//! Process-wide application state: the DB handle and the in-memory key store.
//!
//! The key store holds each user's derived 32-byte encryption key for the
//! duration of their session. Keys are NOT persisted: after a process restart
//! the user must re-enter the passphrase via `/key set` to re-derive the key.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::crypto::SecretKey;
use crate::db::DbHandle;

/// Maps telegram user id -> derived encryption key.
type KeyStore = RwLock<HashMap<i64, SecretKey>>;

#[derive(Clone)]
pub struct AppState {
    pub db: DbHandle,
    keys: Arc<KeyStore>,
}

impl AppState {
    pub fn new(db: DbHandle) -> Self {
        Self {
            db,
            keys: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Store (or replace) the derived key for a user.
    pub fn set_key(&self, tg_id: i64, key: SecretKey) {
        let mut store = self.keys.write().unwrap();
        store.insert(tg_id, key);
    }

    /// Remove the key from memory (the DB record and encrypted data are kept).
    pub fn clear_key(&self, tg_id: i64) -> bool {
        let mut store = self.keys.write().unwrap();
        store.remove(&tg_id).is_some()
    }

    /// Run a closure with the user's key, if it's currently in memory.
    ///
    /// The closure receives a borrowed key and may not store it anywhere that
    /// outlives the call (the read guard is held for the closure's duration).
    pub fn with_key<R>(&self, tg_id: i64, f: impl FnOnce(&SecretKey) -> R) -> Option<R> {
        let store = self.keys.read().unwrap();
        store.get(&tg_id).map(f)
    }

    pub fn has_key(&self, tg_id: i64) -> bool {
        self.keys.read().unwrap().contains_key(&tg_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto;

    fn fresh_state() -> AppState {
        AppState::new(DbHandle::open(":memory:").unwrap())
    }

    #[test]
    fn set_and_use_key() {
        let s = fresh_state();
        let key = crypto::derive_key(b"pw", &[1u8; crypto::SALT_LEN]).unwrap();
        s.set_key(5, key);
        assert!(s.has_key(5));
        let got = s.with_key(5, |k| k.as_ref()[0]).unwrap();
        assert_eq!(
            got,
            crypto::derive_key(b"pw", &[1u8; crypto::SALT_LEN])
                .unwrap()
                .as_ref()[0]
        );
    }

    #[test]
    fn clear_key_removes_but_db_untouched() {
        let s = fresh_state();
        let key = crypto::derive_key(b"pw", &[1u8; crypto::SALT_LEN]).unwrap();
        s.set_key(6, key);
        assert!(s.clear_key(6));
        assert!(!s.has_key(6));
        // Idempotent.
        assert!(!s.clear_key(6));
    }

    #[test]
    fn with_key_returns_none_when_absent() {
        let s = fresh_state();
        assert!(s.with_key(999, |_| ()).is_none());
    }
}
