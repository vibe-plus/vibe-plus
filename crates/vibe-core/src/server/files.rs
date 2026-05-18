//! Sandboxed read/write of the user's Codex (`~/.codex/`) and Claude (`~/.claude/`)
//! configuration directories. Mounted at `/_vp/files/:scope/*`.

use super::*;

#[derive(Debug, Deserialize)]
pub(super) struct FilePathQuery {
    path: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct FileWriteInput {
    path: String,
    raw_text: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct DirCreateInput {
    path: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct FileMoveInput {
    from: String,
    to: String,
    overwrite: Option<bool>,
}

#[derive(Debug, Serialize)]
pub(super) struct FileEntry {
    name: String,
    path: String,
    kind: String,
    size: Option<u64>,
    mtime_ms: Option<i64>,
}

#[derive(Debug, Serialize)]
pub(super) struct FileListResponse {
    scope: String,
    root: String,
    path: String,
    abs_path: String,
    entries: Vec<FileEntry>,
}

#[derive(Debug, Serialize)]
pub(super) struct FileResponse {
    scope: String,
    root: String,
    path: String,
    abs_path: String,
    exists: bool,
    mtime_ms: Option<i64>,
    raw_text: String,
}

fn scope_root(scope: &str) -> anyhow::Result<PathBuf> {
    match scope {
        "codex" => Ok(crate::paths::codex_config_path()?
            .parent()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))),
        "claude" => crate::paths::claude_config_dir(),
        "agents" | "agent" => Ok(crate::paths::real_home_dir()?.join(".agents")),
        "ccswitch" | "cc-switch" => Ok(crate::paths::real_home_dir()?.join(".cc-switch")),
        _ => {
            anyhow::bail!("unsupported scope: {scope}. Supported: codex, claude, agents, ccswitch")
        }
    }
}

fn file_mtime_ms(meta: &std::fs::Metadata) -> Option<i64> {
    let modified = meta.modified().ok()?;
    let dur = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some(dur.as_millis() as i64)
}

fn resolve_within(root: &std::path::Path, rel: &str) -> anyhow::Result<PathBuf> {
    if rel.contains('\0') {
        anyhow::bail!("invalid path");
    }
    let rel_path = std::path::Path::new(rel);
    if rel_path.is_absolute() {
        anyhow::bail!("absolute paths are not allowed");
    }
    let mut out = root.to_path_buf();
    for component in rel_path.components() {
        match component {
            std::path::Component::Normal(part) => out.push(part),
            std::path::Component::CurDir => {}
            _ => anyhow::bail!("path traversal is not allowed"),
        }
    }
    ensure_within(root, &out)?;
    Ok(out)
}

fn ensure_within(root: &std::path::Path, path: &std::path::Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(root)?;
    let root = root.canonicalize()?;
    let canonical = if path.exists() {
        path.canonicalize()?
    } else {
        let mut ancestor = path.parent();
        let mut found = None;
        while let Some(candidate) = ancestor {
            if candidate.exists() {
                found = Some(candidate.canonicalize()?);
                break;
            }
            ancestor = candidate.parent();
        }
        found.unwrap_or_else(|| root.clone())
    };
    if !canonical.starts_with(&root) {
        anyhow::bail!("path resolves outside scope root");
    }
    Ok(())
}

fn relative(root: &std::path::Path, path: &std::path::Path) -> String {
    path.strip_prefix(root)
        .ok()
        .map(|p| p.to_string_lossy().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| ".".into())
}

pub(super) async fn list_files(
    Path(scope): Path<String>,
    Query(q): Query<FilePathQuery>,
) -> Result<Json<FileListResponse>, AppError> {
    let root = scope_root(&scope)?;
    let rel = q.path.unwrap_or_default();
    let path = resolve_within(&root, &rel)?;
    let (mtime_path, entries) = tokio::task::spawn_blocking({
        let root = root.clone();
        let path = path.clone();
        move || -> anyhow::Result<(String, Vec<FileEntry>)> {
            if !path.exists() {
                return Ok((relative(&root, &path), Vec::new()));
            }
            if !path.is_dir() {
                anyhow::bail!("path is not a directory");
            }
            let mut entries = Vec::new();
            for entry in std::fs::read_dir(&path)? {
                let entry = entry?;
                let p = entry.path();
                let meta = entry.metadata()?;
                entries.push(FileEntry {
                    name: entry.file_name().to_string_lossy().to_string(),
                    path: relative(&root, &p),
                    kind: if meta.is_dir() { "dir" } else { "file" }.into(),
                    size: meta.is_file().then_some(meta.len()),
                    mtime_ms: file_mtime_ms(&meta),
                });
            }
            entries.sort_by(|a, b| a.kind.cmp(&b.kind).then_with(|| a.name.cmp(&b.name)));
            Ok((relative(&root, &path), entries))
        }
    })
    .await??;
    Ok(Json(FileListResponse {
        scope,
        root: root.display().to_string(),
        path: mtime_path,
        abs_path: path.display().to_string(),
        entries,
    }))
}

pub(super) async fn read_file(
    Path(scope): Path<String>,
    Query(q): Query<FilePathQuery>,
) -> Result<Json<FileResponse>, AppError> {
    let root = scope_root(&scope)?;
    let rel = q.path.ok_or_else(|| anyhow::anyhow!("missing path"))?;
    let path = resolve_within(&root, &rel)?;
    let out = tokio::task::spawn_blocking({
        let root = root.clone();
        let path = path.clone();
        let scope = scope.clone();
        move || -> anyhow::Result<FileResponse> {
            if !path.exists() {
                return Ok(FileResponse {
                    scope,
                    root: root.display().to_string(),
                    path: relative(&root, &path),
                    abs_path: path.display().to_string(),
                    exists: false,
                    mtime_ms: None,
                    raw_text: String::new(),
                });
            }
            if !path.is_file() {
                anyhow::bail!("path is not a file");
            }
            let meta = std::fs::metadata(&path)?;
            let raw_text = std::fs::read_to_string(&path)?;
            Ok(FileResponse {
                scope,
                root: root.display().to_string(),
                path: relative(&root, &path),
                abs_path: path.display().to_string(),
                exists: true,
                mtime_ms: file_mtime_ms(&meta),
                raw_text,
            })
        }
    })
    .await??;
    Ok(Json(out))
}

pub(super) async fn write_file(
    Path(scope): Path<String>,
    Json(input): Json<FileWriteInput>,
) -> Result<Json<FileResponse>, AppError> {
    let root = scope_root(&scope)?;
    let path = resolve_within(&root, &input.path)?;
    let out = tokio::task::spawn_blocking({
        let root = root.clone();
        let path = path.clone();
        let raw = input.raw_text;
        let scope = scope.clone();
        move || -> anyhow::Result<FileResponse> {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, raw)?;
            let meta = std::fs::metadata(&path)?;
            let raw_text = std::fs::read_to_string(&path)?;
            Ok(FileResponse {
                scope,
                root: root.display().to_string(),
                path: relative(&root, &path),
                abs_path: path.display().to_string(),
                exists: true,
                mtime_ms: file_mtime_ms(&meta),
                raw_text,
            })
        }
    })
    .await??;
    Ok(Json(out))
}

