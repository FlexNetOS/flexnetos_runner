//! Dispatch provenance / authority gate.
//!
//! This is the runner-plane analogue of automaton's authority derivation plus the access-broker
//! pattern from attractor/Archon: before a privileged route is even examined for content, the runner
//! can require that the envelope identify *who* submitted it and that the submitter's authority tier
//! meets the route's configured floor. The signed [`JobSpec`](crate::jobspec::JobSpec) still proves
//! *what* work was authorized; this envelope seam proves *who* is asking the runner to exercise a
//! privileged verb/class now.
//!
//! The policy is opt-in and fail-closed per route. With no configured floors, older App frames with
//! no submitter stay byte-compatible and pass unchanged.

use crate::jobspec::{JobKind, JobSpec};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Ordered authority tiers. Higher tiers satisfy lower floors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorityTier {
    /// Unknown / unauthenticated envelope provenance. This is the implicit tier when no submitter is
    /// supplied; it never satisfies a configured floor above `guest`.
    Guest,
    /// An automated agent identity.
    Agent,
    /// A maintainer / trusted automation identity.
    Maintainer,
    /// Repository/org owner or root automation.
    Owner,
}

impl AuthorityTier {
    pub fn as_str(self) -> &'static str {
        match self {
            AuthorityTier::Guest => "guest",
            AuthorityTier::Agent => "agent",
            AuthorityTier::Maintainer => "maintainer",
            AuthorityTier::Owner => "owner",
        }
    }
}

impl fmt::Display for AuthorityTier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for AuthorityTier {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "guest" | "unknown" | "none" => Ok(AuthorityTier::Guest),
            "agent" | "bot" | "automation" => Ok(AuthorityTier::Agent),
            "maintainer" | "trusted" => Ok(AuthorityTier::Maintainer),
            "owner" | "admin" | "root" | "system" => Ok(AuthorityTier::Owner),
            other => Err(format!("unknown authority tier `{other}`")),
        }
    }
}

/// Envelope provenance: who submitted this dispatch and which tier the App/weave assigned it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Submitter {
    /// Stable identity label (GitHub login, App installation slug, weave actor id, etc.).
    pub id: String,
    /// Authority tier derived by the control plane.
    pub tier: AuthorityTier,
}

impl Submitter {
    pub fn new(id: impl Into<String>, tier: AuthorityTier) -> Self {
        Self {
            id: id.into(),
            tier,
        }
    }
}

/// Configured minimum authority tier by route class. `None` means no authority gate for that route.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AuthorityPolicy {
    ci: Option<AuthorityTier>,
    review: Option<AuthorityTier>,
    agent: Option<AuthorityTier>,
    cycle: Option<AuthorityTier>,
}

impl AuthorityPolicy {
    pub fn disabled() -> Self {
        Self::default()
    }

    pub fn is_active(&self) -> bool {
        self.ci.is_some() || self.review.is_some() || self.agent.is_some() || self.cycle.is_some()
    }

    pub fn require(mut self, class: &str, tier: AuthorityTier) -> Result<Self, String> {
        match normalize_class(class).as_deref() {
            Some("ci") => self.ci = Some(tier),
            Some("review") => self.review = Some(tier),
            Some("agent") => self.agent = Some(tier),
            Some("cycle") => self.cycle = Some(tier),
            _ => return Err(format!("unknown route class `{class}`")),
        }
        Ok(self)
    }

    /// Parse comma-separated route floors, e.g. `cycle=maintainer,agent=owner`.
    pub fn from_rules(rules: &str) -> Result<Self, String> {
        let mut policy = Self::disabled();
        for rule in rules.split(',').map(str::trim).filter(|r| !r.is_empty()) {
            let (class, tier) = rule
                .split_once('=')
                .ok_or_else(|| format!("authority rule `{rule}` must be class=tier"))?;
            policy = policy.require(class, tier.parse()?)?;
        }
        Ok(policy)
    }

    pub fn min_for(&self, job: &JobSpec) -> Option<AuthorityTier> {
        match &job.job {
            JobKind::Ci { .. } => self.ci,
            JobKind::ReviewGate { .. } => self.review,
            JobKind::AgentTask { .. } => self.agent,
            JobKind::LoopCycle { .. } => self.cycle,
        }
    }

    pub fn check(&self, job: &JobSpec, submitter: Option<&Submitter>) -> AuthorityDecision {
        let Some(required) = self.min_for(job) else {
            return AuthorityDecision::Allowed;
        };
        let actual = submitter.map(|s| s.tier).unwrap_or(AuthorityTier::Guest);
        if actual >= required {
            AuthorityDecision::Allowed
        } else {
            AuthorityDecision::Denied {
                route: job.job.class(),
                required,
                actual,
                submitter: submitter.map(|s| s.id.clone()),
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
                parts.push(format!("{class}>={tier}"));
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
pub enum AuthorityDecision {
    Allowed,
    Denied {
        route: &'static str,
        required: AuthorityTier,
        actual: AuthorityTier,
        submitter: Option<String>,
    },
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

    fn cycle() -> JobSpec {
        JobSpec {
            id: "job-1".into(),
            correlation_id: "c".into(),
            from_fork: false,
            job: JobKind::LoopCycle {
                repo: "FlexNetOS/meta".into(),
                task_id: "T-1".into(),
            },
        }
    }

    #[test]
    fn disabled_policy_allows_legacy_frames_without_submitter() {
        assert_eq!(
            AuthorityPolicy::disabled().check(&cycle(), None),
            AuthorityDecision::Allowed
        );
    }

    #[test]
    fn configured_route_floor_denies_missing_or_low_submitter() {
        let policy = AuthorityPolicy::from_rules("cycle=maintainer").unwrap();
        assert!(matches!(
            policy.check(&cycle(), None),
            AuthorityDecision::Denied {
                actual: AuthorityTier::Guest,
                ..
            }
        ));
        assert!(matches!(
            policy.check(&cycle(), Some(&Submitter::new("bot", AuthorityTier::Agent))),
            AuthorityDecision::Denied {
                actual: AuthorityTier::Agent,
                required: AuthorityTier::Maintainer,
                ..
            }
        ));
    }

    #[test]
    fn configured_route_floor_allows_equal_or_higher_tier() {
        let policy = AuthorityPolicy::from_rules("cycle=maintainer,agent=owner").unwrap();
        assert_eq!(
            policy.check(
                &cycle(),
                Some(&Submitter::new("alice", AuthorityTier::Maintainer))
            ),
            AuthorityDecision::Allowed
        );
        assert_eq!(
            policy.check(
                &cycle(),
                Some(&Submitter::new("root", AuthorityTier::Owner))
            ),
            AuthorityDecision::Allowed
        );
    }

    #[test]
    fn rule_parser_accepts_aliases_and_rejects_unknowns() {
        assert_eq!(
            AuthorityPolicy::from_rules("loop=trusted")
                .unwrap()
                .describe(),
            "cycle>=maintainer"
        );
        assert!(AuthorityPolicy::from_rules("wat=owner").is_err());
        assert!(AuthorityPolicy::from_rules("cycle").is_err());
    }
}
