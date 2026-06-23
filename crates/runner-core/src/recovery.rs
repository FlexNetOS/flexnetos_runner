//! Declarative recovery routing (adapted from `strongdm/attractor`'s `retry_target` /
//! `fallback_retry_target` / `wait.human` edges, which turn a step failure into a *declared* next
//! move instead of an ad-hoc loop).
//!
//! When a dispatch fails, the runner does not silently drop it (the orchestrator would re-fire the
//! webhook and the breaker would eventually trip — a blunt stop with no guidance). Instead the
//! runner emits a **recovery directive**: either *retry the same job after a backoff* (attractor's
//! `retry_target`, for transient kernel failures), or *escalate to a human* by opening a fork /
//! review PR (attractor's `fallback_retry_target` → `wait.human`, once retries are exhausted or the
//! failure is inherently un-retryable).
//!
//! **Delegate-only / advice-not-action.** The runner is one-job-per-connection; it does **not**
//! itself re-dispatch. The directive is *advice carried back to the orchestrator* (the App / weave),
//! which owns the retry timer and the escalation PR. The runner only decides *what should happen
//! next* — the same separation as the cost seam ([`crate::cost`]) and the agent seam: the runner
//! reports, weave acts. This keeps recovery firmly in the execution plane (when to give up / when a
//! human is needed) and out of the model-routing plane (weave's domain).
//!
//! The retry count is per-[`fingerprint`](crate::loopguard::fingerprint) (the same semantic key the
//! breaker uses), held in a [`RetryLedger`] the dispatcher keeps across connections. A clean
//! delegation [`clears`](RetryLedger::clear) the fingerprint, so a *later* transient blip starts
//! its own fresh retry budget. The breaker remains the hard backstop above this: recovery proposes
//! a bounded, backed-off retry; the breaker still trips if the orchestrator ignores the advice and
//! hammers the same work.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Why a dispatch failed — the input to the recovery decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureKind {
    /// The kernel was invoked but returned an error — usually transient (retryable).
    KernelFailed,
    /// The runaway-loop breaker tripped — the same work is already looping (NOT retryable: retrying
    /// is exactly what's going wrong; a human must look).
    LoopTripped,
    /// The job was structurally invalid (NOT retryable: the same bytes can never become valid).
    Malformed,
}

impl FailureKind {
    /// Whether a backed-off retry of the *same* job could plausibly succeed.
    fn is_retryable(self) -> bool {
        matches!(self, FailureKind::KernelFailed)
    }
}

/// What the orchestrator should do next. Serializable so it rides back on the dispatch reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryVerb {
    /// Re-dispatch the same job after the directive's backoff.
    Retry,
    /// Stop retrying; open a fork / review PR for a human (attractor's `wait.human`).
    Escalate,
}

/// The recovery directive returned to the orchestrator for a failed dispatch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryDirective {
    pub action: RecoveryVerb,
    /// Which retry attempt this would be (1-based). `0` for an immediate escalation (never retried).
    pub attempt: u32,
    /// The configured retry ceiling, for context in the reply / audit log.
    pub max_retries: u32,
    /// How long the orchestrator should wait before re-dispatching (0 for an escalation).
    pub backoff_secs: u64,
    /// Human-readable rationale (goes in the reply error and the audit detail).
    pub reason: String,
}

impl RecoveryDirective {
    /// Whether this directive asks for a retry (vs. a human escalation).
    pub fn is_retry(&self) -> bool {
        self.action == RecoveryVerb::Retry
    }

    /// A compact one-line summary for a rejection message / audit detail.
    pub fn summary(&self) -> String {
        match self.action {
            RecoveryVerb::Retry => format!(
                "recovery=retry (attempt {}/{}, back off {}s): {}",
                self.attempt, self.max_retries, self.backoff_secs, self.reason
            ),
            RecoveryVerb::Escalate => format!("recovery=escalate: {}", self.reason),
        }
    }
}

/// Stateless recovery configuration: how many times to retry a transient failure, and the base
/// backoff (doubled per attempt — exponential, capped).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecoveryPolicy {
    max_retries: u32,
    base_backoff_secs: u64,
}

