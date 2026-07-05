//! SQLite storage for per-user encrypted data.
//!
//! Schema: `users`, `context_chunks`, `messages` (see [`MIGRATION_SQL`]).
//! All blobs (`ciphertext`, `nonce`) are stored in the shape produced by
//! [`crate::crypto::encrypt`]; this module never sees plaintext.
//!
//! The whole DB is one `Arc<Mutex<Connection>>`. SQLite serializes writes
//! internally, and for a single-process bot this is simpler and correct. Async
//! callers must wrap access in `tokio::task::spawn_blocking`.

use std::path::Path;
use std::sync::{Arc, Mutex};

use rusqlite::{params, Connection, OptionalExtension};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DbError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("Context chunk {seq} not found for user {tg_id}")]
    ChunkNotFound { tg_id: i64, seq: i64 },
    #[error("Lock poisoned")]
    Poisoned,
}

pub type Result<T> = std::result::Result<T, DbError>;

/// A cheaply-clonable handle to the database.
#[derive(Clone)]
pub struct DbHandle {
    conn: Arc<Mutex<Connection>>,
}

/// Persisted per-user record. `pw_hash` is an Argon2id verifier hash of the
/// passphrase over a verifier salt (NOT the encryption key).
#[derive(Debug, Clone)]
pub struct UserRow {
    pub tg_id: i64,
    pub pw_hash: Vec<u8>, // Argon2id(passphrase, verifier_salt), 32 bytes
    pub verifier_salt: Vec<u8>,
    pub key_salt: Vec<u8>, // salt used to derive the encryption key
}

/// One encrypted context chunk, in order of `seq`.
#[derive(Debug, Clone)]
pub struct ChunkRow {
    #[allow(dead_code)]
    pub tg_id: i64,
    pub seq: i64,
    pub blob: Vec<u8>, // nonce(12) || ciphertext+tag
}

/// One encrypted message in the dialogue history.
#[derive(Debug, Clone)]
pub struct MessageRow {
    #[allow(dead_code)]
    pub tg_id: i64,
    pub role: String, // "user" | "assistant"
    pub blob: Vec<u8>,
}

const MIGRATION_SQL: &str = "
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS users (
    tg_id          INTEGER PRIMARY KEY,
    pw_hash        BLOB    NOT NULL,
    verifier_salt  BLOB    NOT NULL,
    key_salt       BLOB    NOT NULL
);

CREATE TABLE IF NOT EXISTS context_chunks (
    tg_id       INTEGER NOT NULL REFERENCES users(tg_id) ON DELETE CASCADE,
    seq         INTEGER NOT NULL,
    nonce       BLOB    NOT NULL,
    ciphertext  BLOB    NOT NULL,
    updated_at  TEXT    NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (tg_id, seq)
);

