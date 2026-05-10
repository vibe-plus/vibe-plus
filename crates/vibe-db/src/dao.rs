//! DAOs for the four tables. Synchronous; callers use `spawn_blocking`.

use crate::Db;
use anyhow::{Context, Result};
use rusqlite::{params, OptionalExtension};
use vibe_protocol::*;

fn now_secs() -> i64 {
    chrono::Utc::now().timestamp()
}

fn provider_kind_to_str(k: ProviderKind) -> &'static str {
    match k {
        ProviderKind::Anthropic => "anthropic",
        ProviderKind::OpenaiChat => "openai-chat",
        ProviderKind::OpenaiResponses => "openai-responses",
        ProviderKind::GeminiNative => "gemini-native",
    }
}

fn provider_kind_from_str(s: &str) -> Result<ProviderKind> {
    Ok(match s {
        "anthropic" => ProviderKind::Anthropic,
        // "openai-compat" is the legacy DB value — map to the renamed variant.
        "openai-chat" | "openai-compat" => ProviderKind::OpenaiChat,
        "openai-responses" => ProviderKind::OpenaiResponses,
        "gemini-native" => ProviderKind::GeminiNative,
        other => anyhow::bail!("unknown provider kind: {other}"),
    })
}

fn route_tier_to_str(t: RouteTier) -> &'static str {
    match t {
        RouteTier::High => "high",
        RouteTier::Low => "low",
        RouteTier::Default => "default",
    }
}

fn _route_tier_from_str(s: &str) -> Result<RouteTier> {
    Ok(match s {
        "high" => RouteTier::High,
        "low" => RouteTier::Low,
        "default" => RouteTier::Default,
        other => anyhow::bail!("unknown route tier: {other}"),
    })
}

impl Db {
    // --- providers ----------------------------------------------------------

