//! SQLite layer for vibe-plus.
//!
//! All DAOs are synchronous; HTTP handlers wrap calls in
//! `tokio::task::spawn_blocking`.

use anyhow::{Context, Result};
use rusqlite::Connection;
use rusqlite_migration::{Migrations, M};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub mod body_store;
pub mod dao;

pub use body_store::*;
pub use dao::*;

#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
    short_conn: Arc<Mutex<Connection>>,
    body_store: Option<body_store::BodyStore>,
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

        let short_path = short_logs_path_for_db(path.as_ref());
        let mut short_conn = Connection::open(&short_path)
            .with_context(|| format!("opening short-log sqlite at {}", short_path.display()))?;
        short_conn.pragma_update(None, "journal_mode", "WAL")?;
        short_conn.pragma_update(None, "foreign_keys", "ON")?;
        Self::migrations().to_latest(&mut short_conn)?;
        migrate_short_logs_from_main(&mut conn, &mut short_conn, path.as_ref())?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            short_conn: Arc::new(Mutex::new(short_conn)),
            body_store: default_body_store_for_db(path.as_ref()),
        })
    }

    /// In-memory db for tests.
    pub fn memory() -> Result<Self> {
        let mut conn = Connection::open_in_memory()?;
        Self::migrations().to_latest(&mut conn)?;
        let mut short_conn = Connection::open_in_memory()?;
        Self::migrations().to_latest(&mut short_conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            short_conn: Arc::new(Mutex::new(short_conn)),
            body_store: None,
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
            M::up(include_str!(
                "../migrations/017_upstream_provider_support.sql"
            )),
            M::up(include_str!("../migrations/018_route_forward_strategy.sql")),
            M::up(include_str!("../migrations/019_drop_route_strategy.sql")),
            M::up(include_str!("../migrations/020_drop_routes_table.sql")),
            M::up(include_str!("../migrations/021_app_logs_events.sql")),
            M::up(include_str!(
                "../migrations/022_credential_disabled_reason.sql"
            )),
            M::up(include_str!("../migrations/023_upstream_attempt_wave.sql")),
            M::up(include_str!("../migrations/024_usage_daily_rollups.sql")),
            M::up(include_str!(
                "../migrations/025_body_refs_and_short_log_prune.sql"
            )),
            M::up(include_str!(
                "../migrations/026_request_log_cost_micros.sql"
            )),
        ])
    }

    pub fn body_store(&self) -> Option<&body_store::BodyStore> {
        self.body_store.as_ref()
    }

    pub fn with_body_store(mut self, root: impl Into<PathBuf>) -> Self {
        self.body_store = Some(body_store::BodyStore::new(root));
        self
    }

    pub fn with<R>(&self, f: impl FnOnce(&Connection) -> Result<R>) -> Result<R> {
        let conn = self.conn.lock().unwrap();
        f(&conn)
    }

    pub fn with_mut<R>(&self, f: impl FnOnce(&mut Connection) -> Result<R>) -> Result<R> {
        let mut conn = self.conn.lock().unwrap();
        f(&mut conn)
    }

    pub fn with_short<R>(&self, f: impl FnOnce(&Connection) -> Result<R>) -> Result<R> {
        let conn = self.short_conn.lock().unwrap();
        f(&conn)
    }

    pub fn with_short_mut<R>(&self, f: impl FnOnce(&mut Connection) -> Result<R>) -> Result<R> {
        let mut conn = self.short_conn.lock().unwrap();
        f(&mut conn)
    }
}

fn migrate_short_logs_from_main(
    main_conn: &mut Connection,
    short_conn: &mut Connection,
    main_path: &Path,
) -> Result<()> {
    let main_path = main_path.to_string_lossy().to_string();
    short_conn.execute("ATTACH DATABASE ?1 AS main_db", [&main_path])?;
    let copy_result = short_conn.execute_batch(
        "INSERT OR IGNORE INTO request_logs SELECT * FROM main_db.request_logs;
         INSERT OR IGNORE INTO upstream_attempt_logs SELECT * FROM main_db.upstream_attempt_logs;
         INSERT OR IGNORE INTO app_logs (id, ts, level, category, message, detail, event_type, payload_json)
            SELECT id, ts, level, category, message, detail, event_type, payload_json
            FROM main_db.app_logs;",
    );
    short_conn.execute_batch("DETACH DATABASE main_db").ok();
    copy_result?;

    // Raw logs are now owned by the short-retention DB. Keep long-lived business
    // metadata and daily rollups in the main DB, but remove duplicated heavy rows
    // so future VACUUM/checkpoints can shrink the hot business database.
    main_conn.execute_batch(
        "DELETE FROM upstream_attempt_logs;
         DELETE FROM app_logs;
         DELETE FROM request_logs;",
    )?;
    Ok(())
}

fn short_logs_path_for_db(db_path: &Path) -> PathBuf {
    let file_name = db_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("vibe.db");
    let short_name = if let Some(stem) = file_name.strip_suffix(".db") {
        format!("{stem}-short-logs.db")
    } else {
        format!("{file_name}-short-logs.db")
    };
    db_path
        .parent()
        .map(|p| p.join(&short_name))
        .unwrap_or_else(|| PathBuf::from(short_name))
}

fn default_body_store_for_db(db_path: &Path) -> Option<body_store::BodyStore> {
    db_path
        .parent()
        .map(|p| body_store::BodyStore::new(p.join("bodies")))
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
