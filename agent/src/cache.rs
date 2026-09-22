use std::path::Path;
use std::sync::Mutex;

use rusqlite::{params, Connection};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CacheError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

pub struct LocalCache {
    conn: Mutex<Connection>,
}

impl LocalCache {
    pub fn open(path: &Path) -> Result<Self, CacheError> {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn migrate(&self) -> Result<(), CacheError> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS collector_hashes (
                collector_name TEXT PRIMARY KEY,
                data_hash TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS pending_uploads (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                payload_type TEXT NOT NULL,
                payload_json TEXT NOT NULL,
                created_at TEXT NOT NULL,
                attempts INTEGER NOT NULL DEFAULT 0
            );
            "#,
        )?;
        Ok(())
    }

    pub fn get_hash(&self, collector: &str) -> Result<Option<String>, CacheError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare("SELECT data_hash FROM collector_hashes WHERE collector_name = ?1")?;
        let mut rows = stmt.query([collector])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }

    pub fn set_hash(&self, collector: &str, hash: &str) -> Result<(), CacheError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO collector_hashes (collector_name, data_hash, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(collector_name) DO UPDATE SET data_hash=excluded.data_hash, updated_at=excluded.updated_at",
            params![collector, hash, chrono::Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn queue_upload(&self, payload_type: &str, payload_json: &str) -> Result<(), CacheError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO pending_uploads (payload_type, payload_json, created_at) VALUES (?1, ?2, ?3)",
            params![payload_type, payload_json, chrono::Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn drain_pending(&self) -> Result<Vec<(i64, String, String)>, CacheError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, payload_type, payload_json FROM pending_uploads ORDER BY id LIMIT 50",
        )?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
        Ok(rows.filter_map(Result::ok).collect())
    }

    pub fn remove_pending(&self, id: i64) -> Result<(), CacheError> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM pending_uploads WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn clear_hashes(&self) -> Result<(), CacheError> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM collector_hashes", [])?;
        Ok(())
    }

    pub fn increment_attempts(&self, id: i64) -> Result<(), CacheError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE pending_uploads SET attempts = attempts + 1 WHERE id = ?1",
            [id],
        )?;
        Ok(())
    }
}