impl RecoveryPolicy {
    /// Build a policy. `base_backoff_secs` is clamped to ≥1 so a retry always waits.
    pub fn new(max_retries: u32, base_backoff_secs: u64) -> Self {
        Self {
            max_retries,
            base_backoff_secs: base_backoff_secs.max(1),
        }
    }

    /// The configured retry ceiling.
    pub fn max_retries(&self) -> u32 {
        self.max_retries
    }

    /// The base backoff (attempt 1's wait).
    pub fn base_backoff_secs(&self) -> u64 {
        self.base_backoff_secs
    }

    /// Exponential backoff for `attempt` (1-based): `base * 2^(attempt-1)`, saturating.
    fn backoff_for(&self, attempt: u32) -> u64 {
        let shift = attempt.saturating_sub(1).min(16); // cap the exponent (≤ base * 65536)
        self.base_backoff_secs.saturating_mul(1u64 << shift)
    }

    /// Decide the recovery directive for a failure of `fingerprint`, advancing the per-fingerprint
    /// attempt counter in `ledger`. Retryable failures retry with backoff until `max_retries` is
    /// exceeded, then escalate; un-retryable failures escalate immediately.
    pub fn decide(
        &self,
        ledger: &mut RetryLedger,
        fingerprint: &str,
        failure: FailureKind,
    ) -> RecoveryDirective {
        if !failure.is_retryable() {
            let reason = match failure {
                FailureKind::LoopTripped => {
                    "runaway-loop breaker tripped — the same work is looping; a human must \
                     intervene (open a review PR)"
                }
                FailureKind::Malformed => {
                    "job is structurally invalid — re-dispatching the same spec cannot succeed; \
                     fix the JobSpec or escalate to a human"
                }
                FailureKind::KernelFailed => unreachable!("KernelFailed is retryable"),
            };
            return RecoveryDirective {
                action: RecoveryVerb::Escalate,
                attempt: 0,
                max_retries: self.max_retries,
                backoff_secs: 0,
                reason: reason.to_string(),
            };
        }

        let attempt = ledger.bump(fingerprint);
        if attempt <= self.max_retries {
            RecoveryDirective {
                action: RecoveryVerb::Retry,
                attempt,
                max_retries: self.max_retries,
                backoff_secs: self.backoff_for(attempt),
                reason: format!(
                    "kernel failed (transient); retry {attempt} of {} after backoff",
                    self.max_retries
                ),
            }
        } else {
            RecoveryDirective {
                action: RecoveryVerb::Escalate,
                attempt,
                max_retries: self.max_retries,
                backoff_secs: 0,
                reason: format!(
                    "kernel failed {attempt} times (> {} retries) — escalating to human review",
                    self.max_retries
                ),
            }
        }
    }
}

impl Default for RecoveryPolicy {
    /// Two retries, 5s base backoff (5s → 10s → escalate).
    fn default() -> Self {
        Self::new(2, 5)
    }
}

/// Per-fingerprint retry attempt counts, held by the dispatcher across connections (like the
/// breaker's window and the governor's spend). A clean delegation clears the fingerprint.
#[derive(Debug, Clone, Default)]
pub struct RetryLedger {
    attempts: HashMap<String, u32>,
}

impl RetryLedger {
    /// A fresh, empty ledger.
    pub fn new() -> Self {
        Self::default()
    }

    /// Increment and return the attempt count for `fingerprint` (first failure → 1).
    pub fn bump(&mut self, fingerprint: &str) -> u32 {
        let n = self.attempts.entry(fingerprint.to_string()).or_insert(0);
        *n += 1;
        *n
    }

    /// The current attempt count for `fingerprint` (0 if never failed).
    pub fn attempts(&self, fingerprint: &str) -> u32 {
        self.attempts.get(fingerprint).copied().unwrap_or(0)
    }

    /// Forget `fingerprint` — called after a clean delegation so a later blip starts fresh.
    pub fn clear(&mut self, fingerprint: &str) {
        self.attempts.remove(fingerprint);
    }

