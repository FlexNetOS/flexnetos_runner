//! History-calibrated pre-route risk score (adapted from `kclaw0`'s `path-simulator.js`, which blends
//! a **static base rate** with the **live failure rate observed from the event ledger** to predict how
//! risky an action is *before* taking it).
//!
//! Every other guard the runner has is a hard *gate* — it admits or refuses. This is the one
//! **advice-only** signal: it never blocks a dispatch. It computes, from a fingerprint's own
//! success/failure history, a smoothed probability that *this* dispatch will fail, and attaches it to
//! the audit event so the orchestrator / weave can act on it (bias toward approval, pre-emptive
//! escalation, or — weave's call — a more conservative model). It is the soft, continuous companion to
//! the hard latches: the breaker trips on identical-loop *volume*, quarantine latches at a failure
//! *count*; the risk score is the *gradient* between "healthy" and "quarantined" that lets a consumer
//! react **before** the latch fires.
//!
//! ## Why a smoothed rate, not a raw one
//! A raw failure rate is useless early: one failure in one attempt reads as "100% risk", one success
//! as "0%". `path-simulator.js` blends a prior. We use a Beta-style smoothing: with a `base_rate`
//! prior of strength `prior_strength` (in pseudo-observations),
//!
//! ```text
//!   score = (failures + base_rate * prior_strength) / (total + prior_strength)
//! ```
//!
//! With **no history** the score is exactly `base_rate` (the static estimate). As real observations
//! accumulate they dominate the prior, so the score converges on the *observed* failure rate. This is
//! the "static base rate blended with live evidence" of the source, made statistically honest.
//!
//! ## Delegate-only, clock-free, opt-in
//! Pure arithmetic over counts — no I/O, no clock, no model routing (weave's domain). The
//! [`RiskLedger`] accumulates per-fingerprint outcomes across dispatches (unlike the retry/quarantine
//! ledgers it is **not** cleared on success — calibration needs the whole history). A [`RiskPolicy`]
//! gates whether the score is computed and surfaced at all; **disabled by default**, so the audit
//! output is byte-for-byte unchanged until an operator opts in (`FXRUN_RISK_ANNOTATE`).

use serde::Serialize;
use std::collections::HashMap;

/// A coarse risk classification for banners / quick filtering, derived from the continuous score.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskBand {
    /// Below [`RiskModel::ELEVATED_AT`] — proceed normally.
    Low,
    /// Between the two thresholds — worth surfacing; a consumer may bias conservative.
    Elevated,
    /// At or above [`RiskModel::HIGH_AT`] — this work fails often; strongly consider human review.
    High,
}

impl RiskBand {
    /// A short lowercase label for audit details / banners.
    pub fn label(&self) -> &'static str {
        match self {
            RiskBand::Low => "low",
            RiskBand::Elevated => "elevated",
            RiskBand::High => "high",
        }
    }
}

impl std::fmt::Display for RiskBand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// A computed risk assessment for one fingerprint: the smoothed failure probability, the number of
/// real observations behind it, and the coarse band. Serializable so it rides on the audit event.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct RiskScore {
    /// Smoothed failure probability in `[0.0, 1.0]` (see the module docs).
    pub score: f64,
    /// How many real observations (successes + failures) informed it — the *confidence* in `score`.
    pub samples: u32,
    /// The coarse band derived from `score`.
    pub band: RiskBand,
}

impl RiskScore {
    /// A compact one-line summary for an audit detail.
    pub fn summary(&self) -> String {
        format!(
            "risk: {} ({:.2}, {} sample{})",
            self.band,
            self.score,
            self.samples,
            if self.samples == 1 { "" } else { "s" }
        )
    }
}

/// The risk model: a static `base_rate` prior of strength `prior_strength`, blended with observed
/// counts. Pure configuration; [`assess`](Self::assess) does the arithmetic.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RiskModel {
    base_rate: f64,
    prior_strength: f64,
}

