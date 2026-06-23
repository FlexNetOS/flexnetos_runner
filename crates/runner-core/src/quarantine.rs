//! Cross-dispatch quarantine of a repeatedly-failing job (adapted from `Conway-Research/automaton`'s
//! child lifecycle `… → unhealthy → recovering → dead` and `strongdm/attractor`'s terminal failure
//! state — a unit that keeps failing the same way is moved to a *terminal* state instead of being
//! retried forever).
//!
//! The runner already has two adjacent guards, and quarantine is deliberately neither:
//! - the **loop breaker** ([`crate::loopguard`]) trips on the *volume* of an identical fingerprint
//!   within a recent window — regardless of whether those dispatches succeed — and **recovers
//!   automatically** as the fingerprint ages out. It catches a tight *loop*, not a *bad job*.
//! - **recovery routing** ([`crate::recovery`]) counts a fingerprint's transient `KernelFailed`
//!   attempts and flips its *advice* from retry to escalate once the retry ceiling is exceeded — but
//!   it is **advice only**: nothing stops the orchestrator from ignoring "escalate" and re-dispatching
//!   the same structurally-doomed work again. The job still reaches the kernel every time.
//!
//! **Quarantine is the enforcement teeth behind that advice.** When a fingerprint has failed at the
//! kernel `threshold` times, it is moved to a **terminal quarantined state**, and every *subsequent*
//! dispatch of that fingerprint is **refused at admission — before the kernel** — until an operator
//! explicitly re-arms the runner (the same "kill-switch, re-arm to continue" doctrine as the budget
//! governor). A clean delegation of a fingerprint resets its failure count (and releases it), so a
//! later *transient* blip of the same work starts fresh — only *persistent* same-way failure latches.
//!
//! Pure and in-memory — no I/O, no clock — like the breaker and the retry ledger. The long-lived UDS
//! server owns one [`QuarantineLedger`] across its accept loop. **Opt-in**: a `threshold` of `0`
//! ([`QuarantinePolicy::disabled`]) never quarantines anything (behaviour-preserving default), so the
//! runner is unchanged until an operator sets `FXRUN_QUARANTINE_THRESHOLD`. Delegate-only: quarantine
//! is a fail-closed admission verdict over the runner's own failure history; it never decides *which*
//! kernel or model runs (weave's domain).

use std::collections::{HashMap, HashSet};

/// How many kernel failures of one fingerprint latch it into quarantine. `0` disables the gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuarantinePolicy {
    threshold: u32,
}

impl QuarantinePolicy {
    /// Build a policy that quarantines a fingerprint after `threshold` kernel failures.
    /// `threshold == 0` disables quarantine entirely (behaviour-preserving).
    pub fn new(threshold: u32) -> Self {
        Self { threshold }
    }

    /// The behaviour-preserving default: quarantine disabled.
    pub fn disabled() -> Self {
        Self { threshold: 0 }
    }

    /// Whether the gate is active (a non-zero threshold was configured).
    pub fn is_active(&self) -> bool {
        self.threshold > 0
    }

    /// The configured failure threshold (0 when disabled).
    pub fn threshold(&self) -> u32 {
        self.threshold
    }

    /// Record a kernel failure of `fingerprint` against `ledger`, quarantining it if its accumulated
    /// failure count reaches the threshold. Returns `true` iff this failure *just* quarantined the
    /// fingerprint (the latching edge — useful for an audit note). No-op when the policy is disabled.
    pub fn on_failure(&self, ledger: &mut QuarantineLedger, fingerprint: &str) -> bool {
        if !self.is_active() {
            return false;
        }
        let count = ledger.record_failure(fingerprint);
        if count >= self.threshold && !ledger.is_quarantined(fingerprint) {
            ledger.quarantine(fingerprint);
            true
        } else {
            false
        }
    }
}

impl Default for QuarantinePolicy {
    /// Disabled by default (preserves the pre-quarantine behaviour).
    fn default() -> Self {
        Self::disabled()
    }
}

/// Per-fingerprint failure counts plus the terminal quarantined set, held by the dispatcher across
/// connections (like the breaker's window and the retry ledger). A clean delegation resets a
/// fingerprint; a threshold breach latches it until an operator re-arms.
#[derive(Debug, Clone, Default)]
pub struct QuarantineLedger {
    failures: HashMap<String, u32>,
    quarantined: HashSet<String>,
}

impl QuarantineLedger {
    /// A fresh, empty ledger.
    pub fn new() -> Self {
        Self::default()
    }

    /// Increment and return the accumulated failure count for `fingerprint`.
    pub fn record_failure(&mut self, fingerprint: &str) -> u32 {
        let n = self.failures.entry(fingerprint.to_string()).or_insert(0);
        *n += 1;
        *n
    }

    /// The current failure count for `fingerprint` (0 if never failed).
    pub fn failures(&self, fingerprint: &str) -> u32 {
        self.failures.get(fingerprint).copied().unwrap_or(0)
    }