    /// Number of fingerprints currently tracked (for observability / tests).
    pub fn tracked(&self) -> usize {
        self.attempts.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FP: &str = "fingerprint-a";

    #[test]
    fn transient_failure_retries_then_escalates() {
        let policy = RecoveryPolicy::new(2, 5);
        let mut ledger = RetryLedger::new();

        // Attempt 1 → retry, 5s.
        let d1 = policy.decide(&mut ledger, FP, FailureKind::KernelFailed);
        assert_eq!(d1.action, RecoveryVerb::Retry);
        assert_eq!(d1.attempt, 1);
        assert_eq!(d1.backoff_secs, 5);

        // Attempt 2 → retry, 10s (exponential).
        let d2 = policy.decide(&mut ledger, FP, FailureKind::KernelFailed);
        assert_eq!(d2.action, RecoveryVerb::Retry);
        assert_eq!(d2.attempt, 2);
        assert_eq!(d2.backoff_secs, 10);

        // Attempt 3 → over the ceiling → escalate.
        let d3 = policy.decide(&mut ledger, FP, FailureKind::KernelFailed);
        assert_eq!(d3.action, RecoveryVerb::Escalate);
        assert_eq!(d3.attempt, 3);
        assert_eq!(d3.backoff_secs, 0);
    }

    #[test]
    fn loop_trip_escalates_immediately_without_consuming_retries() {
        let policy = RecoveryPolicy::default();
        let mut ledger = RetryLedger::new();
        let d = policy.decide(&mut ledger, FP, FailureKind::LoopTripped);
        assert_eq!(d.action, RecoveryVerb::Escalate);
        assert_eq!(d.attempt, 0);
        assert_eq!(
            ledger.attempts(FP),
            0,
            "escalation must not bump the counter"
        );
        assert!(d.reason.contains("runaway-loop"));
    }

    #[test]
    fn malformed_escalates_immediately() {
        let policy = RecoveryPolicy::default();
        let mut ledger = RetryLedger::new();
        let d = policy.decide(&mut ledger, FP, FailureKind::Malformed);
        assert_eq!(d.action, RecoveryVerb::Escalate);
        assert!(d.reason.contains("structurally invalid"));
    }

    #[test]
    fn clear_resets_the_retry_budget() {
        let policy = RecoveryPolicy::new(1, 1);
        let mut ledger = RetryLedger::new();
        assert_eq!(
            policy
                .decide(&mut ledger, FP, FailureKind::KernelFailed)
                .action,
            RecoveryVerb::Retry
        );
        // A clean delegation clears the fingerprint…
        ledger.clear(FP);
        assert_eq!(ledger.attempts(FP), 0);
        // …so the next failure is once again attempt 1 (retry, not escalate).
        let d = policy.decide(&mut ledger, FP, FailureKind::KernelFailed);
        assert_eq!(d.action, RecoveryVerb::Retry);
        assert_eq!(d.attempt, 1);
    }

    #[test]
    fn per_fingerprint_counters_are_independent() {
        let policy = RecoveryPolicy::new(1, 1);
        let mut ledger = RetryLedger::new();
        policy.decide(&mut ledger, "a", FailureKind::KernelFailed); // a → 1 (retry)
        policy.decide(&mut ledger, "a", FailureKind::KernelFailed); // a → 2 (escalate)
        let b = policy.decide(&mut ledger, "b", FailureKind::KernelFailed); // b → 1 (retry)
        assert_eq!(b.action, RecoveryVerb::Retry);
        assert_eq!(ledger.tracked(), 2);
    }

    #[test]
    fn directive_serializes_with_snake_case_verb() {
        let policy = RecoveryPolicy::default();
        let mut ledger = RetryLedger::new();
        let d = policy.decide(&mut ledger, FP, FailureKind::KernelFailed);
        let json = serde_json::to_string(&d).unwrap();
        assert!(json.contains(r#""action":"retry""#));
        let back: RecoveryDirective = serde_json::from_str(&json).unwrap();
        assert_eq!(back, d);
    }

    #[test]
    fn base_backoff_is_clamped_to_at_least_one() {
        assert_eq!(RecoveryPolicy::new(2, 0).base_backoff_secs(), 1);
    }

    #[test]
    fn default_policy_is_two_retries_five_seconds() {
        let p = RecoveryPolicy::default();
        assert_eq!(p.max_retries(), 2);
        assert_eq!(p.base_backoff_secs(), 5);
    }
}