impl RiskModel {
    /// `score >= ELEVATED_AT` is [`RiskBand::Elevated`].
    pub const ELEVATED_AT: f64 = 0.25;
    /// `score >= HIGH_AT` is [`RiskBand::High`].
    pub const HIGH_AT: f64 = 0.5;

    /// Build a model. `base_rate` is clamped to `[0,1]`; `prior_strength` to `>= 0`.
    pub fn new(base_rate: f64, prior_strength: f64) -> Self {
        Self {
            base_rate: base_rate.clamp(0.0, 1.0),
            prior_strength: prior_strength.max(0.0),
        }
    }

    /// The default model: a 10% base failure prior worth 4 pseudo-observations (so it takes a few real
    /// results to move the needle, but real evidence wins quickly).
    pub fn standard() -> Self {
        Self::new(0.1, 4.0)
    }

    /// The configured base rate (the score with no history).
    pub fn base_rate(&self) -> f64 {
        self.base_rate
    }

    /// Classify a smoothed score into a band.
    fn band_for(score: f64) -> RiskBand {
        if score >= Self::HIGH_AT {
            RiskBand::High
        } else if score >= Self::ELEVATED_AT {
            RiskBand::Elevated
        } else {
            RiskBand::Low
        }
    }

    /// Assess `successes`/`failures` into a [`RiskScore`] via Beta-style smoothing toward `base_rate`.
    pub fn assess(&self, successes: u32, failures: u32) -> RiskScore {
        let total = successes as f64 + failures as f64;
        let score = (failures as f64 + self.base_rate * self.prior_strength)
            / (total + self.prior_strength);
        RiskScore {
            score,
            samples: successes + failures,
            band: Self::band_for(score),
        }
    }
}

impl Default for RiskModel {
    fn default() -> Self {
        Self::standard()
    }
}

/// Per-fingerprint outcome history `(successes, failures)`, accumulated across dispatches. Held by the
/// dispatcher across connections (like the retry/quarantine ledgers) — but **not** cleared on success,
/// since calibration needs the whole record.
#[derive(Debug, Clone, Default)]
pub struct RiskLedger {
    history: HashMap<String, (u32, u32)>,
}

impl RiskLedger {
    /// A fresh, empty ledger.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record one outcome for `fingerprint` (`success = true` for a clean delegation).
    pub fn record(&mut self, fingerprint: &str, success: bool) {
        let entry = self
            .history
            .entry(fingerprint.to_string())
            .or_insert((0, 0));
        if success {
            entry.0 += 1;
        } else {
            entry.1 += 1;
        }
    }

    /// The `(successes, failures)` recorded for `fingerprint` (`(0, 0)` if unseen).
    pub fn history(&self, fingerprint: &str) -> (u32, u32) {
        self.history.get(fingerprint).copied().unwrap_or((0, 0))
    }

    /// Number of fingerprints tracked (observability / tests).
    pub fn tracked(&self) -> usize {
        self.history.len()
    }
}

/// Whether risk scoring is enabled, and the model to use. Disabled by default (the audit stream is
/// unchanged until an operator opts in).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RiskPolicy {
    enabled: bool,
    model: RiskModel,
}

impl RiskPolicy {
    /// An enabled policy using `model`.
    pub fn enabled(model: RiskModel) -> Self {
        Self {
            enabled: true,
            model,
        }
    }

    /// The behaviour-preserving default: scoring off (no score computed or surfaced).
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            model: RiskModel::standard(),
        }
    }

    /// Build from an operator flag string: a truthy `FXRUN_RISK_ANNOTATE` (`1`/`true`/`yes`/`on`)
    /// enables scoring with the standard model; anything else disables it.
    pub fn from_env(flag: &str) -> Self {
        let on = matches!(
            flag.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        );
        if on {
            Self::enabled(RiskModel::standard())
        } else {
            Self::disabled()
        }
    }

    /// Whether scoring is on.
    pub fn is_active(&self) -> bool {
        self.enabled
    }

    /// Assess `fingerprint` against its recorded history — `None` when scoring is disabled (so the
    /// caller attaches nothing and the audit output is unchanged).
    pub fn assess(&self, ledger: &RiskLedger, fingerprint: &str) -> Option<RiskScore> {
        if !self.enabled {
            return None;
        }
        let (s, f) = ledger.history(fingerprint);
        Some(self.model.assess(s, f))
    }
}

