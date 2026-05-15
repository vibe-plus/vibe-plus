//! Upstream response outcome classification.
//!
//! Maps HTTP status codes to retry decisions in one place, keeping policy
//! separate from the forwarding mechanics and easy to unit-test.

use crate::providers::Wire;
use axum::http::StatusCode;

/// What to do when an upstream returns a retryable status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryOutcome {
    /// 429 — rate limit / quota.
    /// Rotate credential; do NOT trip circuit breaker.
    RateLimit,
    /// 401 — authentication failed.
    /// Force-open CB for a cooldown period; do NOT permanently disable the
    /// credential. The CB will auto-recover after `open_timeout_secs`.
    AuthError,
    /// 402 — payment required. Treat like a server error: trip CB.
    PaymentError,
    /// 5xx — upstream server error. Trip CB.
    ServerError,
    /// 404 on the OpenAI Responses wire — route or model mismatch.
    /// Release sticky routing and try the next provider.
    NotFound,
}

/// Classify a response status into a retry decision.
///
/// Returns `None` when the response is **not** retryable (e.g. 400 Bad Request)
/// and should be forwarded directly to the client.
pub fn classify_retryable(status: StatusCode, wire: Wire) -> Option<RetryOutcome> {
    match status {
        StatusCode::TOO_MANY_REQUESTS => Some(RetryOutcome::RateLimit),
        StatusCode::UNAUTHORIZED => Some(RetryOutcome::AuthError),
        // 403 Forbidden — treat as auth error so the credential is rotated and
        // the circuit breaker trips instead of immediately returning a client
        // error that locks the Codex CLI into a reconnect loop on the same slot.
        StatusCode::FORBIDDEN => Some(RetryOutcome::AuthError),
        StatusCode::PAYMENT_REQUIRED => Some(RetryOutcome::PaymentError),
        StatusCode::NOT_FOUND if wire == Wire::OpenaiResponses => Some(RetryOutcome::NotFound),
        s if s.is_server_error() => Some(RetryOutcome::ServerError),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_all_retryable_statuses() {
        assert_eq!(
            classify_retryable(StatusCode::TOO_MANY_REQUESTS, Wire::OpenaiChat),
            Some(RetryOutcome::RateLimit)
        );
        assert_eq!(
            classify_retryable(StatusCode::UNAUTHORIZED, Wire::OpenaiChat),
            Some(RetryOutcome::AuthError)
        );
        assert_eq!(
            classify_retryable(StatusCode::PAYMENT_REQUIRED, Wire::OpenaiChat),
            Some(RetryOutcome::PaymentError)
        );
        assert_eq!(
            classify_retryable(StatusCode::INTERNAL_SERVER_ERROR, Wire::OpenaiChat),
            Some(RetryOutcome::ServerError)
        );
        assert_eq!(
            classify_retryable(StatusCode::SERVICE_UNAVAILABLE, Wire::OpenaiChat),
            Some(RetryOutcome::ServerError)
        );
        assert_eq!(
            classify_retryable(StatusCode::BAD_GATEWAY, Wire::OpenaiChat),
            Some(RetryOutcome::ServerError)
        );
    }

    #[test]
    fn responses_404_is_retryable_but_chat_404_is_client_error() {
        assert_eq!(
            classify_retryable(StatusCode::NOT_FOUND, Wire::OpenaiResponses),
            Some(RetryOutcome::NotFound)
        );
        assert_eq!(
            classify_retryable(StatusCode::NOT_FOUND, Wire::OpenaiChat),
            None
        );
        assert_eq!(
            classify_retryable(StatusCode::NOT_FOUND, Wire::Anthropic),
            None
        );
    }

    #[test]
    fn client_errors_are_not_retryable() {
        assert_eq!(
            classify_retryable(StatusCode::BAD_REQUEST, Wire::OpenaiChat),
            None
        );
        assert_eq!(
            classify_retryable(StatusCode::NOT_FOUND, Wire::OpenaiChat),
            None
        );
        assert_eq!(
            classify_retryable(StatusCode::UNPROCESSABLE_ENTITY, Wire::OpenaiChat),
            None
        );
    }

    #[test]
    fn forbidden_is_auth_error_so_credential_rotates() {
        // 403 must rotate credentials (same as 401) rather than short-circuit,
        // otherwise a broken slot locks the Codex CLI into a reconnect loop.
        assert_eq!(
            classify_retryable(StatusCode::FORBIDDEN, Wire::OpenaiChat),
            Some(RetryOutcome::AuthError)
        );
        assert_eq!(
            classify_retryable(StatusCode::FORBIDDEN, Wire::OpenaiResponses),
            Some(RetryOutcome::AuthError)
        );
    }

    #[test]
    fn success_and_redirect_are_not_retryable() {
        assert_eq!(classify_retryable(StatusCode::OK, Wire::OpenaiChat), None);
        assert_eq!(
            classify_retryable(StatusCode::CREATED, Wire::OpenaiChat),
            None
        );
        assert_eq!(
            classify_retryable(StatusCode::MOVED_PERMANENTLY, Wire::OpenaiChat),
            None
        );
    }
}
