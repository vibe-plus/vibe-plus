//! Per-provider three-state circuit breaker.
//!
//! States:
//!   Closed   — normal operation
//!   Open     — requests blocked; waits `open_timeout` before probing
//!   HalfOpen — limited probes; closes on enough successes, re-opens on any failure

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::config::FailoverConfig;

/// Returned by mutating circuit-breaker methods when the circuit state actually changes.
/// Callers can use this to emit observability events without the breaker needing WsHub access.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CircuitStateChange {
    /// Circuit just opened from Closed or re-opened from HalfOpen.
    Opened { consecutive_failures: u32 },
    /// Circuit closed: recovered after enough successes in HalfOpen.
    Closed,
    /// Circuit was manually reset to Closed by an operator.
    ManualReset,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Closed,
    Open,
    HalfOpen,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            State::Closed => "closed",
            State::Open => "open",
            State::HalfOpen => "half-open",
        }
    }
}

#[derive(Debug)]
struct Breaker {
    state: State,
    consecutive_failures: u32,
    consecutive_successes: u32,
    opened_at: Option<Instant>,
}

impl Breaker {
    fn new() -> Self {
        Self {
            state: State::Closed,
            consecutive_failures: 0,
            consecutive_successes: 0,
            opened_at: None,
        }
    }
}

#[derive(Clone)]
pub struct CircuitBreakers {
    inner: Arc<Mutex<HashMap<String, Breaker>>>,
    cfg: FailoverConfig,
}