    pub fn provider_list(&self) -> Result<Vec<Provider>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT id, name, kind, base_url, auth_ref, enabled, priority,
                        model_aliases_json, created_at, updated_at
                 FROM providers ORDER BY priority ASC, created_at ASC",
            )?;
            let rows = stmt.query_map([], row_to_provider)?;
            let mut out = Vec::new();
            for r in rows {
                out.push(r?);
            }
            Ok(out)
        })
    }

    pub fn provider_get(&self, id: &str) -> Result<Option<Provider>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT id, name, kind, base_url, auth_ref, enabled, priority,
                        model_aliases_json, created_at, updated_at
                 FROM providers WHERE id = ?1",
            )?;
            let r = stmt
                .query_row(params![id], row_to_provider)
                .optional()?;
            Ok(r)
        })
    }

    pub fn provider_find_by_kind_and_base_url(
        &self,
        kind: ProviderKind,
        base_url: &str,
    ) -> Result<Option<Provider>> {
        fn norm(s: &str) -> String {
            s.trim().trim_end_matches('/').to_lowercase()
        }
        let want = norm(base_url);
        let list = self.provider_list()?;
        Ok(list
            .into_iter()
            .find(|p| p.kind == kind && norm(&p.base_url) == want))
    }

    pub fn provider_insert(&self, input: ProviderInput) -> Result<Provider> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = now_secs();
        let aliases_json = serde_json::to_string(&input.model_aliases)?;
        self.with(|c| {
            c.execute(
                "INSERT INTO providers (id, name, kind, base_url, auth_ref, enabled, priority,
                                        model_aliases_json, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    id,
                    input.name,
                    provider_kind_to_str(input.kind),
                    input.base_url,
                    input.auth_ref,
                    input.enabled as i32,
                    input.priority,
                    aliases_json,
                    now,
                    now,
                ],
            )?;
            Ok(())
        })?;
        self.provider_get(&id)?
            .context("inserted provider missing on read-back")
    }

    pub fn provider_update(&self, id: &str, input: ProviderInput) -> Result<Provider> {
        let now = now_secs();
        let aliases_json = serde_json::to_string(&input.model_aliases)?;
        let updated = self.with(|c| {
            let n = c.execute(
                "UPDATE providers
                 SET name = ?2, kind = ?3, base_url = ?4, auth_ref = ?5,
                     enabled = ?6, priority = ?7, model_aliases_json = ?8, updated_at = ?9
                 WHERE id = ?1",
                params![
                    id,
                    input.name,
                    provider_kind_to_str(input.kind),
                    input.base_url,
                    input.auth_ref,
                    input.enabled as i32,
                    input.priority,
                    aliases_json,
                    now,
                ],
            )?;
            Ok(n)
        })?;
        if updated == 0 {
            anyhow::bail!("provider {id} not found");
        }
        self.provider_get(id)?
            .context("updated provider missing on read-back")
    }

    pub fn provider_delete(&self, id: &str) -> Result<()> {
        self.with(|c| {
            let n = c.execute("DELETE FROM providers WHERE id = ?1", params![id])?;
            if n == 0 {
                anyhow::bail!("provider {id} not found");
            }
            Ok(())
        })
    }

    // --- routes -------------------------------------------------------------

    pub fn route_list(&self) -> Result<Vec<Route>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT id, name, match_model, target_provider_id, target_model, tier, priority
                 FROM routes ORDER BY priority ASC",
            )?;
            let rows = stmt.query_map([], row_to_route)?;
            let mut out = Vec::new();
            for r in rows {
                out.push(r?);
            }
            Ok(out)
        })
    }

    pub fn route_upsert(&self, route: Route) -> Result<()> {
        self.with(|c| {
            c.execute(
                "INSERT INTO routes (id, name, match_model, target_provider_id, target_model, tier, priority)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(id) DO UPDATE SET
                   name = excluded.name,
                   match_model = excluded.match_model,
                   target_provider_id = excluded.target_provider_id,
                   target_model = excluded.target_model,
                   tier = excluded.tier,
                   priority = excluded.priority",
                params![
                    route.id,
                    route.name,
                    route.match_model,
                    route.target_provider_id,
                    route.target_model,
                    route_tier_to_str(route.tier),
                    route.priority,
                ],
            )?;
            Ok(())
        })
    }

    // --- request logs -------------------------------------------------------

    pub fn log_insert(&self, log: &RequestLog) -> Result<()> {
        self.with(|c| {
            c.execute(
                "INSERT INTO request_logs (
                    id, started_at, app, provider_id, requested_model, upstream_model,
                    status_code, error, latency_ms, first_token_ms,
                    input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                    estimated_cost_usd,
                    wire, route_prefix, credential_id, cb_key, upstream_http_status,
                    upstream_error_preview, dedupe_key,
                    request_body, response_body, client_response_body
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                           ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25)",
                params![
                    log.id,
                    log.started_at,
                    log.app,
                    log.provider_id,
                    log.requested_model,
                    log.upstream_model,
                    log.status_code,
                    log.error,
                    log.latency_ms,
                    log.first_token_ms,
                    log.input_tokens,
                    log.output_tokens,
                    log.cache_read_tokens,
                    log.cache_creation_tokens,
                    log.estimated_cost_usd,
                    log.wire,
                    log.route_prefix,
                    log.credential_id,
                    log.cb_key,
                    log.upstream_http_status,
                    log.upstream_error_preview,
                    log.dedupe_key,
                    log.request_body,
                    log.response_body,
                    log.client_response_body,
                ],
            )?;
            Ok(())
        })
    }

    /// Attach gateway→client transform trace (e.g. Codex WS Responses events) after streaming ends.
    pub fn log_set_client_response_body(&self, id: &str, client_body: Option<&str>) -> Result<()> {
        self.with(|c| {
            let n = c.execute(
                "UPDATE request_logs SET client_response_body = ?1 WHERE id = ?2",
                params![client_body, id],
            )?;
            if n == 0 {
                anyhow::bail!("request_logs update: no row for id {id}");
            }
            Ok(())
        })
    }

    pub fn log_list(&self, limit: i64, offset: i64) -> Result<LogPage> {
        self.with(|c| {
            let total: i64 = c.query_row("SELECT count(*) FROM request_logs", [], |r| r.get(0))?;
            let mut stmt = c.prepare(
                &format!(
                    "SELECT {} FROM request_logs ORDER BY started_at DESC LIMIT ?1 OFFSET ?2",
                    Self::LOG_COLS_LIST
                ),
            )?;
            let rows = stmt.query_map(params![limit, offset], row_to_log_list)?;
            let mut items = Vec::new();
            for r in rows {
                items.push(r?);
            }
            Ok(LogPage {
                items,
                total,
                limit,
                offset,
            })
        })
    }

    pub fn usage_summary_last_hours(&self, hours: i64) -> Result<UsageSummary> {
        let since = now_secs() - hours * 3600;
        self.with(|c| {
            let (requests, input, output, cache_read, cache_create): (i64, i64, i64, i64, i64) = c
                .query_row(
                    "SELECT count(*),
                            COALESCE(sum(input_tokens), 0),
                            COALESCE(sum(output_tokens), 0),
                            COALESCE(sum(cache_read_tokens), 0),
                            COALESCE(sum(cache_creation_tokens), 0)
                     FROM request_logs WHERE started_at >= ?1",
                    params![since],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
                )?;
            Ok(UsageSummary {
                range: format!("last_{hours}h"),
                requests,
                input_tokens: input,
                output_tokens: output,
                cache_read_tokens: cache_read,
                cache_creation_tokens: cache_create,
                estimated_cost_usd: "0".into(),
            })
        })
    }

    pub fn count_logs_since(&self, since: i64) -> Result<i64> {
        self.with(|c| {
            let n: i64 = c.query_row(
                "SELECT count(*) FROM request_logs WHERE started_at >= ?1",
                params![since],
                |r| r.get(0),
            )?;
            Ok(n)
        })
    }

    /// Returns `(ok_count, total_count)` where ok is HTTP 2xx.
    pub fn ok_total_since(&self, since: i64) -> Result<(i64, i64)> {
        self.with(|c| {
            let total: i64 = c.query_row(
                "SELECT count(*) FROM request_logs WHERE started_at >= ?1",
                params![since],
                |r| r.get(0),
            )?;
            let ok: i64 = c.query_row(
                "SELECT count(*) FROM request_logs WHERE started_at >= ?1
                   AND status_code >= 200 AND status_code < 300",
                params![since],
                |r| r.get(0),
            )?;
            Ok((ok, total))
        })
    }

    /// Filtered log list — supports optional provider_id, status bucket, model, time range.
    pub fn log_list_filtered(
        &self,
        limit: i64,
        offset: i64,
        since: Option<i64>,
        provider_id: Option<&str>,
        status_ok: Option<bool>,
    ) -> Result<LogPage> {
        self.with(|c| {
            // Build dynamic WHERE
            let mut conditions = Vec::<String>::new();
            if let Some(ts) = since { conditions.push(format!("started_at >= {ts}")); }
            if let Some(pid) = provider_id { conditions.push(format!("provider_id = '{pid}'")); }
            if let Some(ok) = status_ok {
                if ok { conditions.push("status_code >= 200 AND status_code < 300".into()); }
                else  { conditions.push("(status_code IS NULL OR status_code >= 400)".into()); }
            }
            let where_clause = if conditions.is_empty() {
                String::new()
            } else {
                format!("WHERE {}", conditions.join(" AND "))
            };
            let total: i64 = c.query_row(
                &format!("SELECT count(*) FROM request_logs {where_clause}"),
                [],
                |r| r.get(0),
            )?;
            let mut stmt = c.prepare(&format!(
                "SELECT {} FROM request_logs {where_clause}
                 ORDER BY started_at DESC LIMIT {limit} OFFSET {offset}",
                Self::LOG_COLS_LIST
            ))?;
            let rows = stmt.query_map([], row_to_log_list)?;
            let mut items = Vec::new();
            for r in rows { items.push(r?); }
            Ok(LogPage { items, total, limit, offset })
        })
    }

    /// Full log row including optional raw bodies (may be large).
    pub fn log_get(&self, id: &str) -> Result<Option<RequestLog>> {
        self.with(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {} FROM request_logs WHERE id = ?1",
                Self::LOG_COLS_FULL
            ))?;
            let mut rows = stmt.query_map(params![id], row_to_log_detail)?;
            Ok(rows.next().transpose()?)
        })
    }

    /// Per-model request count and token totals for the last N hours.
    pub fn top_models(&self, hours: i64, limit: i64) -> Result<Vec<vibe_protocol::ModelStat>> {
        let since = now_secs() - hours * 3600;
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT COALESCE(upstream_model, requested_model, 'unknown') as model,
                        count(*) as reqs,
                        COALESCE(sum(input_tokens), 0),
                        COALESCE(sum(output_tokens), 0)
                 FROM request_logs WHERE started_at >= ?1
                 GROUP BY model ORDER BY reqs DESC LIMIT ?2",
            )?;
            let rows = stmt.query_map(params![since, limit], |r| {
                Ok(vibe_protocol::ModelStat {
                    model: r.get(0)?,
                    requests: r.get(1)?,
                    input_tokens: r.get(2)?,
                    output_tokens: r.get(3)?,
                })
            })?;
            let mut out = Vec::new();
            for r in rows { out.push(r?); }
            Ok(out)
        })
    }

    /// Per-provider aggregated stats for the last N hours.
    pub fn per_provider_stats(&self, hours: i64) -> Result<Vec<vibe_protocol::ProviderStat>> {
        let since = now_secs() - hours * 3600;
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT l.provider_id,
                        COALESCE(p.name, l.provider_id, 'unknown') as name,
                        count(*) as total,
                        sum(CASE WHEN l.status_code >= 200 AND l.status_code < 300 THEN 1 ELSE 0 END) as ok,
                        sum(CASE WHEN l.status_code IS NULL OR l.status_code >= 400 THEN 1 ELSE 0 END) as err,
                        COALESCE(avg(l.latency_ms), 0) as avg_lat,
                        COALESCE(sum(l.input_tokens), 0),
                        COALESCE(sum(l.output_tokens), 0),
                        sum(CASE WHEN l.status_code = 429 THEN 1 ELSE 0 END),
                        sum(CASE WHEN l.status_code = 503 THEN 1 ELSE 0 END),
                        sum(CASE WHEN l.status_code BETWEEN 400 AND 499 AND l.status_code != 429 THEN 1 ELSE 0 END),
                        sum(CASE WHEN l.status_code >= 500 AND l.status_code != 503 THEN 1 ELSE 0 END)
                 FROM request_logs l
                 LEFT JOIN providers p ON p.id = l.provider_id
                 WHERE l.started_at >= ?1 AND l.provider_id IS NOT NULL
                 GROUP BY l.provider_id",
            )?;
            let rows = stmt.query_map(params![since], |r| {
                let total: i64 = r.get(2)?;
                let ok: i64 = r.get(3)?;
                let err: i64 = r.get(4)?;
                let avg_lat: f64 = r.get(5)?;
                Ok(vibe_protocol::ProviderStat {
                    provider_id: r.get(0)?,
                    provider_name: r.get(1)?,
                    requests: total,
                    successes: ok,
                    failures: err,
                    success_rate: if total > 0 { ok as f64 / total as f64 } else { 1.0 },
                    avg_latency_ms: avg_lat as i64,
                    input_tokens: r.get(6)?,
                    output_tokens: r.get(7)?,
                    err_429: r.get(8)?,
                    err_503: r.get(9)?,
                    err_4xx_other: r.get(10)?,
                    err_5xx_other: r.get(11)?,
                })
            })?;
            let mut out = Vec::new();
            for r in rows { out.push(r?); }
            Ok(out)
        })
    }

    /// Rolling-window stats for a single provider (gateway `request_logs`, not upstream Plan quota).
    pub fn provider_stat_single(&self, provider_id: &str, hours: i64) -> Result<Option<vibe_protocol::ProviderStat>> {
        let since = now_secs() - hours * 3600;
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT l.provider_id,
                        COALESCE(p.name, l.provider_id, 'unknown') as name,
                        count(*) as total,
                        sum(CASE WHEN l.status_code >= 200 AND l.status_code < 300 THEN 1 ELSE 0 END) as ok,
                        sum(CASE WHEN l.status_code IS NULL OR l.status_code >= 400 THEN 1 ELSE 0 END) as err,
                        COALESCE(avg(l.latency_ms), 0) as avg_lat,
                        COALESCE(sum(l.input_tokens), 0),
                        COALESCE(sum(l.output_tokens), 0),
                        sum(CASE WHEN l.status_code = 429 THEN 1 ELSE 0 END),
                        sum(CASE WHEN l.status_code = 503 THEN 1 ELSE 0 END),
                        sum(CASE WHEN l.status_code BETWEEN 400 AND 499 AND l.status_code != 429 THEN 1 ELSE 0 END),
                        sum(CASE WHEN l.status_code >= 500 AND l.status_code != 503 THEN 1 ELSE 0 END)
                 FROM request_logs l
                 LEFT JOIN providers p ON p.id = l.provider_id
                 WHERE l.started_at >= ?1 AND l.provider_id = ?2
                 GROUP BY l.provider_id",
            )?;
            let mut rows = stmt.query_map(params![since, provider_id], |r| {
                let total: i64 = r.get(2)?;
                let ok: i64 = r.get(3)?;
                let err: i64 = r.get(4)?;
                let avg_lat: f64 = r.get(5)?;
                Ok(vibe_protocol::ProviderStat {
                    provider_id: r.get(0)?,
                    provider_name: r.get(1)?,
                    requests: total,
                    successes: ok,
                    failures: err,
                    success_rate: if total > 0 { ok as f64 / total as f64 } else { 1.0 },
                    avg_latency_ms: avg_lat as i64,
                    input_tokens: r.get(6)?,
                    output_tokens: r.get(7)?,
                    err_429: r.get(8)?,
                    err_503: r.get(9)?,
                    err_4xx_other: r.get(10)?,
                    err_5xx_other: r.get(11)?,
                })
            })?;
            Ok(rows.next().transpose()?)
        })
    }

    pub fn plan_snapshot_insert(&self, snap: &vibe_protocol::CredentialPlanSnapshot) -> Result<()> {
        self.with(|c| {
            c.execute(
                "INSERT INTO credential_plan_snapshots (
                    id, credential_id, captured_at,
                    codex_5h_used_percent, codex_7d_used_percent,
                    codex_5h_reset_after_seconds, codex_7d_reset_after_seconds,
                    codex_primary_used_percent, codex_secondary_used_percent,
                    summary, source
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    snap.id,
                    snap.credential_id,
                    snap.captured_at,
                    snap.codex_5h_used_percent,
                    snap.codex_7d_used_percent,
                    snap.codex_5h_reset_after_seconds,
                    snap.codex_7d_reset_after_seconds,
                    snap.codex_primary_used_percent,
                    snap.codex_secondary_used_percent,
                    snap.summary,
                    snap.source,
                ],
            )?;
            Ok(())
        })
    }

    pub fn plan_snapshot_latest(&self, credential_id: &str) -> Result<Option<vibe_protocol::CredentialPlanSnapshot>> {
        self.with(|c| {
            let snap = c
                .query_row(
                    "SELECT id, credential_id, captured_at,
                            codex_5h_used_percent, codex_7d_used_percent,
                            codex_5h_reset_after_seconds, codex_7d_reset_after_seconds,
                            codex_primary_used_percent, codex_secondary_used_percent,
                            summary, source
                     FROM credential_plan_snapshots
                     WHERE credential_id = ?1
                     ORDER BY captured_at DESC LIMIT 1",
                    params![credential_id],
                    row_to_plan_snapshot,
                )
                .optional()?;
            Ok(snap)
        })
    }

    pub fn credential_count_same_fingerprint(
        &self,
        fingerprint: &str,
        exclude_id: Option<&str>,
    ) -> Result<i64> {
        self.with(|c| {
            let n = if let Some(ex) = exclude_id {
                c.query_row(
                    "SELECT count(*) FROM credentials WHERE auth_fingerprint = ?1 AND id != ?2",
                    params![fingerprint, ex],
                    |r| r.get::<_, i64>(0),
                )?
            } else {
                c.query_row(
                    "SELECT count(*) FROM credentials WHERE auth_fingerprint = ?1",
                    params![fingerprint],
                    |r| r.get::<_, i64>(0),
                )?
            };
            Ok(n)
        })
    }

    pub fn latency_percentiles(&self, hours: i64) -> Result<(i64, i64)> {
        let since = now_secs() - hours * 3600;
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT latency_ms FROM request_logs
                 WHERE started_at >= ?1 AND latency_ms IS NOT NULL
                 ORDER BY latency_ms",
            )?;
            let vals: Vec<i64> = stmt
                .query_map(params![since], |r| r.get(0))?
                .filter_map(|r| r.ok())
                .collect();
            if vals.is_empty() {
                return Ok((0, 0));
            }
            let p50 = vals[vals.len() / 2];
            let p95 = vals[(vals.len() as f64 * 0.95) as usize].min(*vals.last().unwrap());
            Ok((p50, p95))
        })
    }

    // --- provider health -----------------------------------------------------

    pub fn health_upsert(
        &self,
        provider_id: &str,
        success: bool,
        latency_ms: Option<i64>,
        error: Option<&str>,
    ) -> Result<()> {
        let now = now_secs();
        self.with(|c| {
            c.execute(
                "INSERT INTO provider_health (
                     provider_id, is_healthy, consecutive_failures,
                     total_requests, total_successes, total_failures,
                     last_success_at, last_failure_at, last_error, avg_latency_ms, updated_at
                 ) VALUES (?1, ?2, ?3, 1, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT(provider_id) DO UPDATE SET
                     total_requests = total_requests + 1,
                     total_successes = total_successes + excluded.total_successes,
                     total_failures  = total_failures  + excluded.total_failures,
                     consecutive_failures = CASE
                         WHEN excluded.total_successes > 0 THEN 0
                         ELSE consecutive_failures + 1
                     END,
                     is_healthy = excluded.is_healthy,
                     last_success_at = COALESCE(excluded.last_success_at, last_success_at),
                     last_failure_at = COALESCE(excluded.last_failure_at, last_failure_at),
                     last_error = COALESCE(excluded.last_error, last_error),
                     avg_latency_ms = CASE
                         WHEN excluded.avg_latency_ms IS NOT NULL THEN
                             COALESCE((avg_latency_ms * 9 + excluded.avg_latency_ms) / 10, excluded.avg_latency_ms)
                         ELSE avg_latency_ms
                     END,
                     updated_at = excluded.updated_at",
                params![
                    provider_id,
                    success as i64,
                    if success { 0i64 } else { 1i64 },
                    if success { 1i64 } else { 0i64 },
                    if success { 0i64 } else { 1i64 },
                    if success { Some(now) } else { None::<i64> },
                    if success { None::<i64> } else { Some(now) },
                    error,
                    latency_ms,
                    now,
                ],
            )?;
            Ok(())
        })
    }

    pub fn health_get(&self, provider_id: &str) -> Result<Option<DbHealth>> {
        self.with(|c| {
            let r = c.query_row(
                "SELECT provider_id, is_healthy, consecutive_failures,
                        total_requests, total_successes, total_failures,
                        last_success_at, last_failure_at, last_error, avg_latency_ms, updated_at
                 FROM provider_health WHERE provider_id = ?1",
                params![provider_id],
                row_to_health,
            ).optional()?;
            Ok(r)
        })
    }

    pub fn health_list(&self) -> Result<Vec<DbHealth>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT provider_id, is_healthy, consecutive_failures,
                        total_requests, total_successes, total_failures,
                        last_success_at, last_failure_at, last_error, avg_latency_ms, updated_at
                 FROM provider_health",
            )?;
            let rows = stmt.query_map([], row_to_health)?;
            let mut out = Vec::new();
            for r in rows { out.push(r?); }
            Ok(out)
        })
    }

    // --- credentials --------------------------------------------------------

    const CRED_COLS: &'static str =
        "id, provider_id, label, auth_ref, plan_type, notes,
         enabled, priority,
         rl_requests_limit, rl_requests_remaining, rl_requests_reset_at,
         rl_tokens_limit, rl_tokens_remaining, rl_tokens_reset_at,
         last_used_at, last_error, consecutive_failures, created_at, updated_at,
         oauth_access_token, oauth_refresh_token, oauth_expires_at, auth_fingerprint";

    const LOG_COLS_LIST: &'static str =
        "id, started_at, app, provider_id, requested_model, upstream_model,
         status_code, error, latency_ms, first_token_ms,
         input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
         estimated_cost_usd,
         wire, route_prefix, credential_id, cb_key, upstream_http_status,
         upstream_error_preview, dedupe_key";

    const LOG_COLS_FULL: &'static str =
        "id, started_at, app, provider_id, requested_model, upstream_model,
         status_code, error, latency_ms, first_token_ms,
         input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
         estimated_cost_usd,
         wire, route_prefix, credential_id, cb_key, upstream_http_status,
         upstream_error_preview, dedupe_key,
         request_body, response_body, client_response_body";

    pub fn credential_list_for_provider(&self, provider_id: &str) -> Result<Vec<vibe_protocol::Credential>> {
        self.with(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {} FROM credentials WHERE provider_id = ?1 ORDER BY priority ASC, created_at ASC",
                Self::CRED_COLS,
            ))?;
            let rows = stmt.query_map(params![provider_id], row_to_credential)?;
            let mut out = Vec::new();
            for r in rows { out.push(r?); }
            Ok(out)
        })
    }

    pub fn credential_list_all(&self) -> Result<Vec<vibe_protocol::Credential>> {
        self.with(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {} FROM credentials ORDER BY provider_id, priority ASC, created_at ASC",
                Self::CRED_COLS,
            ))?;
            let rows = stmt.query_map([], row_to_credential)?;
            let mut out = Vec::new();
            for r in rows { out.push(r?); }
            Ok(out)
        })
    }

    pub fn credential_get(&self, id: &str) -> Result<Option<vibe_protocol::Credential>> {
        self.with(|c| {
            let r = c.query_row(
                &format!("SELECT {} FROM credentials WHERE id = ?1", Self::CRED_COLS),
                params![id],
                row_to_credential,
            ).optional()?;
            Ok(r)
        })
    }

    pub fn credential_insert(
        &self,
        provider_id: &str,
        input: vibe_protocol::CredentialInput,
        auth_fingerprint: Option<String>,
    ) -> Result<vibe_protocol::Credential> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = now_secs();
        self.with(|c| {
            c.execute(
                "INSERT INTO credentials
                    (id, provider_id, label, auth_ref, plan_type, notes, enabled, priority,
                     oauth_access_token, oauth_refresh_token, oauth_expires_at,
                     auth_fingerprint,
                     created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
                params![
                    id,
                    provider_id,
                    input.label,
                    input.auth_ref,
                    input.plan_type,
                    input.notes,
                    input.enabled as i32,
                    input.priority,
                    input.oauth_access_token,
                    input.oauth_refresh_token,
                    input.oauth_expires_at,
                    auth_fingerprint,
                    now,
                    now,
                ],
            )?;
            Ok(())
        })?;
        self.credential_get(&id)?.context("inserted credential missing on read-back")
    }

    pub fn credential_update(
        &self,
        id: &str,
        input: vibe_protocol::CredentialInput,
        auth_fingerprint: Option<String>,
    ) -> Result<vibe_protocol::Credential> {
        let now = now_secs();
        // oauth_refresh_token is write-only: only update it when the caller provides a value.
        let n = self.with(|c| {
            Ok(c.execute(
                "UPDATE credentials
                 SET label=?2, auth_ref=?3, plan_type=?4, notes=?5,
                     enabled=?6, priority=?7,
                     oauth_access_token=?8,
                     oauth_refresh_token=COALESCE(?9, oauth_refresh_token),
                     oauth_expires_at=?10,
                     auth_fingerprint=?11,
                     updated_at=?12
                 WHERE id=?1",
                params![
                    id,
                    input.label,
                    input.auth_ref,
                    input.plan_type,
                    input.notes,
                    input.enabled as i32,
                    input.priority,
                    input.oauth_access_token,
                    input.oauth_refresh_token,
                    input.oauth_expires_at,
                    auth_fingerprint,
                    now,
                ],
            )?)
        })?;
        if n == 0 {
            anyhow::bail!("credential {id} not found");
        }
        self.credential_get(id)?.context("updated credential missing on read-back")
    }

    /// Fetch only the refresh_token for a credential (write-only field, not in Credential).
    pub fn credential_get_refresh_token(&self, id: &str) -> Result<Option<String>> {
        self.with(|c| {
            let v = c.query_row(
                "SELECT oauth_refresh_token FROM credentials WHERE id = ?1",
                params![id],
                |r| r.get::<_, Option<String>>(0),
            ).optional()?;
            Ok(v.flatten())
        })
    }

    /// Called after a successful OAuth token refresh: persists fresh tokens.
    pub fn credential_update_oauth_tokens(
        &self,
        id: &str,
        access_token: &str,
        refresh_token: &str,
        expires_at: Option<i64>,
    ) -> Result<()> {
        let now = now_secs();
        self.with(|c| {
            c.execute(
                "UPDATE credentials
                 SET oauth_access_token=?2, oauth_refresh_token=?3,
                     oauth_expires_at=?4, updated_at=?5
                 WHERE id=?1",
                params![id, access_token, refresh_token, expires_at, now],
            )?;
            Ok(())
        })
    }

    pub fn credential_delete(&self, id: &str) -> Result<()> {
        self.with(|c| {
            let n = c.execute("DELETE FROM credentials WHERE id=?1", params![id])?;
            if n == 0 { anyhow::bail!("credential {id} not found"); }
            Ok(())
        })
    }

    /// Update rate-limit counters extracted from upstream response headers.
    pub fn credential_update_rate_limit(
        &self,
        id: &str,
        rl_requests_limit: Option<i64>,
        rl_requests_remaining: Option<i64>,
        rl_requests_reset_at: Option<i64>,
        rl_tokens_limit: Option<i64>,
        rl_tokens_remaining: Option<i64>,
        rl_tokens_reset_at: Option<i64>,
    ) -> Result<()> {
        let now = now_secs();
        self.with(|c| {
            c.execute(
                "UPDATE credentials SET
                     rl_requests_limit      = COALESCE(?2, rl_requests_limit),
                     rl_requests_remaining  = COALESCE(?3, rl_requests_remaining),
                     rl_requests_reset_at   = COALESCE(?4, rl_requests_reset_at),
                     rl_tokens_limit        = COALESCE(?5, rl_tokens_limit),
                     rl_tokens_remaining    = COALESCE(?6, rl_tokens_remaining),
                     rl_tokens_reset_at     = COALESCE(?7, rl_tokens_reset_at),
                     updated_at             = ?8
                 WHERE id = ?1",
                params![id, rl_requests_limit, rl_requests_remaining, rl_requests_reset_at,
                         rl_tokens_limit, rl_tokens_remaining, rl_tokens_reset_at, now],
            )?;
            Ok(())
        })
    }

    /// Record a successful use: update last_used_at, clear consecutive_failures and last_error.
    pub fn credential_record_success(&self, id: &str) -> Result<()> {
        let now = now_secs();
        self.with(|c| {
            c.execute(
                "UPDATE credentials SET last_used_at=?2, consecutive_failures=0, last_error=NULL, updated_at=?2 WHERE id=?1",
                params![id, now],
            )?;
            Ok(())
        })
    }

    /// Record a failed use: increment consecutive_failures, store error message.
    pub fn credential_record_failure(&self, id: &str, error: Option<&str>) -> Result<()> {
        let now = now_secs();
        self.with(|c| {
            c.execute(
                "UPDATE credentials SET last_used_at=?2, consecutive_failures=consecutive_failures+1,
                     last_error=COALESCE(?3, last_error), updated_at=?2
                 WHERE id=?1",
                params![id, now, error],
            )?;
            Ok(())
        })
    }
}

