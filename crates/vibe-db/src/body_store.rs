//! Filesystem-backed storage for large request/response bodies.
//!
//! SQLite rows keep small `body ref` strings (currently `file:<relative path>`),
//! while the raw lossy-UTF8 body lives below `~/.vibe/bodies` (or the caller's
//! configured directory). Old inline DB body fields remain readable as fallback.

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

const REF_PREFIX: &str = "file:";
const BODY_EXT: &str = "body";

#[derive(Debug, Clone)]
pub struct BodyStore {
    root: PathBuf,
}

#[derive(Debug, Clone, Default)]
pub struct BodyPruneOptions {
    pub max_age_secs: Option<i64>,
    pub max_files: Option<usize>,
    pub max_bytes: Option<u64>,
}

#[derive(Debug, Clone, Default)]
pub struct BodyPruneStats {
    pub files_deleted: usize,
    pub bytes_deleted: u64,
    pub bytes_remaining: u64,
    pub files_remaining: usize,
}

#[derive(Debug, Clone)]
struct BodyFileInfo {
    path: PathBuf,
    modified_secs: i64,
    len: u64,
}

impl BodyStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn write_text(&self, kind: &str, request_id: &str, text: &str) -> Result<String> {
        fs::create_dir_all(&self.root)?;
        let safe_kind = sanitize_segment(kind);
        let safe_request_id = sanitize_segment(request_id);
        let filename = format!("{}-{}.{}", safe_kind, Uuid::new_v4().simple(), BODY_EXT);
        let rel = PathBuf::from(day_bucket())
            .join(safe_request_id)
            .join(filename);
        let path = self.root.join(&rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, text.as_bytes())
            .with_context(|| format!("writing body temp file {}", tmp.display()))?;
        fs::rename(&tmp, &path)
            .with_context(|| format!("committing body file {}", path.display()))?;
        Ok(format!(
            "{REF_PREFIX}{}",
            rel.to_string_lossy().replace('\\', "/")
        ))
    }

    pub fn read_text(&self, body_ref: &str) -> Result<Option<String>> {
        let Some(rel) = body_ref.strip_prefix(REF_PREFIX) else {
            return Ok(None);
        };
        let rel = Path::new(rel);
        if rel
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            anyhow::bail!("invalid body ref path: {body_ref}");
        }
        let path = self.root.join(rel);
        if !path.exists() {
            return Ok(None);
        }
        let bytes =
            fs::read(&path).with_context(|| format!("reading body file {}", path.display()))?;
        Ok(Some(String::from_utf8_lossy(&bytes).into_owned()))
    }

    pub fn prune(&self, opts: BodyPruneOptions) -> Result<BodyPruneStats> {
        let mut files = Vec::new();
        collect_body_files(&self.root, &mut files)?;
        files.sort_by_key(|f| f.modified_secs);

        let now = crate::dao::now_secs();
        let mut total_bytes: u64 = files.iter().map(|f| f.len).sum();
        let mut remaining_files = files.len();
        let mut deleted = vec![false; files.len()];
        let mut stats = BodyPruneStats {
            bytes_remaining: total_bytes,
            files_remaining: remaining_files,
            ..Default::default()
        };

        for (idx, info) in files.iter().enumerate() {
            if let Some(max_age) = opts.max_age_secs {
                if max_age >= 0 && info.modified_secs < now.saturating_sub(max_age) {
                    delete_body_file(
                        info,
                        &mut deleted[idx],
                        &mut total_bytes,
                        &mut remaining_files,
                        &mut stats,
                    );
                }
            }
        }

        if let Some(max_files) = opts.max_files {
            for (idx, info) in files.iter().enumerate() {
                if remaining_files <= max_files {
                    break;
                }
                delete_body_file(
                    info,
                    &mut deleted[idx],
                    &mut total_bytes,
                    &mut remaining_files,
                    &mut stats,
                );
            }
        }

        if let Some(max_bytes) = opts.max_bytes {
            for (idx, info) in files.iter().enumerate() {
                if total_bytes <= max_bytes {
                    break;
                }
                delete_body_file(
                    info,
                    &mut deleted[idx],
                    &mut total_bytes,
                    &mut remaining_files,
                    &mut stats,
                );
            }
        }

        stats.bytes_remaining = total_bytes;
        stats.files_remaining = remaining_files;
        prune_empty_dirs(&self.root, &self.root).ok();
        Ok(stats)
    }
}

fn delete_body_file(
    info: &BodyFileInfo,
    already_deleted: &mut bool,
    total_bytes: &mut u64,
    remaining_files: &mut usize,
    stats: &mut BodyPruneStats,
) {
    if *already_deleted {
        return;
    }
    if fs::remove_file(&info.path).is_ok() {
        *already_deleted = true;
        *total_bytes = total_bytes.saturating_sub(info.len);
        *remaining_files = remaining_files.saturating_sub(1);
        stats.files_deleted += 1;
        stats.bytes_deleted = stats.bytes_deleted.saturating_add(info.len);
    }
}

fn collect_body_files(root: &Path, out: &mut Vec<BodyFileInfo>) -> Result<()> {
    if !root.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        let ft = entry.file_type()?;
        if ft.is_dir() {
            collect_body_files(&path, out)?;
        } else if ft.is_file() {
            let meta = entry.metadata()?;
            let modified_secs = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            out.push(BodyFileInfo {
                path,
                modified_secs,
                len: meta.len(),
            });
        }
    }
    Ok(())
}

fn prune_empty_dirs(root: &Path, current: &Path) -> Result<bool> {
    if !current.exists() {
        return Ok(true);
    }
    let mut empty = true;
    for entry in fs::read_dir(current)? {
        let path = entry?.path();
        if path.is_dir() {
            if !prune_empty_dirs(root, &path)? {
                empty = false;
            }
        } else {
            empty = false;
        }
    }
    if empty && current != root {
        fs::remove_dir(current).ok();
    }
    Ok(empty)
}

fn sanitize_segment(input: &str) -> String {
    let out: String = input
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if out.is_empty() {
        "unknown".into()
    } else {
        out
    }
}

fn day_bucket() -> String {
    let ts = crate::dao::now_secs();
    chrono::DateTime::<chrono::Utc>::from_timestamp(ts, 0)
        .map(|dt| dt.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "1970-01-01".to_string())
}