CREATE TABLE IF NOT EXISTS messages (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    tg_id       INTEGER NOT NULL REFERENCES users(tg_id) ON DELETE CASCADE,
    role        TEXT    NOT NULL,
    nonce       BLOB    NOT NULL,
    ciphertext  BLOB    NOT NULL,
    created_at  TEXT    NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_messages_tg ON messages(tg_id, id);
";

impl DbHandle {
    /// Open (or create) the database at `path` and run migrations.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(MIGRATION_SQL)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>> {
        self.conn.lock().map_err(|_| DbError::Poisoned)
    }

    // ----- users -----------------------------------------------------------

    /// Insert a new user record. Fails if the user already exists.
    pub fn insert_user(&self, row: &UserRow) -> Result<()> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO users (tg_id, pw_hash, verifier_salt, key_salt) VALUES (?, ?, ?, ?)",
            params![row.tg_id, row.pw_hash, row.verifier_salt, row.key_salt],
        )?;
        Ok(())
    }

    pub fn get_user(&self, tg_id: i64) -> Result<Option<UserRow>> {
        let conn = self.lock()?;
        let row = conn
            .query_row(
                "SELECT tg_id, pw_hash, verifier_salt, key_salt FROM users WHERE tg_id = ?",
                params![tg_id],
                |r| {
                    Ok(UserRow {
                        tg_id: r.get(0)?,
                        pw_hash: r.get(1)?,
                        verifier_salt: r.get(2)?,
                        key_salt: r.get(3)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    /// Replace the user's verifier hash + both salts (used on passphrase change).
    pub fn update_user_verifier(&self, row: &UserRow) -> Result<()> {
        let conn = self.lock()?;
        conn.execute(
            "UPDATE users SET pw_hash = ?, verifier_salt = ?, key_salt = ? WHERE tg_id = ?",
            params![row.pw_hash, row.verifier_salt, row.key_salt, row.tg_id],
        )?;
        Ok(())
    }

    /// Delete a user record. Foreign keys are ON DELETE CASCADE, so all the
    /// user's context chunks and messages are removed with it.
    /// Returns true if a row was actually deleted.
    pub fn delete_user(&self, tg_id: i64) -> Result<bool> {
        let conn = self.lock()?;
        let changed = conn.execute("DELETE FROM users WHERE tg_id = ?", params![tg_id])?;
        Ok(changed > 0)
    }

    // ----- context chunks -------------------------------------------------

    /// Next available `seq` for this user (1 if there are no chunks yet).
    pub fn next_seq(&self, tg_id: i64) -> Result<i64> {
        let conn = self.lock()?;
        let max: Option<i64> = conn
            .query_row(
                "SELECT MAX(seq) FROM context_chunks WHERE tg_id = ?",
                params![tg_id],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        Ok(max.unwrap_or(0) + 1)
    }

    /// Insert a new context chunk with an explicit `seq`.
    pub fn insert_chunk(&self, tg_id: i64, seq: i64, blob: &[u8]) -> Result<()> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO context_chunks (tg_id, seq, nonce, ciphertext) VALUES (?, ?, ?, ?)",
            // blob is nonce||ciphertext; store nonce separately for clarity.
            params![
                tg_id,
                seq,
                &blob[..crate::crypto::NONCE_LEN],
                &blob[crate::crypto::NONCE_LEN..]
            ],
        )?;
        Ok(())
    }

    /// Replace an existing chunk's ciphertext (keeps `seq`).
    pub fn update_chunk(&self, tg_id: i64, seq: i64, blob: &[u8]) -> Result<()> {
        let conn = self.lock()?;
        let changed = conn.execute(
            "UPDATE context_chunks
             SET nonce = ?, ciphertext = ?, updated_at = datetime('now')
             WHERE tg_id = ? AND seq = ?",
            params![
                &blob[..crate::crypto::NONCE_LEN],
                &blob[crate::crypto::NONCE_LEN..],
                tg_id,
                seq
            ],
        )?;
        if changed == 0 {
            return Err(DbError::ChunkNotFound { tg_id, seq });
        }
        Ok(())
    }

    pub fn delete_chunk(&self, tg_id: i64, seq: i64) -> Result<bool> {
        let conn = self.lock()?;
        let changed = conn.execute(
            "DELETE FROM context_chunks WHERE tg_id = ? AND seq = ?",
            params![tg_id, seq],
        )?;
        Ok(changed > 0)
    }

    /// All chunks for a user, ordered by `seq` (used to assemble the GPT
    /// context). Each blob is reconstructed as `nonce || ciphertext`.
    pub fn chunks(&self, tg_id: i64) -> Result<Vec<ChunkRow>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT tg_id, seq, nonce, ciphertext FROM context_chunks
             WHERE tg_id = ? ORDER BY seq ASC",
        )?;
        let rows = stmt
            .query_map(params![tg_id], |r| {
                let tg_id: i64 = r.get(0)?;
                let seq: i64 = r.get(1)?;
                let nonce: Vec<u8> = r.get(2)?;
                let ciphertext: Vec<u8> = r.get(3)?;
                let mut blob = Vec::with_capacity(nonce.len() + ciphertext.len());
                blob.extend_from_slice(&nonce);
                blob.extend_from_slice(&ciphertext);
                Ok(ChunkRow { tg_id, seq, blob })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Just the list of `seq`s (for `/ctx list`).
    pub fn chunk_seqs(&self, tg_id: i64) -> Result<Vec<i64>> {
        let conn = self.lock()?;
        let mut stmt =
            conn.prepare("SELECT seq FROM context_chunks WHERE tg_id = ? ORDER BY seq ASC")?;
        let seqs = stmt
            .query_map(params![tg_id], |r| r.get::<_, i64>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(seqs)
    }

    // ----- messages -------------------------------------------------------

    pub fn append_message(&self, tg_id: i64, role: &str, blob: &[u8]) -> Result<()> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO messages (tg_id, role, nonce, ciphertext) VALUES (?, ?, ?, ?)",
            params![
                tg_id,
                role,
                &blob[..crate::crypto::NONCE_LEN],
                &blob[crate::crypto::NONCE_LEN..]
            ],
        )?;
        Ok(())
    }

    /// Last `limit` messages for a user, oldest-first (chat order).
    pub fn recent_messages(&self, tg_id: i64, limit: u32) -> Result<Vec<MessageRow>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT tg_id, role, nonce, ciphertext FROM (
                SELECT * FROM messages WHERE tg_id = ?
                ORDER BY id DESC LIMIT ?
             ) ORDER BY id ASC",
        )?;
        let rows = stmt
            .query_map(params![tg_id, limit], |r| {
                let tg_id: i64 = r.get(0)?;
                let role: String = r.get(1)?;
                let nonce: Vec<u8> = r.get(2)?;
                let ciphertext: Vec<u8> = r.get(3)?;
                let mut blob = Vec::with_capacity(nonce.len() + ciphertext.len());
                blob.extend_from_slice(&nonce);
                blob.extend_from_slice(&ciphertext);
                Ok(MessageRow { tg_id, role, blob })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn clear_messages(&self, tg_id: i64) -> Result<()> {
        let conn = self.lock()?;
        conn.execute("DELETE FROM messages WHERE tg_id = ?", params![tg_id])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto;

    fn tmp_db() -> DbHandle {
        DbHandle::open(":memory:").unwrap()
    }

    fn make_user(db: &DbHandle, tg_id: i64) -> UserRow {
        let row = UserRow {
            tg_id,
            pw_hash: vec![0u8; 32],
            verifier_salt: vec![1u8; crypto::SALT_LEN],
            key_salt: vec![2u8; crypto::SALT_LEN],
        };
        db.insert_user(&row).unwrap();
        row
    }

    #[test]
    fn inserts_and_reads_user() {
        let db = tmp_db();
        make_user(&db, 42);
        let got = db.get_user(42).unwrap().unwrap();
        assert_eq!(got.tg_id, 42);
        assert_eq!(got.key_salt, vec![2u8; crypto::SALT_LEN]);
    }

    #[test]
    fn chunks_are_ordered_by_seq() {
        let db = tmp_db();
        let key = crypto::derive_key(b"pw", &[9u8; crypto::SALT_LEN]).unwrap();
        make_user(&db, 7);

        // seq assigned by next_seq: 1, 2, 3.
        for text in ["alpha", "beta", "gamma"] {
            let seq = db.next_seq(7).unwrap();
            let blob = crypto::encrypt(&key, text.as_bytes()).unwrap();
            db.insert_chunk(7, seq, &blob).unwrap();
        }

        let seqs = db.chunk_seqs(7).unwrap();
        assert_eq!(seqs, vec![1, 2, 3]);

        let chunks = db.chunks(7).unwrap();
        let joined: Vec<String> = chunks
            .iter()
            .map(|c| String::from_utf8(crypto::decrypt(&key, &c.blob).unwrap()).unwrap())
            .collect();
        assert_eq!(joined, vec!["alpha", "beta", "gamma"]);
    }

    #[test]
    fn delete_chunk_keeps_other_seqs() {
        let db = tmp_db();
        let key = crypto::derive_key(b"pw", &[9u8; crypto::SALT_LEN]).unwrap();
        make_user(&db, 8);
        for _ in 0..3 {
            let seq = db.next_seq(8).unwrap();
            let blob = crypto::encrypt(&key, b"x").unwrap();
            db.insert_chunk(8, seq, &blob).unwrap();
        }
        assert!(db.delete_chunk(8, 2).unwrap());
        assert_eq!(db.chunk_seqs(8).unwrap(), vec![1, 3]); // NOT renumbered
        assert!(!db.delete_chunk(8, 99).unwrap());
    }

    #[test]
    fn recent_messages_respects_limit_and_order() {
        let db = tmp_db();
        let key = crypto::derive_key(b"pw", &[9u8; crypto::SALT_LEN]).unwrap();
        make_user(&db, 9);
        for i in 0..5u32 {
            let blob = crypto::encrypt(&key, format!("m{i}").as_bytes()).unwrap();
            db.append_message(9, "user", &blob).unwrap();
        }
        let last2 = db.recent_messages(9, 2).unwrap();
        assert_eq!(last2.len(), 2);
        let texts: Vec<String> = last2
            .iter()
            .map(|m| String::from_utf8(crypto::decrypt(&key, &m.blob).unwrap()).unwrap())
            .collect();
        // oldest-first within the window: m3, m4
        assert_eq!(texts, vec!["m3", "m4"]);
    }

    #[test]
    fn clear_messages_wipes_history() {
        let db = tmp_db();
        let key = crypto::derive_key(b"pw", &[9u8; crypto::SALT_LEN]).unwrap();
        make_user(&db, 10);
        let blob = crypto::encrypt(&key, b"hi").unwrap();
        db.append_message(10, "user", &blob).unwrap();
        db.clear_messages(10).unwrap();
        assert!(db.recent_messages(10, 100).unwrap().is_empty());
    }
}