/// Raw health row from SQLite — circuit_state is added by the core layer.
#[derive(Debug, Clone)]
pub struct DbHealth {
    pub provider_id: String,
    pub is_healthy: bool,
    pub consecutive_failures: i32,
    pub total_requests: i64,
    pub total_successes: i64,
    pub total_failures: i64,
    pub last_success_at: Option<i64>,
    pub last_failure_at: Option<i64>,
    pub last_error: Option<String>,
    pub avg_latency_ms: Option<i64>,
    pub updated_at: i64,
}

// --- row -> struct helpers ---------------------------------------------------

fn row_to_provider(r: &rusqlite::Row) -> rusqlite::Result<Provider> {
    let kind_s: String = r.get(2)?;
    let aliases_json: String = r.get(7)?;
    Ok(Provider {
        id: r.get(0)?,
        name: r.get(1)?,
        kind: provider_kind_from_str(&kind_s)
            .map_err(|e| rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Text, e.into()))?,
        base_url: r.get(3)?,
        auth_ref: r.get(4)?,
        enabled: r.get::<_, i64>(5)? != 0,
        priority: r.get(6)?,
        model_aliases: serde_json::from_str(&aliases_json).unwrap_or_default(),
        created_at: r.get(8)?,
        updated_at: r.get(9)?,
    })
}

