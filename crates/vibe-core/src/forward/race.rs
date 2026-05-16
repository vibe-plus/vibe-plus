//! Fanout-race forwarding harness (Phase 3 — scaffolding).
//!
//! When a matched [`vibe_protocol::Route`] is set to
//! [`vibe_protocol::ForwardStrategy::Race`], the gateway should:
//!
//! 1. Take the first `Route::fanout_n` expanded credential picks.
//! 2. Fire each upstream request concurrently via `tokio::spawn`.
//! 3. The first racer to return `200 OK + first body byte` wins; its
//!    response is streamed/buffered back to the downstream client.
//! 4. Losers are aborted via a shared [`tokio_util::sync::CancellationToken`]
//!    and logged with [`UpstreamAttemptOutcome::RaceAborted`].
//! 5. If every racer returns retryable failures, fall through to the
//!    remaining picks via the normal sequential loop.
//!
//! ## Current state (Phase 3b scaffolding)
//!
//! - Types and signatures are stable.
//! - [`try_one_pick`] and [`forward_race`] are stubs that emit a clear 501
//!   response and a tracing warning — this surfaces misconfiguration during
//!   testing without silently falling back.
//! - The actual extraction of `forward()`'s 720-line per-pick body into
//!   [`try_one_pick`] lands in Phase 3c. That refactor is mechanical but
//!   bulky (transforming every `continue;` → [`PickResult::Retry`] and every
//!   terminal `return` → [`PickResult::Final`]).

use super::selector::ExpandedPick;
use crate::claude_summary::ClaudeClientKind;
use crate::codex_summary::CodexClientKind;
use crate::providers::Wire;
use crate::state::AppState;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use bytes::Bytes;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use vibe_protocol::CredentialPlanSnapshot;

/// Per-request data shared across all racers and the sequential fallback.
///
/// All fields are owned (or `Arc`-wrapped) so a `PickCtx` can be cloned cheaply
/// into spawned racer tasks without lifetime entanglement.
#[derive(Clone)]
pub struct PickCtx {
    pub wire: Wire,
    pub route_prefix: Option<String>,
    pub log_id: String,
    pub started_at: i64,
    pub started_instant: Instant,
    pub app: String,
    pub requested_model: String,
    pub upstream_path: Option<String>,
    pub dedupe_key: Option<String>,
    pub client_transport: Option<String>,
    pub request_headers_json: Option<String>,
    pub codex_client_kind: CodexClientKind,
    pub claude_client_kind: ClaudeClientKind,
    pub body: Bytes,
    pub req_headers: HeaderMap,
    pub request_snapshot: Option<String>,
    pub sticky_key: Option<String>,
    pub plan_by_cred: Arc<HashMap<String, CredentialPlanSnapshot>>,
}

/// Outcome of attempting a single credential pick.
pub enum PickResult {
    /// Terminal — propagate to the downstream client unchanged.
    /// Includes both 2xx success (streaming or buffered) and non-retryable
    /// 4xx errors (caller's fault — don't try another upstream).
    Final(Response),
    /// Retryable — move on to the next pick (or, in race mode, wait for
    /// another racer). Caller updates `last_error` and routing trace.
    Retry {
        last_error: String,
        routing_note: String,
    },
    /// Circuit-breaker open — pick was skipped before any upstream call.
    CircuitSkip { provider_id: String },
    /// Race loser — another racer won, this attempt was aborted in-flight.
    /// Caller logs but does not surface to downstream.
    RaceAborted { provider_id: String },
}

/// Stub for the per-pick attempt that the race harness will spawn.
///
/// Phase 3c will populate this with the body of the existing for-loop in
/// `forward()` (lines ~1531-2252 of mod.rs), with each `continue;` rewritten
/// to a [`PickResult::Retry`] return and each terminal `return` rewritten to
/// [`PickResult::Final`].
#[allow(dead_code)]
pub async fn try_one_pick(
    _state: AppState,
    _epick: ExpandedPick,
    _attempt_index: i32,
    _ctx: Arc<PickCtx>,
) -> PickResult {
    PickResult::Retry {
        last_error: "race: try_one_pick not yet implemented (Phase 3c)".into(),
        routing_note: "skipped — try_one_pick stub".into(),
    }
}

/// Stub harness entry point. Today this returns a clear 501 so race-enabled
/// routes fail loudly instead of silently falling back. Phase 3c will replace
/// the body with a `tokio::spawn` + `mpsc::channel` + `CancellationToken`
/// race loop driven by [`try_one_pick`].
#[allow(dead_code)]
pub async fn forward_race(
    _state: AppState,
    _expanded_picks: Vec<ExpandedPick>,
    _ctx: Arc<PickCtx>,
    fanout_n: u8,
) -> Response {
    tracing::warn!(
        fanout_n,
        "race harness invoked but not yet implemented (Phase 3c)"
    );
    (
        StatusCode::NOT_IMPLEMENTED,
        "forward_race harness pending Phase 3c — falling back to sequential is recommended for now",
    )
        .into_response()
}
