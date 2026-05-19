//! One-shot DB maintenance helpers: backfill old inline bodies into the
//! filesystem body store (gzip-compressed) and reclaim space with VACUUM.
//!
//! Safe by construction: every row is only touched if it has an inline body
//! AND no body_ref — i.e. the operation is idempotent and never overwrites
//! externalised data.

use anyhow::Result;
use rusqlite::params;

use crate::Db;

/// Counters describing how much un-externalised body data is still inline.
#[derive(Debug, Default, Clone, Copy)]
pub struct SlimStats {
    pub request_log_inline_rows: i64,
    pub request_log_inline_bytes: i64,
    pub upstream_attempt_inline_rows: i64,
    pub upstream_attempt_inline_bytes: i64,
}

impl SlimStats {
    pub fn inline_rows(&self) -> i64 {
        self.request_log_inline_rows + self.upstream_attempt_inline_rows
    }
    pub fn inline_bytes(&self) -> i64 {
        self.request_log_inline_bytes + self.upstream_attempt_inline_bytes
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SlimReport {
    pub request_log_rows_migrated: i64,
    pub upstream_attempt_rows_migrated: i64,
    pub bytes_externalised: i64,
    pub bytes_reclaimed: i64,
}

impl Db {
    /// Count rows that still have inline body columns set with no body_ref.
    pub fn slim_stats(&self) -> Result<SlimStats> {
        self.with_short(|c| {
            let (rl_rows, rl_bytes): (i64, i64) = c.query_row(
                "SELECT COUNT(*),
                        COALESCE(SUM(
                            COALESCE(length(request_body),0) +
                            COALESCE(length(response_body),0) +
                            COALESCE(length(client_response_body),0)
                        ),0)
                 FROM request_logs
                 WHERE (request_body IS NOT NULL AND request_body_ref IS NULL)
                    OR (response_body IS NOT NULL AND response_body_ref IS NULL)
                    OR (client_response_body IS NOT NULL AND client_response_body_ref IS NULL)",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            let (ua_rows, ua_bytes): (i64, i64) = c.query_row(
                "SELECT COUNT(*),
                        COALESCE(SUM(
                            COALESCE(length(request_body),0) +
                            COALESCE(length(response_body),0)
                        ),0)
                 FROM upstream_attempt_logs
                 WHERE (request_body IS NOT NULL AND request_body_ref IS NULL)
                    OR (response_body IS NOT NULL AND response_body_ref IS NULL)",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            Ok(SlimStats {
                request_log_inline_rows: rl_rows,
                request_log_inline_bytes: rl_bytes,
                upstream_attempt_inline_rows: ua_rows,
                upstream_attempt_inline_bytes: ua_bytes,
            })
        })
    }

    /// Move inline bodies into the configured body store (gzip on write) and
    /// null the inline columns. Returns counts.
    ///
    /// Operates row-by-row in its own short transactions so a crash mid-run
    /// leaves a consistent DB (each row either fully migrated or untouched).
    /// Skips rows that have no body_store configured (tests).
    pub fn slim_backfill(&self) -> Result<SlimReport> {
        let Some(store) = self.body_store().cloned() else {
            return Ok(SlimReport::default());
        };
        let mut report = SlimReport::default();

        // request_logs
        let rows = self.with_short(|c| {
            let mut stmt = c.prepare(
                "SELECT id, request_body, response_body, client_response_body,
                        request_body_ref, response_body_ref, client_response_body_ref
                 FROM request_logs
                 WHERE (request_body IS NOT NULL AND request_body_ref IS NULL)
                    OR (response_body IS NOT NULL AND response_body_ref IS NULL)
                    OR (client_response_body IS NOT NULL AND client_response_body_ref IS NULL)",
            )?;
            let mapped = stmt.query_map([], |r| {
                Ok::<
                    (
                        String,
                        Option<String>,
                        Option<String>,
                        Option<String>,
                        Option<String>,
                        Option<String>,
                        Option<String>,
                    ),
                    rusqlite::Error,
                >((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                ))
            })?;
            let mut out = Vec::new();
            for row in mapped {
                out.push(row?);
            }
            Ok(out)
        })?;

        for (id, req_b, resp_b, client_b, req_ref, resp_ref, client_ref) in rows {
            let new_req = externalise_one(
                &store,
                "request",
                &id,
                req_ref.as_deref(),
                req_b.as_deref(),
                &mut report.bytes_externalised,
            )?;
            let new_resp = externalise_one(
                &store,
                "response",
                &id,
                resp_ref.as_deref(),
                resp_b.as_deref(),
                &mut report.bytes_externalised,
            )?;
            let new_client = externalise_one(
                &store,
                "client-response",
                &id,
                client_ref.as_deref(),
                client_b.as_deref(),
                &mut report.bytes_externalised,
            )?;
            self.with_short_mut(|c| {
                c.execute(
                    "UPDATE request_logs SET
                        request_body = NULL, response_body = NULL, client_response_body = NULL,
                        request_body_ref = COALESCE(?2, request_body_ref),
                        response_body_ref = COALESCE(?3, response_body_ref),
                        client_response_body_ref = COALESCE(?4, client_response_body_ref)
                     WHERE id = ?1",
                    params![id, new_req, new_resp, new_client],
                )?;
                Ok(())
            })?;
            report.request_log_rows_migrated += 1;
        }

        // upstream_attempt_logs
        let rows = self.with_short(|c| {
            let mut stmt = c.prepare(
                "SELECT attempt_id, request_body, response_body, request_body_ref, response_body_ref
                 FROM upstream_attempt_logs
                 WHERE (request_body IS NOT NULL AND request_body_ref IS NULL)
                    OR (response_body IS NOT NULL AND response_body_ref IS NULL)",
            )?;
            let mapped = stmt.query_map([], |r| {
                Ok::<
                    (
                        String,
                        Option<String>,
                        Option<String>,
                        Option<String>,
                        Option<String>,
                    ),
                    rusqlite::Error,
                >((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })?;
            let mut out = Vec::new();
            for row in mapped {
                out.push(row?);
            }
            Ok(out)
        })?;

        for (attempt_id, req_b, resp_b, req_ref, resp_ref) in rows {
            let new_req = externalise_one(
                &store,
                "attempt-request",
                &attempt_id,
                req_ref.as_deref(),
                req_b.as_deref(),
                &mut report.bytes_externalised,
            )?;
            let new_resp = externalise_one(
                &store,
                "attempt-response",
                &attempt_id,
                resp_ref.as_deref(),
                resp_b.as_deref(),
                &mut report.bytes_externalised,
            )?;
            self.with_short_mut(|c| {
                c.execute(
                    "UPDATE upstream_attempt_logs SET
                        request_body = NULL, response_body = NULL,
                        request_body_ref = COALESCE(?2, request_body_ref),
                        response_body_ref = COALESCE(?3, response_body_ref)
                     WHERE attempt_id = ?1",
                    params![attempt_id, new_req, new_resp],
                )?;
                Ok(())
            })?;
            report.upstream_attempt_rows_migrated += 1;
        }

        Ok(report)
    }

    /// Run `VACUUM` on both connections. Reclaims free pages and physically
    /// shrinks the DB file. Does not touch row contents.
    pub fn vacuum(&self) -> Result<()> {
        self.with_mut(|c| {
            c.execute_batch("VACUUM")?;
            Ok(())
        })?;
        self.with_short_mut(|c| {
            c.execute_batch("VACUUM")?;
            Ok(())
        })?;
        Ok(())
    }
}

fn externalise_one(
    store: &crate::body_store::BodyStore,
    kind: &str,
    owner_id: &str,
    existing_ref: Option<&str>,
    inline: Option<&str>,
    bytes_out: &mut i64,
) -> Result<Option<String>> {
    // Only externalise when the row has an inline body AND no ref yet.
    if existing_ref.is_some() {
        return Ok(None);
    }
    let Some(text) = inline else { return Ok(None) };
    if text.is_empty() {
        return Ok(None);
    }
    *bytes_out += text.len() as i64;
    let new_ref = store.write_text(kind, owner_id, text)?;
    Ok(Some(new_ref))
}