fn row_to_route(r: &rusqlite::Row) -> rusqlite::Result<Route> {
    let tier_s: String = r.get(5)?;
    Ok(Route {
        id: r.get(0)?,
        name: r.get(1)?,
        match_model: r.get(2)?,
        target_provider_id: r.get(3)?,
        target_model: r.get(4)?,
        tier: _route_tier_from_str(&tier_s)
            .map_err(|e| rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, e.into()))?,
        priority: r.get(6)?,
    })
}

fn row_to_health(r: &rusqlite::Row) -> rusqlite::Result<DbHealth> {
    Ok(DbHealth {
        provider_id: r.get(0)?,
        is_healthy: r.get::<_, i64>(1)? != 0,
        consecutive_failures: r.get(2)?,
        total_requests: r.get(3)?,
        total_successes: r.get(4)?,
        total_failures: r.get(5)?,
        last_success_at: r.get(6)?,
        last_failure_at: r.get(7)?,
        last_error: r.get(8)?,
        avg_latency_ms: r.get(9)?,
        updated_at: r.get(10)?,
    })
}

fn row_to_credential(r: &rusqlite::Row) -> rusqlite::Result<vibe_protocol::Credential> {
    // Columns (0-based), matching CRED_COLS order:
    // 0  id, 1  provider_id, 2  label, 3  auth_ref, 4  plan_type, 5  notes,
    // 6  enabled, 7  priority,
    // 8  rl_requests_limit, 9  rl_requests_remaining, 10 rl_requests_reset_at,
    // 11 rl_tokens_limit, 12 rl_tokens_remaining, 13 rl_tokens_reset_at,
    // 14 last_used_at, 15 last_error, 16 consecutive_failures,
    // 17 created_at, 18 updated_at,
    // 19 oauth_access_token, 20 oauth_refresh_token, 21 oauth_expires_at,
    // 22 auth_fingerprint
    let oauth_has_refresh: bool = r.get::<_, Option<String>>(20)?.is_some();
    Ok(vibe_protocol::Credential {
        id: r.get(0)?,
        provider_id: r.get(1)?,
        label: r.get(2)?,
        auth_ref: r.get(3)?,
        plan_type: r.get(4)?,
        notes: r.get(5)?,
        enabled: r.get::<_, i64>(6)? != 0,
        priority: r.get(7)?,
        rl_requests_limit: r.get(8)?,
        rl_requests_remaining: r.get(9)?,
        rl_requests_reset_at: r.get(10)?,
        rl_tokens_limit: r.get(11)?,
        rl_tokens_remaining: r.get(12)?,
        rl_tokens_reset_at: r.get(13)?,
        last_used_at: r.get(14)?,
        last_error: r.get(15)?,
        consecutive_failures: r.get(16)?,
        created_at: r.get(17)?,
        updated_at: r.get(18)?,
        oauth_access_token: r.get(19)?,
        oauth_has_refresh,
        oauth_expires_at: r.get(21)?,
        auth_fingerprint: r.get(22)?,
    })
}