impl Default for RiskPolicy {
    fn default() -> Self {
        Self::disabled()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_history_scores_at_the_base_rate() {
        let m = RiskModel::new(0.1, 4.0);
        let r = m.assess(0, 0);
        assert!((r.score - 0.1).abs() < 1e-9);
        assert_eq!(r.samples, 0);
        assert_eq!(r.band, RiskBand::Low);
    }

    #[test]
    fn persistent_failure_drives_the_score_up_into_high() {
        let m = RiskModel::new(0.1, 4.0);
        // 0 successes, 10 failures → (10 + 0.4) / (10 + 4) ≈ 0.74.
        let r = m.assess(0, 10);
        assert!(
            r.score > RiskModel::HIGH_AT,
            "score {} should be high",
            r.score
        );
        assert_eq!(r.band, RiskBand::High);
        assert_eq!(r.samples, 10);
    }

    #[test]
    fn consistent_success_keeps_the_score_low() {
        let m = RiskModel::new(0.1, 4.0);
        let r = m.assess(20, 0);
        assert!(r.score < RiskModel::ELEVATED_AT);
        assert_eq!(r.band, RiskBand::Low);
    }

    #[test]
    fn a_mixed_record_lands_in_the_elevated_band() {
        let m = RiskModel::new(0.1, 4.0);
        // 6 successes, 6 failures → (6 + 0.4)/(12+4) = 0.4 → elevated.
        let r = m.assess(6, 6);
        assert_eq!(r.band, RiskBand::Elevated);
    }

    #[test]
    fn the_prior_smooths_a_tiny_sample() {
        // One failure in one attempt is NOT 100% risk thanks to the prior.
        let m = RiskModel::new(0.1, 4.0);
        let r = m.assess(0, 1);
        assert!(
            r.score < 0.5,
            "one failure shouldn't read as high risk: {}",
            r.score
        );
    }

    #[test]
    fn base_rate_and_prior_are_clamped() {
        assert_eq!(RiskModel::new(2.0, -5.0).base_rate(), 1.0);
        // prior_strength clamped to 0 → score is the raw rate.
        let r = RiskModel::new(0.1, -5.0).assess(0, 1);
        assert!((r.score - 1.0).abs() < 1e-9);
    }

    #[test]
    fn ledger_accumulates_and_does_not_clear_on_success() {
        let mut l = RiskLedger::new();
        l.record("fp", false);
        l.record("fp", true);
        l.record("fp", true);
        assert_eq!(l.history("fp"), (2, 1));
        assert_eq!(l.tracked(), 1);
        assert_eq!(l.history("other"), (0, 0));
    }

    #[test]
    fn disabled_policy_assesses_to_none() {
        let l = RiskLedger::new();
        assert!(RiskPolicy::disabled().assess(&l, "fp").is_none());
        assert!(!RiskPolicy::disabled().is_active());
    }

    #[test]
    fn enabled_policy_reads_history_through_the_ledger() {
        let mut l = RiskLedger::new();
        for _ in 0..8 {
            l.record("fp", false);
        }
        let score = RiskPolicy::enabled(RiskModel::standard())
            .assess(&l, "fp")
            .expect("enabled → Some");
        assert_eq!(score.band, RiskBand::High);
    }

    #[test]
    fn from_env_parses_truthy_flags() {
        assert!(RiskPolicy::from_env("1").is_active());
        assert!(RiskPolicy::from_env("true").is_active());
        assert!(RiskPolicy::from_env("ON").is_active());
        assert!(!RiskPolicy::from_env("0").is_active());
        assert!(!RiskPolicy::from_env("").is_active());
    }

    #[test]
    fn score_summary_reads_cleanly() {
        let r = RiskModel::standard().assess(0, 1);
        let s = r.summary();
        assert!(s.starts_with("risk: "));
        assert!(s.contains("1 sample)"));
    }
}