    /// Move `fingerprint` into the terminal quarantined state.
    pub fn quarantine(&mut self, fingerprint: &str) {
        self.quarantined.insert(fingerprint.to_string());
    }

    /// Whether `fingerprint` is currently quarantined (admission must refuse it).
    pub fn is_quarantined(&self, fingerprint: &str) -> bool {
        self.quarantined.contains(fingerprint)
    }

    /// Clear a fingerprint after a clean delegation: forget its failures and release any quarantine,
    /// so a later *transient* failure of the same work starts from zero.
    pub fn clear(&mut self, fingerprint: &str) {
        self.failures.remove(fingerprint);
        self.quarantined.remove(fingerprint);
    }

    /// Operator re-arm: release every quarantined fingerprint and reset all failure counts.
    pub fn release_all(&mut self) {
        self.failures.clear();
        self.quarantined.clear();
    }

    /// Number of fingerprints currently quarantined (for the audit/observability surface).
    pub fn quarantined_count(&self) -> usize {
        self.quarantined.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FP: &str = "fingerprint-a";

    #[test]
    fn disabled_policy_never_quarantines() {
        let policy = QuarantinePolicy::disabled();
        let mut ledger = QuarantineLedger::new();
        for _ in 0..50 {
            assert!(!policy.on_failure(&mut ledger, FP));
        }
        assert!(!ledger.is_quarantined(FP));
        assert_eq!(ledger.quarantined_count(), 0);
    }

    #[test]
    fn quarantines_exactly_at_the_threshold() {
        let policy = QuarantinePolicy::new(3);
        let mut ledger = QuarantineLedger::new();
        // First two failures count but do not latch.
        assert!(!policy.on_failure(&mut ledger, FP));
        assert!(!ledger.is_quarantined(FP));
        assert!(!policy.on_failure(&mut ledger, FP));
        assert!(!ledger.is_quarantined(FP));
        // The third failure latches — and reports the latching edge.
        assert!(policy.on_failure(&mut ledger, FP), "3rd failure latches");
        assert!(ledger.is_quarantined(FP));
        assert_eq!(ledger.quarantined_count(), 1);
    }

    #[test]
    fn latching_edge_fires_only_once() {
        let policy = QuarantinePolicy::new(2);
        let mut ledger = QuarantineLedger::new();
        policy.on_failure(&mut ledger, FP); // 1
        assert!(policy.on_failure(&mut ledger, FP), "2nd latches"); // 2 → latch
                                                                    // Further failures are already quarantined → no new latching edge.
        assert!(!policy.on_failure(&mut ledger, FP));
        assert!(ledger.is_quarantined(FP));
    }

    #[test]
    fn clean_delegation_resets_and_releases() {
        let policy = QuarantinePolicy::new(2);
        let mut ledger = QuarantineLedger::new();
        policy.on_failure(&mut ledger, FP);
        policy.on_failure(&mut ledger, FP);
        assert!(ledger.is_quarantined(FP));
        // A clean delegation clears it…
        ledger.clear(FP);
        assert!(!ledger.is_quarantined(FP));
        assert_eq!(ledger.failures(FP), 0);
        // …so the count starts fresh (one failure no longer re-latches a threshold-2 policy).
        assert!(!policy.on_failure(&mut ledger, FP));
        assert!(!ledger.is_quarantined(FP));
    }

    #[test]
    fn per_fingerprint_isolation() {
        let policy = QuarantinePolicy::new(2);
        let mut ledger = QuarantineLedger::new();
        policy.on_failure(&mut ledger, "a");
        policy.on_failure(&mut ledger, "a"); // a quarantined
        policy.on_failure(&mut ledger, "b"); // b: one failure only
        assert!(ledger.is_quarantined("a"));
        assert!(!ledger.is_quarantined("b"));
        assert_eq!(ledger.quarantined_count(), 1);
    }

    #[test]
    fn release_all_rearms_every_fingerprint() {
        let policy = QuarantinePolicy::new(1);
        let mut ledger = QuarantineLedger::new();
        policy.on_failure(&mut ledger, "a");
        policy.on_failure(&mut ledger, "b");
        assert_eq!(ledger.quarantined_count(), 2);
        ledger.release_all();
        assert_eq!(ledger.quarantined_count(), 0);
        assert_eq!(ledger.failures("a"), 0);
    }

    #[test]
    fn threshold_of_one_quarantines_on_first_failure() {
        let policy = QuarantinePolicy::new(1);
        let mut ledger = QuarantineLedger::new();
        assert!(policy.on_failure(&mut ledger, FP));
        assert!(ledger.is_quarantined(FP));
    }

    #[test]
    fn default_is_disabled() {
        assert!(!QuarantinePolicy::default().is_active());
        assert_eq!(QuarantinePolicy::default().threshold(), 0);
    }
}
