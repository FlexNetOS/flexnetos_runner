//! Windowed dispatch rate limit + per-route failure cooldown (adapted from
//! `Conway-Research/automaton`'s hourly/daily call caps and its **"5-minute backoff on error"**
//! per-endpoint penalty).
//!
//! This is the **timing** complement to the runner's existing volume guards, each of which answers a
//! different question:
//! - the **loop breaker** ([`crate::loopguard`]) — is the *same* job recurring in a short window?
//! - the **governor** ([`crate::governor`]) — has the *lifetime* job/token/USD budget been spent?
//! - **quarantine** ([`crate::quarantine`]) — has *this fingerprint* failed terminally too often?
//!
//! None of them bounds the **rate** of *distinct, in-budget, non-looping* dispatches — a burst of
//! many different valid jobs in a few seconds sails through every existing gate. The rate limiter is
//! that bound: a rolling **window cap** (at most `max_per_window` dispatches in any `window_secs`
//! span) plus a **per-route cooldown** (after a route — a job [`class`](crate::jobspec::JobKind::class)
//! — fails, hold further dispatches of that route for `route_cooldown_secs`, automaton's error
//! backoff). A rate refusal is *not* a job failure: it carries a **retry-after** so the orchestrator
//! simply re-dispatches later (it never consumes the recovery retry budget or escalates to a human).
//!
//! **Clock-injected, so `runner-core` stays clock-free.** Exactly like [`crate::deadline`] (which
//! computes a `Duration` and lets the binary measure wall-clock), this module never reads a clock:
//! every entry point takes a monotonic `now_secs` the caller supplies. The dispatcher passes a
//! monotonic reading; tests pass explicit timestamps and advance them by hand — no sleeping, fully
//! deterministic. Delegate-only and orthogonal to model routing (weave's domain): the limiter decides
//! only *whether more may dispatch right now*, never *which* model runs.
//!
//! **Behaviour-preserving default:** with no window cap and no cooldown configured the limiter is
//! inert — every [`RateLimiter::check`] admits, allocates nothing, and the dispatch path is unchanged.

use std::collections::{HashMap, VecDeque};

/// Operator configuration for the rate limiter. A `0`/absent dimension is *off* (the env convention
/// shared with the budget/deadline knobs), so the all-zero policy is the behaviour-preserving inert
/// default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateLimitPolicy {
    /// Max dispatches admitted in any rolling `window_secs` span. `None` = the window cap is off.
    max_per_window: Option<u32>,
    /// The rolling window width in seconds (clamped to ≥1). Only meaningful when `max_per_window` is set.
    window_secs: u64,
    /// After a route fails, hold further dispatches of that route for this many seconds. `None` = off.
    route_cooldown_secs: Option<u64>,
}

impl RateLimitPolicy {
    /// Build a policy. `max_per_window`/`route_cooldown_secs` of `0` mean "that dimension is off";
    /// `window_secs` is clamped to ≥1 (a zero-width window is meaningless).
    pub fn new(max_per_window: u32, window_secs: u64, route_cooldown_secs: u64) -> Self {
        Self {
            max_per_window: (max_per_window > 0).then_some(max_per_window),
            window_secs: window_secs.max(1),
            route_cooldown_secs: (route_cooldown_secs > 0).then_some(route_cooldown_secs),
        }
    }

    /// The fully-inert policy (no window cap, no cooldown) — the default.
    pub fn disabled() -> Self {
        Self::new(0, 1, 0)
    }

    /// Build from operator env values (`FXRUN_RATE_MAX` dispatches per `FXRUN_RATE_WINDOW_SECS`;
    /// `FXRUN_ROUTE_COOLDOWN_SECS` per-route failure backoff). A `window_secs` of 0 falls back to 60s.
    pub fn from_env(max_per_window: u32, window_secs: u64, route_cooldown_secs: u64) -> Self {
        let window = if window_secs == 0 { 60 } else { window_secs };
        Self::new(max_per_window, window, route_cooldown_secs)
    }

    /// Whether any dimension is configured (i.e. the limiter will ever deny).
    pub fn is_active(&self) -> bool {
        self.max_per_window.is_some() || self.route_cooldown_secs.is_some()
    }

    /// The configured window cap, if any.
    pub fn max_per_window(&self) -> Option<u32> {
        self.max_per_window
    }

    /// The rolling window width in seconds.
    pub fn window_secs(&self) -> u64 {
        self.window_secs
    }

    /// The configured per-route failure cooldown in seconds, if any.
    pub fn route_cooldown_secs(&self) -> Option<u64> {
        self.route_cooldown_secs
    }
}

impl Default for RateLimitPolicy {
    fn default() -> Self {
        Self::disabled()
    }
}