pub(super) async fn delete_file(
    Path(scope): Path<String>,
    Query(q): Query<FilePathQuery>,
) -> Result<StatusCode, AppError> {
    let rel = q.path.ok_or_else(|| anyhow::anyhow!("missing path"))?;
    if rel.trim().is_empty() || rel.trim() == "." {
        return Err(anyhow::anyhow!("refusing to delete scope root").into());
    }
    let root = scope_root(&scope)?;
    let path = resolve_within(&root, &rel)?;
    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        if path.is_dir() {
            std::fs::remove_dir_all(path)?;
        } else if path.exists() {
            std::fs::remove_file(path)?;
        }
        Ok(())
    })
    .await??;
    Ok(StatusCode::NO_CONTENT)
}

pub(super) async fn create_dir(
    Path(scope): Path<String>,
    Json(input): Json<DirCreateInput>,
) -> Result<Json<FileListResponse>, AppError> {
    let root = scope_root(&scope)?;
    let path = resolve_within(&root, &input.path)?;
    tokio::task::spawn_blocking({
        let path = path.clone();
        move || std::fs::create_dir_all(path)
    })
    .await??;
    list_files(
        Path(scope),
        Query(FilePathQuery {
            path: Some(input.path),
        }),
    )
    .await
}

pub(super) async fn move_file(
    Path(scope): Path<String>,
    Json(input): Json<FileMoveInput>,
) -> Result<Json<FileResponse>, AppError> {
    if input.from.trim().is_empty() || input.from.trim() == "." {
        return Err(anyhow::anyhow!("refusing to move scope root").into());
    }
    if input.to.trim().is_empty() || input.to.trim() == "." {
        return Err(anyhow::anyhow!("destination path is required").into());
    }
    let root = scope_root(&scope)?;
    let from = resolve_within(&root, &input.from)?;
    let to = resolve_within(&root, &input.to)?;
    let out = tokio::task::spawn_blocking({
        let root = root.clone();
        let overwrite = input.overwrite.unwrap_or(false);
        let scope = scope.clone();
        move || -> anyhow::Result<FileResponse> {
            if !from.exists() {
                anyhow::bail!("source path does not exist");
            }
            if to.exists() {
                if !overwrite {
                    anyhow::bail!("destination already exists");
                }
                if to.is_dir() {
                    std::fs::remove_dir_all(&to)?;
                } else {
                    std::fs::remove_file(&to)?;
                }
            }
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::rename(&from, &to)?;
            let (raw_text, mtime_ms) = if to.is_file() {
                let meta = std::fs::metadata(&to)?;
                (std::fs::read_to_string(&to)?, file_mtime_ms(&meta))
            } else {
                (
                    String::new(),
                    std::fs::metadata(&to).ok().as_ref().and_then(file_mtime_ms),
                )
            };
            Ok(FileResponse {
                scope,
                root: root.display().to_string(),
                path: relative(&root, &to),
                abs_path: to.display().to_string(),
                exists: true,
                mtime_ms,
                raw_text,
            })
        }
    })
    .await??;
    Ok(Json(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_within_rejects_path_escape_attempts() {
        let root = tempfile::tempdir().expect("root");
        for rel in ["..", "../secret", "a/../../secret", "/tmp/secret", "a\0b"] {
            assert!(
                resolve_within(root.path(), rel).is_err(),
                "path escape should be rejected: {rel:?}"
            );
        }
    }

    #[test]
    fn resolve_within_accepts_normal_nested_paths() {
        let root = tempfile::tempdir().expect("root");
        let resolved = resolve_within(root.path(), "profiles/default.json").expect("resolved");
        assert!(resolved.starts_with(root.path()));
        assert!(resolved.ends_with("profiles/default.json"));
    }
}
