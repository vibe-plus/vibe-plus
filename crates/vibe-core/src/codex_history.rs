//! Rewrite local Codex history metadata so conversations appear under one provider label.
//!
//! Scans `~/.codex` SQLite state DBs and rollout `.jsonl` session files, replacing
//! `model_provider` fields with the vibe+ provider id (`vibeplus` by default).

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use vibe_protocol::{CodexHistorySummary, CodexHistoryUnifyInput};

pub const DEFAULT_PROVIDER_ID: &str = "vibeplus";

pub fn default_codex_home() -> Result<PathBuf> {
    let dirs = directories::UserDirs::new().context("cannot find home directory")?;
    Ok(dirs.home_dir().join(".codex"))
}

/// Cheap fingerprint of Codex rollout files (count + newest mtime). Used to skip redundant unifies.
pub fn codex_home_change_stamp() -> Option<String> {
    let home = default_codex_home().ok()?;
    if !home.exists() {
        return None;
    }
    let mut file_count = 0u64;
    let mut max_mtime = 0u64;
    for dir in [home.join("sessions"), home.join("archived_sessions")] {
        stamp_rollout_dir(&dir, &mut file_count, &mut max_mtime).ok();
    }
    Some(format!("{file_count}:{max_mtime}"))
}

fn stamp_rollout_dir(dir: &Path, file_count: &mut u64, max_mtime: &mut u64) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            stamp_rollout_dir(&path, file_count, max_mtime)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            *file_count += 1;
            if let Ok(meta) = fs::metadata(&path) {
                if let Ok(modified) = meta.modified() {
                    if let Ok(secs) = modified.duration_since(std::time::UNIX_EPOCH) {
                        *max_mtime = (*max_mtime).max(secs.as_secs());
                    }
                }
            }
        }
    }
    Ok(())
}

/// Best-effort unify on startup / `vibe up`. Skips when `~/.codex` is missing; logs warnings on failure.
pub fn try_auto_unify() -> Option<CodexHistorySummary> {
    let home = match default_codex_home() {
        Ok(h) if h.exists() => h,
        _ => return None,
    };
    match unify(CodexHistoryUnifyInput {
        provider: DEFAULT_PROVIDER_ID.to_string(),
        from_providers: Vec::new(),
        apply: true,
        no_backup: false,
        codex_home: Some(home.to_string_lossy().to_string()),
    }) {
        Ok(summary) => Some(summary),
        Err(err) => {
            tracing::warn!(error = %err, "codex history auto-unify skipped");
            None
        }
    }
}

pub fn unify(input: CodexHistoryUnifyInput) -> Result<CodexHistorySummary> {
    let provider = if input.provider.trim().is_empty() {
        DEFAULT_PROVIDER_ID.to_string()
    } else {
        input.provider.trim().to_string()
    };
    let codex_home = input
        .codex_home
        .map(PathBuf::from)
        .map(Ok)
        .unwrap_or_else(default_codex_home)?;
    if !codex_home.exists() {
        anyhow::bail!("Codex home does not exist: {}", codex_home.display());
    }

    let filter = ProviderFilter::new(provider.clone(), input.from_providers.clone());
    let mut summary = CodexHistorySummary {
        codex_home: codex_home.to_string_lossy().to_string(),
        provider,
        from_providers: input.from_providers,
        applied: input.apply,
        sqlite_files_seen: 0,
        sqlite_files_changed: 0,
        sqlite_rows_changed: 0,
        rollout_files_seen: 0,
        rollout_files_changed: 0,
        rollout_fields_changed: 0,
        backups_created: 0,
    };

    for db_path in find_state_dbs(&codex_home)? {
        summary.sqlite_files_seen += 1;
        let changed = unify_sqlite(&db_path, &filter, input.apply, !input.no_backup)
            .with_context(|| format!("updating {}", db_path.display()))?;
        if changed > 0 {
            summary.sqlite_files_changed += 1;
            summary.sqlite_rows_changed += changed;
            if input.apply && !input.no_backup {
                summary.backups_created += 1;
            }
        }
    }

    for rollout_path in find_rollout_files(&codex_home)? {
        summary.rollout_files_seen += 1;
        let changed = unify_rollout_file(&rollout_path, &filter, input.apply, !input.no_backup)
            .with_context(|| format!("updating {}", rollout_path.display()))?;
        if changed > 0 {
            summary.rollout_files_changed += 1;
            summary.rollout_fields_changed += changed;
            if input.apply && !input.no_backup {
                summary.backups_created += 1;
            }
        }
    }

    Ok(summary)
}

struct ProviderFilter {
    target: String,
    from: Vec<String>,
}

impl ProviderFilter {
    fn new(target: String, from: Vec<String>) -> Self {
        Self { target, from }
    }

    fn should_replace(&self, current: &str) -> bool {
        if current == self.target {
            return false;
        }
        self.from.is_empty() || self.from.iter().any(|p| p == current)
    }
}