/// The limiter's admission decision for one dispatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RateDecision {
    /// Under the window cap and the route is not cooling down — dispatch may proceed (slot reserved).
    Admit,
    /// Refused for timing reasons. Carries the reason and a **retry-after**: how many seconds until a
    /// retry could plausibly be admitted (the window slot frees, or the cooldown elapses). A timing
    /// refusal, *not* a job failure — the orchestrator re-dispatches after the hint.
    Denied {
        reason: String,
        retry_after_secs: u64,
    },
}

impl RateDecision {
    /// Whether dispatch was refused.
    pub fn is_denied(&self) -> bool {
        matches!(self, RateDecision::Denied { .. })
    }

    /// The retry-after hint (0 when admitted).
    pub fn retry_after_secs(&self) -> u64 {
        match self {
            RateDecision::Admit => 0,
            RateDecision::Denied {
                retry_after_secs, ..
            } => *retry_after_secs,
        }
    }
}

/// Stateful windowed rate limiter + per-route cooldown ledger. Held by the dispatcher across
/// connections (like the breaker's window and the governor's spend). Clock-free: every method takes
/// the caller's monotonic `now_secs`.
#[derive(Debug, Clone)]
pub struct RateLimiter {
    policy: RateLimitPolicy,
    /// Admit timestamps within the current window (ascending; pruned on each check).
    recent: VecDeque<u64>,
    /// Route → the `now_secs` at which its failure cooldown expires.
    cooldown_until: HashMap<String, u64>,
}

impl RateLimiter {
    /// A limiter governed by `policy`.
    pub fn new(policy: RateLimitPolicy) -> Self {
        Self {
            policy,
            recent: VecDeque::new(),
            cooldown_until: HashMap::new(),
        }
    }

    /// A fully-inert limiter (the behaviour-preserving default).
    pub fn disabled() -> Self {
        Self::new(RateLimitPolicy::disabled())
    }

    /// The governing policy.
    pub fn policy(&self) -> RateLimitPolicy {
        self.policy
    }

    /// Drop window entries older than the rolling window relative to `now_secs`.
    fn prune(&mut self, now_secs: u64) {
        let window = self.policy.window_secs;
        while let Some(&front) = self.recent.front() {
            // An entry is in-window while `front + window > now`; otherwise it has aged out.
            if front.saturating_add(window) <= now_secs {
                self.recent.pop_front();
            } else {
                break;
            }
        }
    }

    /// The pre-dispatch timing gate for a dispatch of `route` at `now_secs`. Checks the per-route
    /// cooldown first (a route in the penalty box is refused regardless of headroom), then the rolling
    /// window cap. On [`RateDecision::Admit`] a window slot is reserved; a denial reserves nothing.
    pub fn check(&mut self, route: &str, now_secs: u64) -> RateDecision {
        // (1) Per-route failure cooldown (automaton's error backoff).
        if let Some(&until) = self.cooldown_until.get(route) {
            if now_secs < until {
                return RateDecision::Denied {
                    reason: format!(
                        "route `{route}` is in failure cooldown for {}s more (automaton error backoff)",
                        until - now_secs
                    ),
                    retry_after_secs: until - now_secs,
                };
            }
            // Cooldown elapsed — release it so the map doesn't grow unbounded.
            self.cooldown_until.remove(route);
        }

        // (2) Rolling window cap.
        if let Some(max) = self.policy.max_per_window {
            self.prune(now_secs);
            if self.recent.len() as u32 >= max {
                // The window is full; the soonest slot frees when the oldest in-window entry ages out.
                let retry_after = self
                    .recent
                    .front()
                    .map(|&front| {
                        front
                            .saturating_add(self.policy.window_secs)
                            .saturating_sub(now_secs)
                    })
                    .unwrap_or(self.policy.window_secs)
                    .max(1);
                return RateDecision::Denied {
                    reason: format!(
                        "dispatch rate limit: {max} per {}s window reached",
                        self.policy.window_secs
                    ),
                    retry_after_secs: retry_after,
                };
            }
            self.recent.push_back(now_secs);
        }

        RateDecision::Admit
    }

    /// Record that a dispatch of `route` failed at `now_secs`, starting (or extending) its cooldown so
    /// subsequent dispatches of that route are held for `route_cooldown_secs`. No-op when no cooldown
    /// is configured.
    pub fn record_failure(&mut self, route: &str, now_secs: u64) {
        if let Some(cooldown) = self.policy.route_cooldown_secs {
            self.cooldown_until
                .insert(route.to_string(), now_secs.saturating_add(cooldown));
        }
    }

    /// Clear a route's cooldown — called after a clean delegation of that route, so one success
    /// releases the penalty early (the route is healthy again).
    pub fn clear_route(&mut self, route: &str) {
        self.cooldown_until.remove(route);
    }

    /// How many dispatches are currently counted in the window at `now_secs` (prunes first).
    pub fn in_window(&mut self, now_secs: u64) -> usize {
        self.prune(now_secs);
        self.recent.len()
    }

