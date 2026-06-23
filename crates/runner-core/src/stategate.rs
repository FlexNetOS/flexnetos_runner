//! State-gated route admission.
//!
//! The governor's [`SurvivalTier`](crate::governor::SurvivalTier) is a read-only state signal. This
//! gate lets operators map route classes (`ci`/`review`/`agent`/`cycle`) to the worst tier they are
//! allowed to run in. It is the runner-plane analogue of automaton's idle-only / state-gated tools:
//! under load or distress, defer non-essential classes before they consume single-flight locks, rate
//! slots, loop-window entries, budget, or kernel time.
//!
//! Behaviour-preserving default: no rules means no route is state-gated.

use crate::governor::SurvivalTier;
use crate::jobspec::JobSpec;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StateGatePolicy {
    ci: Option<SurvivalTier>,
    review: Option<SurvivalTier>,
    agent: Option<SurvivalTier>,
    cycle: Option<SurvivalTier>,
}

impl StateGatePolicy {
    pub fn disabled() -> Self {
        Self::default()
    }

    pub fn is_active(&self) -> bool {
        self.ci.is_some() || self.review.is_some() || self.agent.is_some() || self.cycle.is_some()
    }

    pub fn from_rules(rules: &str) -> Result<Self, String> {
        let mut p = Self::disabled();
        for rule in rules.split(',').map(str::trim).filter(|r| !r.is_empty()) {
            let (class, tier) = rule
                .split_once('=')
                .ok_or_else(|| format!("state-gate rule `{rule}` must be class=max_tier"))?;
            p = p.allow_until(class, parse_tier(tier)?)?;
        }
        Ok(p)
    }

    pub fn allow_until(mut self, class: &str, max_tier: SurvivalTier) -> Result<Self, String> {
        match normalize_class(class).as_deref() {
            Some("ci") => self.ci = Some(max_tier),
            Some("review") => self.review = Some(max_tier),
            Some("agent") => self.agent = Some(max_tier),
            Some("cycle") => self.cycle = Some(max_tier),
            _ => return Err(format!("unknown route class `{class}`")),
        }
        Ok(self)
    }

    pub fn max_for_class(&self, class: &str) -> Option<SurvivalTier> {
        match normalize_class(class).as_deref() {
            Some("ci") => self.ci,
            Some("review") => self.review,
            Some("agent") => self.agent,
            Some("cycle") => self.cycle,
            _ => None,
        }
    }

    pub fn check(&self, job: &JobSpec, tier: SurvivalTier) -> StateGateDecision {
        let class = job.job.class();
        let Some(max_tier) = self.max_for_class(class) else {
            return StateGateDecision::Allowed;
        };
        if tier <= max_tier {
            StateGateDecision::Allowed
        } else {
            StateGateDecision::Deferred {
                class,
                current: tier,
                allowed_until: max_tier,
            }
        }
    }

    pub fn describe(&self) -> String {
        let mut parts = Vec::new();
        for (class, tier) in [
            ("ci", self.ci),
            ("review", self.review),
            ("agent", self.agent),
            ("cycle", self.cycle),
        ] {
            if let Some(tier) = tier {
                parts.push(format!("{class}<={tier}"));
            }
        }
        if parts.is_empty() {
            "off".into()
        } else {
            parts.join(",")
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateGateDecision {
    Allowed,
    Deferred {
        class: &'static str,
        current: SurvivalTier,
        allowed_until: SurvivalTier,
    },
}

fn parse_tier(s: &str) -> Result<SurvivalTier, String> {
    match s.trim().to_ascii_lowercase().as_str() {
        "full" => Ok(SurvivalTier::Full),
        "conserving" | "conserve" => Ok(SurvivalTier::Conserving),
        "distress" => Ok(SurvivalTier::Distress),
        "halted" | "halt" => Ok(SurvivalTier::Halted),
        other => Err(format!("unknown survival tier `{other}`")),
    }
}

fn normalize_class(class: &str) -> Option<String> {
    match class.trim().to_ascii_lowercase().as_str() {
        "ci" | "build" | "test" => Some("ci".into()),
        "review" | "review_gate" | "review-gate" => Some("review".into()),
        "agent" | "agent_task" | "agent-task" => Some("agent".into()),
        "cycle" | "loop" | "loop_cycle" | "loop-cycle" => Some("cycle".into()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobspec::{JobKind, JobSpec};

    fn agent_job() -> JobSpec {
        JobSpec {
            id: "j".into(),
            correlation_id: "c".into(),
            from_fork: false,
            job: JobKind::AgentTask {
                repo: "r".into(),
                prompt_ref: "p".into(),
                agent: Default::default(),
            },
        }
    }

    #[test]
    fn disabled_policy_allows_every_tier() {
        assert_eq!(
            StateGatePolicy::disabled().check(&agent_job(), SurvivalTier::Halted),
            StateGateDecision::Allowed
        );
    }

    #[test]
    fn defers_when_current_tier_exceeds_class_floor() {
        let p = StateGatePolicy::from_rules("agent=full,cycle=conserving").unwrap();
        assert!(matches!(
            p.check(&agent_job(), SurvivalTier::Conserving),
            StateGateDecision::Deferred {
                class: "agent",
                current: SurvivalTier::Conserving,
                allowed_until: SurvivalTier::Full
            }
        ));
    }

    #[test]
    fn allows_when_current_tier_is_within_class_floor() {
        let p = StateGatePolicy::from_rules("agent=conserving").unwrap();
        assert_eq!(
            p.check(&agent_job(), SurvivalTier::Full),
            StateGateDecision::Allowed
        );
        assert_eq!(
            p.check(&agent_job(), SurvivalTier::Conserving),
            StateGateDecision::Allowed
        );
    }

    #[test]
    fn parser_rejects_unknown_class_or_tier() {
        assert!(StateGatePolicy::from_rules("agent=nope").is_err());
        assert!(StateGatePolicy::from_rules("wat=full").is_err());
    }
}
