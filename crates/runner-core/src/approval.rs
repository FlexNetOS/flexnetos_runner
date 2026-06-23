//! Human-approval admission policy (adapted from `coleam00/Archon`'s `ApprovalNode` /
//! `interactive: true` and `strongdm/attractor`'s `wait.human` hexagon).
//!
//! Some work should not run unattended even when every safety gate passes — a class of jobs the
//! operator wants a human to authorize first (e.g. agent tasks that can push code, merge-gate
//! reviews on protected repos). This module decides **which job classes require approval**; the
//! cryptographic **grant** that satisfies the requirement lives on the wire envelope
//! ([`crate::wire::Approval`], an HMAC over the job fingerprint).
//!
//! Runner-plane fit: the runner is the admission choke point, so "hold until a human approves" is a
//! natural admission state here — between the budget gate and routing. It pairs with fork-PR
//! isolation (forks are already hosted-only) and with the recovery layer (a held job escalates to a
//! human for approval, exactly `wait.human`). **Delegate-only:** the runner only *holds and surfaces
//! the request* — a human / the orchestrator approves out of band and re-dispatches with a grant.
//! The runner never decides *who* may approve, only *that* this class needs one.
//!
//! **Behaviour-preserving default:** the empty policy ([`ApprovalPolicy::none`]) requires approval
//! for nothing, so dispatch is unchanged until an operator opts a band in (`FXRUN_APPROVAL_BANDS`).

use crate::jobspec::{JobKind, JobSpec};

/// Which job classes (bands) require a human approval grant before dispatch. Each flag is an
/// independent opt-in; all-false is the behaviour-preserving default.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ApprovalPolicy {
    /// `Ci` build/test jobs require approval.
    pub ci: bool,
    /// `ReviewGate` merge-gate reviews require approval.
    pub review_gates: bool,
    /// `AgentTask` jobs (an agent acting on the repo) require approval.
    pub agent_tasks: bool,
    /// `LoopCycle` (ship) jobs require approval.
    pub loop_cycles: bool,
}

impl ApprovalPolicy {
    /// A policy that requires approval for nothing (the default — dispatch unchanged).
    pub fn none() -> Self {
        Self::default()
    }

    /// Whether any band is enabled (i.e. the gate will ever hold a job).
    pub fn is_active(&self) -> bool {
        self.ci || self.review_gates || self.agent_tasks || self.loop_cycles
    }

    /// Parse a comma-separated band list (`FXRUN_APPROVAL_BANDS`), e.g. `"agent,review"`. Recognised
    /// tokens: `ci`, `review`, `agent`, `cycle` (case-insensitive, whitespace-tolerant). Unknown
    /// tokens are ignored (fail-open: an operator typo never silently blocks all dispatch).
    pub fn from_bands(spec: &str) -> Self {
        let mut p = Self::none();
        for raw in spec.split(',') {
            match raw.trim().to_ascii_lowercase().as_str() {
                "ci" => p.ci = true,
                "review" | "review_gate" | "review_gates" => p.review_gates = true,
                "agent" | "agent_task" | "agent_tasks" => p.agent_tasks = true,
                "cycle" | "loop" | "loop_cycle" | "loop_cycles" => p.loop_cycles = true,
                _ => {}
            }
        }
        p
    }

    /// Whether `job` falls in a band that requires a human approval grant.
    pub fn requires(&self, job: &JobSpec) -> bool {
        match &job.job {
            JobKind::Ci { .. } => self.ci,
            JobKind::ReviewGate { .. } => self.review_gates,
            JobKind::AgentTask { .. } => self.agent_tasks,
            JobKind::LoopCycle { .. } => self.loop_cycles,
        }
    }

    /// The enabled band names (for banners / doctor), sorted by the pipeline order.
    pub fn enabled_bands(&self) -> Vec<&'static str> {
        let mut v = Vec::new();
        if self.ci {
            v.push("ci");
        }
        if self.review_gates {
            v.push("review");
        }
        if self.agent_tasks {
            v.push("agent");
        }
        if self.loop_cycles {
            v.push("cycle");
        }
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::Agent;

    fn job(kind: JobKind) -> JobSpec {
        JobSpec {
            id: "j".into(),
            correlation_id: "c".into(),
            from_fork: false,
            job: kind,
        }
    }

    fn ci() -> JobSpec {
        job(JobKind::Ci {
            repo: "FlexNetOS/meta".into(),
            head_sha: "abc".into(),
        })
    }

    fn agent_task() -> JobSpec {
        job(JobKind::AgentTask {
            repo: "FlexNetOS/meta".into(),
            prompt_ref: "p".into(),
            agent: Agent::default(),
        })
    }

    #[test]
    fn default_policy_requires_approval_for_nothing() {
        let p = ApprovalPolicy::none();
        assert!(!p.is_active());
        assert!(!p.requires(&ci()));
        assert!(!p.requires(&agent_task()));
    }

    #[test]
    fn from_bands_parses_a_comma_list() {
        let p = ApprovalPolicy::from_bands("agent, review");
        assert!(p.agent_tasks);
        assert!(p.review_gates);
        assert!(!p.ci);
        assert!(!p.loop_cycles);
        assert!(p.is_active());
    }

    #[test]
    fn from_bands_is_case_and_whitespace_tolerant_and_ignores_unknowns() {
        let p = ApprovalPolicy::from_bands("  AGENT ,bogus,  CYCLE ");
        assert!(p.agent_tasks);
        assert!(p.loop_cycles);
        assert!(!p.review_gates);
        // A pure-garbage spec yields the inert policy (fail-open, never blocks everything).
        assert!(!ApprovalPolicy::from_bands("nonsense,,,").is_active());
    }

    #[test]
    fn requires_matches_only_the_enabled_band() {
        let p = ApprovalPolicy::from_bands("agent");
        assert!(p.requires(&agent_task()));
        assert!(!p.requires(&ci()));
    }

    #[test]
    fn enabled_bands_lists_in_pipeline_order() {
        let p = ApprovalPolicy::from_bands("cycle,ci,agent");
        assert_eq!(p.enabled_bands(), vec!["ci", "agent", "cycle"]);
    }
}