impl CircuitBreakers {
    pub fn new(cfg: FailoverConfig) -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            cfg,
        }
    }

    /// Returns whether a request is allowed for this provider.
    pub fn allow(&self, provider_id: &str) -> bool {
        let mut map = self.inner.lock().unwrap();
        let b = map
            .entry(provider_id.to_string())
            .or_insert_with(Breaker::new);
        match b.state {
            State::Closed => true,
            State::Open => {
                let elapsed = b.opened_at.map(|t| t.elapsed()).unwrap_or(Duration::MAX);
                if elapsed >= Duration::from_secs(self.cfg.open_timeout_secs) {
                    b.state = State::HalfOpen;
                    b.consecutive_successes = 0;
                    true
                } else {
                    false
                }
            }
            State::HalfOpen => true,
        }
    }

    pub fn record_success(&self, provider_id: &str) -> Option<CircuitStateChange> {
        let mut map = self.inner.lock().unwrap();
        let b = map
            .entry(provider_id.to_string())
            .or_insert_with(Breaker::new);
        b.consecutive_failures = 0;
        match b.state {
            State::HalfOpen => {
                b.consecutive_successes += 1;
                if b.consecutive_successes >= self.cfg.success_threshold {
                    b.state = State::Closed;
                    b.consecutive_successes = 0;
                    tracing::info!(provider_id, "circuit closed (recovered)");
                    return Some(CircuitStateChange::Closed);
                }
            }
            State::Closed => {}
            State::Open => {} // shouldn't happen
        }
        None
    }

    pub fn record_failure(&self, provider_id: &str) -> Option<CircuitStateChange> {
        let mut map = self.inner.lock().unwrap();
        let b = map
            .entry(provider_id.to_string())
            .or_insert_with(Breaker::new);
        b.consecutive_failures += 1;
        b.consecutive_successes = 0;
        match b.state {
            State::Closed => {
                if b.consecutive_failures >= self.cfg.failure_threshold {
                    b.state = State::Open;
                    b.opened_at = Some(Instant::now());
                    let n = b.consecutive_failures;
                    tracing::warn!(provider_id, consecutive_failures = n, "circuit opened");
                    return Some(CircuitStateChange::Opened {
                        consecutive_failures: n,
                    });
                }
            }
            State::HalfOpen => {
                b.state = State::Open;
                b.opened_at = Some(Instant::now());
                let n = b.consecutive_failures;
                tracing::warn!(provider_id, "circuit re-opened (half-open probe failed)");
                return Some(CircuitStateChange::Opened {
                    consecutive_failures: n,
                });
            }
            State::Open => {}
        }
        None
    }

    pub fn state_of(&self, provider_id: &str) -> State {
        let map = self.inner.lock().unwrap();
        map.get(provider_id)
            .map(|b| b.state)
            .unwrap_or(State::Closed)
    }

    /// Returns true if the circuit is currently blocking requests (Open and within timeout).
    /// Unlike `allow()`, this has no side effects (no state transition).
    pub fn is_blocking(&self, provider_id: &str) -> bool {
        let map = self.inner.lock().unwrap();
        if let Some(b) = map.get(provider_id) {
            if b.state == State::Open {
                let elapsed = b.opened_at.map(|t| t.elapsed()).unwrap_or(Duration::MAX);
                return elapsed < Duration::from_secs(self.cfg.open_timeout_secs);
            }
        }
        false
    }

    pub fn open_remaining_secs(&self, provider_id: &str) -> Option<u64> {
        let map = self.inner.lock().unwrap();
        let b = map.get(provider_id)?;
        if b.state != State::Open {
            return None;
        }
        let elapsed = b.opened_at.map(|t| t.elapsed()).unwrap_or(Duration::MAX);
        let timeout = Duration::from_secs(self.cfg.open_timeout_secs);
        if elapsed >= timeout {
            Some(0)
        } else {
            Some((timeout - elapsed).as_secs())
        }
    }

    pub fn consecutive_failures(&self, provider_id: &str) -> u32 {
        let map = self.inner.lock().unwrap();
        map.get(provider_id)
            .map(|b| b.consecutive_failures)
            .unwrap_or(0)
    }

    /// Manually reset circuit-breaker state for UI operations.
    /// Returns `Some(ManualReset)` if the circuit was previously non-Closed.
    pub fn reset(&self, provider_id: &str) -> Option<CircuitStateChange> {
        let mut map = self.inner.lock().unwrap();
        let b = map
            .entry(provider_id.to_string())
            .or_insert_with(Breaker::new);
        let was_open = b.state != State::Closed;
        b.state = State::Closed;
        b.consecutive_failures = 0;
        b.consecutive_successes = 0;
        b.opened_at = None;
        tracing::info!(provider_id, "circuit manually reset to closed");
        if was_open {
            Some(CircuitStateChange::ManualReset)
        } else {
            None
        }
    }

    /// Force the circuit open immediately for fatal auth failures like 401,
    /// without waiting for the normal consecutive-failure threshold.
    /// Returns `Some(Opened)`.
    pub fn force_open(&self, provider_id: &str) -> CircuitStateChange {
        let mut map = self.inner.lock().unwrap();
        let b = map
            .entry(provider_id.to_string())
            .or_insert_with(Breaker::new);
        b.state = State::Open;
        b.opened_at = Some(Instant::now());
        b.consecutive_successes = 0;
        if b.consecutive_failures == 0 {
            b.consecutive_failures = 1;
        }
        let n = b.consecutive_failures;
        tracing::warn!(
            provider_id,
            consecutive_failures = n,
            "circuit force-opened"
        );
        CircuitStateChange::Opened {
            consecutive_failures: n,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::FailoverConfig;

    fn cfg() -> FailoverConfig {
        FailoverConfig {
            failure_threshold: 3,
            success_threshold: 2,
            open_timeout_secs: 1,
            inject_cache: false,
        }
    }

    #[test]
    fn closes_after_enough_successes_in_half_open() {
        let cb = CircuitBreakers::new(cfg());
        for _ in 0..3 {
            cb.record_failure("p1");
        }
        assert_eq!(cb.state_of("p1"), State::Open);
        assert!(!cb.allow("p1")); // still within timeout

        // Simulate time passing by directly patching state
        {
            let mut map = cb.inner.lock().unwrap();
            map.get_mut("p1").unwrap().opened_at = Some(Instant::now() - Duration::from_secs(2));
        }
        assert!(cb.allow("p1")); // transitions to HalfOpen
        assert_eq!(cb.state_of("p1"), State::HalfOpen);

        cb.record_success("p1");
        assert_eq!(cb.state_of("p1"), State::HalfOpen); // still need 1 more
        cb.record_success("p1");
        assert_eq!(cb.state_of("p1"), State::Closed);
    }

    #[test]
    fn reopens_on_half_open_failure() {
        let cb = CircuitBreakers::new(cfg());
        for _ in 0..3 {
            cb.record_failure("p2");
        }
        {
            let mut map = cb.inner.lock().unwrap();
            map.get_mut("p2").unwrap().opened_at = Some(Instant::now() - Duration::from_secs(2));
        }
        cb.allow("p2"); // → HalfOpen
        cb.record_failure("p2");
        assert_eq!(cb.state_of("p2"), State::Open);
    }
}