fn find_state_dbs(codex_home: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(codex_home)? {
        let path = entry?.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.starts_with("state") && (name.ends_with(".sqlite") || name.ends_with(".db")) {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}

fn find_rollout_files(codex_home: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for dir in [
        codex_home.join("sessions"),
        codex_home.join("archived_sessions"),
    ] {
        collect_jsonl_files(&dir, &mut paths)?;
    }
    paths.sort();
    Ok(paths)
}

fn collect_jsonl_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_jsonl_files(&path, out)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            out.push(path);
        }
    }
    Ok(())
}

fn unify_sqlite(path: &Path, filter: &ProviderFilter, apply: bool, backup: bool) -> Result<usize> {
    let conn = Connection::open(path)?;
    let has_threads: bool = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='threads'",
            [],
            |_| Ok(true),
        )
        .optional()?
        .unwrap_or(false);
    if !has_threads {
        return Ok(0);
    }

    let mut stmt = conn.prepare("SELECT count(*) FROM threads WHERE model_provider <> ?1")?;
    let changed: usize = if filter.from.is_empty() {
        stmt.query_row(params![filter.target], |row| row.get::<_, i64>(0))? as usize
    } else {
        let mut total = 0usize;
        for provider in &filter.from {
            if provider != &filter.target {
                total += conn.query_row(
                    "SELECT count(*) FROM threads WHERE model_provider = ?1",
                    params![provider],
                    |row| row.get::<_, i64>(0),
                )? as usize;
            }
        }
        total
    };

    if changed == 0 || !apply {
        return Ok(changed);
    }

    if backup {
        backup_file(path)?;
    }
    if filter.from.is_empty() {
        conn.execute(
            "UPDATE threads SET model_provider = ?1 WHERE model_provider <> ?1",
            params![filter.target],
        )?;
    } else {
        for provider in &filter.from {
            if provider != &filter.target {
                conn.execute(
                    "UPDATE threads SET model_provider = ?1 WHERE model_provider = ?2",
                    params![filter.target, provider],
                )?;
            }
        }
    }
    Ok(changed)
}

fn unify_rollout_file(
    path: &Path,
    filter: &ProviderFilter,
    apply: bool,
    backup: bool,
) -> Result<usize> {
    let file = fs::File::open(path)?;
    let reader = BufReader::new(file);
    let mut lines = Vec::new();
    let mut changed_fields = 0usize;

    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            lines.push(line);
            continue;
        }
        match serde_json::from_str::<Value>(&line) {
            Ok(mut value) => {
                let line_changes = replace_provider_fields(&mut value, filter);
                changed_fields += line_changes;
                if line_changes > 0 {
                    lines.push(serde_json::to_string(&value)?);
                } else {
                    lines.push(line);
                }
            }
            Err(_) => lines.push(line),
        }
    }

    if changed_fields == 0 || !apply {
        return Ok(changed_fields);
    }

    if backup {
        backup_file(path)?;
    }
    let tmp = path.with_extension("jsonl.tmp");
    {
        let mut out = fs::File::create(&tmp)?;
        for line in &lines {
            writeln!(out, "{line}")?;
        }
    }
    fs::rename(&tmp, path)?;
    Ok(changed_fields)
}

fn replace_provider_fields(value: &mut Value, filter: &ProviderFilter) -> usize {
    let mut changed = 0;
    changed += replace_string_path(value, &["payload", "model_provider"], filter);
    changed += replace_string_path(value, &["payload", "model_provider_id"], filter);
    changed += replace_string_path(
        value,
        &["payload", "turn_context", "model_provider"],
        filter,
    );
    changed
}

fn replace_string_path(value: &mut Value, path: &[&str], filter: &ProviderFilter) -> usize {
    let Some(slot) = value.pointer_mut(&format!("/{}", path.join("/"))) else {
        return 0;
    };
    let Some(current) = slot.as_str() else {
        return 0;
    };
    if !filter.should_replace(current) {
        return 0;
    }
    *slot = Value::String(filter.target.clone());
    1
}

fn backup_file(path: &Path) -> Result<()> {
    let ts = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .context("backup path has no file name")?;
    let backup_path = path.with_file_name(format!("{file_name}.{ts}.bak"));
    fs::copy(path, backup_path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_known_provider_fields() {
        let filter = ProviderFilter::new("vibeplus".into(), vec![]);
        let mut value = serde_json::json!({
            "type": "session_meta",
            "payload": {
                "model_provider": "openai",
                "model_provider_id": "anthropic",
                "turn_context": { "model_provider": "old" }
            }
        });

        assert_eq!(replace_provider_fields(&mut value, &filter), 3);
        assert_eq!(value["payload"]["model_provider"], "vibeplus");
        assert_eq!(value["payload"]["model_provider_id"], "vibeplus");
        assert_eq!(
            value["payload"]["turn_context"]["model_provider"],
            "vibeplus"
        );
    }

    #[test]
    fn respects_from_filter() {
        let filter = ProviderFilter::new("vibeplus".into(), vec!["openai".into()]);
        let mut value = serde_json::json!({
            "payload": {
                "model_provider": "openai",
                "model_provider_id": "anthropic"
            }
        });

        assert_eq!(replace_provider_fields(&mut value, &filter), 1);
        assert_eq!(value["payload"]["model_provider"], "vibeplus");
        assert_eq!(value["payload"]["model_provider_id"], "anthropic");
    }
}