fn row_to_plan_snapshot(
    r: &rusqlite::Row,
) -> rusqlite::Result<vibe_protocol::CredentialPlanSnapshot> {
    Ok(vibe_protocol::CredentialPlanSnapshot {
        id: r.get(0)?,
        credential_id: r.get(1)?,
        captured_at: r.get(2)?,
        codex_5h_used_percent: r.get(3)?,
        codex_7d_used_percent: r.get(4)?,
        codex_5h_reset_after_seconds: r.get(5)?,
        codex_7d_reset_after_seconds: r.get(6)?,
        codex_primary_used_percent: r.get(7)?,
        codex_secondary_used_percent: r.get(8)?,
        summary: r.get(9)?,
        source: r.get(10)?,
    })
}

fn row_to_log_list(r: &rusqlite::Row) -> rusqlite::Result<RequestLog> {
    Ok(RequestLog {
        id: r.get(0)?,
        started_at: r.get(1)?,
        app: r.get(2)?,
        provider_id: r.get(3)?,
        requested_model: r.get(4)?,
        upstream_model: r.get(5)?,
        status_code: r.get(6)?,
        error: r.get(7)?,
        latency_ms: r.get(8)?,
        first_token_ms: r.get(9)?,
        input_tokens: r.get(10)?,
        output_tokens: r.get(11)?,
        cache_read_tokens: r.get(12)?,
        cache_creation_tokens: r.get(13)?,
        estimated_cost_usd: r.get(14)?,
        wire: r.get(15)?,
        route_prefix: r.get(16)?,
        credential_id: r.get(17)?,
        cb_key: r.get(18)?,
        upstream_http_status: r.get(19)?,
        upstream_error_preview: r.get(20)?,
        dedupe_key: r.get(21)?,
        request_body: None,
        response_body: None,
        client_response_body: None,
    })
}