    /// How many routes are currently cooling down (for observability / tests; does not prune expired
    /// entries — those are released lazily on the next [`check`](Self::check) of that route).
    pub fn cooling_routes(&self) -> usize {
        self.cooldown_until.len()
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::disabled()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_limiter_admits_everything() {
        let mut rl = RateLimiter::disabled();
        assert!(!rl.policy().is_active());
        for t in 0..100 {
            assert_eq!(rl.check("ci", t), RateDecision::Admit);
        }
    }

    #[test]
    fn window_cap_admits_up_to_max_then_denies_within_the_window() {
        // 2 dispatches per 10s window.
        let mut rl = RateLimiter::new(RateLimitPolicy::new(2, 10, 0));
        assert_eq!(rl.check("ci", 0), RateDecision::Admit); // 1/2
        assert_eq!(rl.check("ci", 1), RateDecision::Admit); // 2/2
        let d = rl.check("ci", 2); // 3rd within the window → denied
        assert!(d.is_denied());
        // Oldest entry (t=0) ages out at t=10 → retry-after = 10 - 2 = 8.
        assert_eq!(d.retry_after_secs(), 8);
    }

    #[test]
    fn window_slides_so_capacity_returns_after_entries_age_out() {
        let mut rl = RateLimiter::new(RateLimitPolicy::new(2, 10, 0));
        assert_eq!(rl.check("ci", 0), RateDecision::Admit); // t=0
        assert_eq!(rl.check("ci", 5), RateDecision::Admit); // t=5
        assert!(rl.check("ci", 6).is_denied()); // full (t=0 and t=5 both in window)
                                                // At t=10 the t=0 entry has aged out (0 + 10 <= 10) → one slot frees.
        assert_eq!(rl.check("ci", 10), RateDecision::Admit);
        assert_eq!(rl.in_window(10), 2); // t=5 and t=10
    }

    #[test]
    fn denied_dispatch_does_not_consume_a_window_slot() {
        let mut rl = RateLimiter::new(RateLimitPolicy::new(1, 10, 0));
        assert_eq!(rl.check("ci", 0), RateDecision::Admit);
        assert!(rl.check("ci", 1).is_denied());
        assert!(rl.check("ci", 2).is_denied());
        // Only the single admit is counted; the denials reserved nothing.
        assert_eq!(rl.in_window(2), 1);
    }

    #[test]
    fn route_cooldown_holds_a_failed_route_then_releases() {
        // No window cap; 30s per-route cooldown.
        let mut rl = RateLimiter::new(RateLimitPolicy::new(0, 1, 30));
        assert_eq!(rl.check("review", 0), RateDecision::Admit);
        rl.record_failure("review", 5); // fails at t=5 → cooldown until t=35
        let d = rl.check("review", 10);
        assert!(d.is_denied());
        assert_eq!(d.retry_after_secs(), 25); // 35 - 10
                                              // A *different* route is unaffected.
        assert_eq!(rl.check("ci", 10), RateDecision::Admit);
        // After the cooldown elapses the route is admitted again.
        assert_eq!(rl.check("review", 35), RateDecision::Admit);
        assert_eq!(rl.cooling_routes(), 0); // released lazily on the admitting check
    }

    #[test]
    fn clear_route_releases_the_cooldown_on_success() {
        let mut rl = RateLimiter::new(RateLimitPolicy::new(0, 1, 60));
        rl.record_failure("agent", 0);
        assert!(rl.check("agent", 1).is_denied());
        rl.clear_route("agent"); // a later clean delegation
        assert_eq!(rl.check("agent", 2), RateDecision::Admit);
        assert_eq!(rl.cooling_routes(), 0);
    }

    #[test]
    fn cooldown_is_checked_before_the_window_so_a_cooling_route_is_refused_with_headroom() {
        // Plenty of window headroom, but the route is cooling down → cooldown wins.
        let mut rl = RateLimiter::new(RateLimitPolicy::new(100, 10, 30));
        rl.record_failure("ci", 0);
        match rl.check("ci", 1) {
            RateDecision::Denied { reason, .. } => assert!(reason.contains("cooldown")),
            RateDecision::Admit => {
                panic!("a cooling route must be refused even with window headroom")
            }
        }
    }

    #[test]
    fn from_env_zero_window_defaults_to_sixty_and_zeros_disable() {
        assert_eq!(RateLimitPolicy::from_env(5, 0, 0).window_secs(), 60);
        assert!(!RateLimitPolicy::from_env(0, 0, 0).is_active());
        assert_eq!(
            RateLimitPolicy::from_env(5, 30, 0).max_per_window(),
            Some(5)
        );
        assert_eq!(
            RateLimitPolicy::from_env(0, 0, 300).route_cooldown_secs(),
            Some(300)
        );
    }

    #[test]
    fn window_is_clamped_to_at_least_one_second() {
        assert_eq!(RateLimitPolicy::new(5, 0, 0).window_secs(), 1);
    }
}
