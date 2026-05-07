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
        ProviderKind::OpenaiCompat => "openai-compat",
        ProviderKind::OpenaiResponses => "openai-responses",
        ProviderKind::GeminiNative => "gemini-native",
    }
}

fn provider_kind_from_str(s: &str) -> Result<ProviderKind> {
    Ok(match s {
        "anthropic" => ProviderKind::Anthropic,
        "openai-compat" => ProviderKind::OpenaiCompat,
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
                    estimated_cost_usd
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
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
                ],
            )?;
            Ok(())
        })
    }

    pub fn log_list(&self, limit: i64, offset: i64) -> Result<LogPage> {
        self.with(|c| {
            let total: i64 = c.query_row("SELECT count(*) FROM request_logs", [], |r| r.get(0))?;
            let mut stmt = c.prepare(
                "SELECT id, started_at, app, provider_id, requested_model, upstream_model,
                        status_code, error, latency_ms, first_token_ms,
                        input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                        estimated_cost_usd
                 FROM request_logs ORDER BY started_at DESC LIMIT ?1 OFFSET ?2",
            )?;
            let rows = stmt.query_map(params![limit, offset], row_to_log)?;
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
                "SELECT id, started_at, app, provider_id, requested_model, upstream_model,
                        status_code, error, latency_ms, first_token_ms,
                        input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                        estimated_cost_usd
                 FROM request_logs {where_clause}
                 ORDER BY started_at DESC LIMIT {limit} OFFSET {offset}",
            ))?;
            let rows = stmt.query_map([], row_to_log)?;
            let mut items = Vec::new();
            for r in rows { items.push(r?); }
            Ok(LogPage { items, total, limit, offset })
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
                        COALESCE(sum(l.output_tokens), 0)
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
                })
            })?;
            let mut out = Vec::new();
            for r in rows { out.push(r?); }
            Ok(out)
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

fn row_to_log(r: &rusqlite::Row) -> rusqlite::Result<RequestLog> {
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
