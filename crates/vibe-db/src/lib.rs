//! SQLite layer for vibe-plus.
//!
//! All DAOs are synchronous; HTTP handlers wrap calls in
//! `tokio::task::spawn_blocking`.

use anyhow::{Context, Result};
use rusqlite::Connection;
use rusqlite_migration::{Migrations, M};
use std::path::Path;
use std::sync::{Arc, Mutex};

pub mod dao;

pub use dao::*;

#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

impl Db {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        if let Some(parent) = path.as_ref().parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let mut conn = Connection::open(path.as_ref())
            .with_context(|| format!("opening sqlite at {}", path.as_ref().display()))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        Self::migrations().to_latest(&mut conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// In-memory db for tests.
    pub fn memory() -> Result<Self> {
        let mut conn = Connection::open_in_memory()?;
        Self::migrations().to_latest(&mut conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    fn migrations() -> Migrations<'static> {
        Migrations::new(vec![
            M::up(include_str!("../migrations/001_init.sql")),
            M::up(include_str!("../migrations/002_health.sql")),
            M::up(include_str!("../migrations/003_credentials.sql")),
            M::up(include_str!("../migrations/004_oauth_credentials.sql")),
            M::up(include_str!("../migrations/005_request_logs_extended.sql")),
            M::up(include_str!("../migrations/006_plan_fingerprint.sql")),
            M::up(include_str!("../migrations/007_request_log_bodies.sql")),
            M::up(include_str!("../migrations/008_client_response_body.sql")),
            M::up(include_str!("../migrations/009_oauth_identity_cache.sql")),
            M::up(include_str!(
                "../migrations/010_request_log_transport_headers.sql"
            )),
            M::up(include_str!(
                "../migrations/011_request_log_stream_trace.sql"
            )),
            M::up(include_str!("../migrations/012_upstream_attempt_logs.sql")),
            M::up(include_str!(
                "../migrations/013_upstream_attempt_log_detail.sql"
            )),
            M::up(include_str!("../migrations/014_app_logs.sql")),
            M::up(include_str!("../migrations/015_provider_protocols.sql")),
            M::up(include_str!(
                "../migrations/016_credential_models_and_balance.sql"
            )),
        ])
    }

    pub fn with<R>(&self, f: impl FnOnce(&Connection) -> Result<R>) -> Result<R> {
        let conn = self.conn.lock().unwrap();
        f(&conn)
    }

    pub fn with_mut<R>(&self, f: impl FnOnce(&mut Connection) -> Result<R>) -> Result<R> {
        let mut conn = self.conn.lock().unwrap();
        f(&mut conn)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_apply_to_empty_db() {
        let db = Db::memory().expect("memory db");
        db.with(|c| {
            let count: i64 = c.query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table'",
                [],
                |r| r.get(0),
            )?;
            // providers, routes, request_logs, model_pricing + sqlite_sequence is internal
            assert!(count >= 4, "expected >=4 tables, got {count}");
            Ok(())
        })
        .unwrap();
    }
}