fn row_to_log_detail(r: &rusqlite::Row) -> rusqlite::Result<RequestLog> {
    Ok(RequestLog {
        id: r.get(0)?,
        started_at: r.get(1)?,
        app: r.get(2)?,
        provider_id: r.get(3)?,
        requested_model: r.get(4)?,
        upstream_model: r.get(5)?,
        status_code: r.get(6)?,
        error: r.get(7)?,
        latency_ms: r.get(8)?,
        first_token_ms: r.get(9)?,
        input_tokens: r.get(10)?,
        output_tokens: r.get(11)?,
        cache_read_tokens: r.get(12)?,
        cache_creation_tokens: r.get(13)?,
        estimated_cost_usd: r.get(14)?,
        wire: r.get(15)?,
        route_prefix: r.get(16)?,
        credential_id: r.get(17)?,
        cb_key: r.get(18)?,
        upstream_http_status: r.get(19)?,
        upstream_error_preview: r.get(20)?,
        dedupe_key: r.get(21)?,
        request_body: r.get(22)?,
        response_body: r.get(23)?,
        client_response_body: r.get(24)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_input() -> ProviderInput {
        ProviderInput {
            name: "anthropic-prod".into(),
            kind: ProviderKind::Anthropic,
            base_url: "https://api.anthropic.com".into(),
            auth_ref: Some("keyring:anthropic-prod".into()),
            enabled: true,
            priority: 100,
            model_aliases: vec![ModelAlias {
                alias: "high".into(),
                upstream_model: "claude-sonnet-4-6".into(),
            }],
        }
    }

    #[test]
    fn provider_crud() {
        let db = Db::memory().unwrap();
        let p = db.provider_insert(sample_input()).unwrap();
        assert_eq!(p.name, "anthropic-prod");

        let all = db.provider_list().unwrap();
        assert_eq!(all.len(), 1);

        let mut input = sample_input();
        input.name = "anthropic-prod-2".into();
        let updated = db.provider_update(&p.id, input).unwrap();
        assert_eq!(updated.name, "anthropic-prod-2");

        db.provider_delete(&p.id).unwrap();
        assert_eq!(db.provider_list().unwrap().len(), 0);
    }

    #[test]
    fn log_insert_and_summary() {
        let db = Db::memory().unwrap();
        db.log_insert(&RequestLog {
            id: "log-1".into(),
            started_at: now_secs(),
            app: Some("claude-code".into()),
            provider_id: None,
            requested_model: Some("claude".into()),
            upstream_model: Some("claude-sonnet-4-6".into()),
            status_code: Some(200),
            error: None,
            latency_ms: Some(123),
            first_token_ms: Some(50),
            input_tokens: 10,
            output_tokens: 20,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            estimated_cost_usd: "0".into(),
            wire: None,
            route_prefix: None,
            credential_id: None,
            cb_key: None,
            upstream_http_status: None,
            upstream_error_preview: None,
            dedupe_key: None,
            request_body: None,
            response_body: None,
            client_response_body: None,
        })
        .unwrap();

        let page = db.log_list(50, 0).unwrap();
        assert_eq!(page.items.len(), 1);
        let summary = db.usage_summary_last_hours(1).unwrap();
        assert_eq!(summary.requests, 1);
        assert_eq!(summary.input_tokens, 10);
        assert_eq!(summary.output_tokens, 20);
    }
}
