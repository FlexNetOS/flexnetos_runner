//! Kernel router (ADR-0008 §2/S5): map a job to the existing kernel that executes it.
//!
//! **Delegate-only.** It produces a typed [`KernelPlan`] (which kernel, what intent)
//! WITHOUT running anything. The dispatcher (P2) turns a plan into a subprocess call; the
//! runner NEVER reimplements loop_lib / atc / handoff / weave.

use crate::agent::Agent;
use crate::jobspec::{JobKind, JobSpec};

/// The existing kernels the runner delegates to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kernel {
    LoopLib,
    Atc,
    Handoff,
    Weave,
}

impl Kernel {
    /// All canonical kernels, in stable display order.
    pub const ALL: [Kernel; 4] = [Kernel::LoopLib, Kernel::Atc, Kernel::Handoff, Kernel::Weave];

    /// The canonical binary name for this kernel.
    pub fn program(&self) -> &'static str {
        match self {
            Kernel::LoopLib => "loop",
            Kernel::Atc => "atc",
            Kernel::Handoff => "hf",
            Kernel::Weave => "weave",
        }
    }

    /// Parse an operator-facing kernel name / alias for target allowlists.
    pub fn parse(name: &str) -> Result<Self, String> {
        match name.trim().to_ascii_lowercase().as_str() {
            "loop" | "loop_lib" | "loop-lib" | "looplib" => Ok(Kernel::LoopLib),
            "atc" => Ok(Kernel::Atc),
            "hf" | "handoff" => Ok(Kernel::Handoff),
            "weave" => Ok(Kernel::Weave),
            other => Err(format!("unknown kernel target `{other}`")),
        }
    }
}

impl std::fmt::Display for Kernel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.program())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KernelPlan {
    pub kernel: Kernel,
    /// Stable route candidate id (auditable selector witness).
    pub route_id: String,
    /// Route candidate weight. Higher wins; `route_id` breaks ties ascending.
    pub route_weight: i32,
    /// Human-readable intent; exact argv is finalized by the dispatcher in P2.
    pub intent: String,
    pub repo: String,
    /// For agent-class jobs (kernel = `atc`), which agent backend `atc` drives. `None` for
    /// non-agent kernels (`loop`/`hf`/`weave`), which have no agent backend.
    pub agent: Option<Agent>,
}

/// One possible route for a job. The selector is total and deterministic: highest weight wins;
/// ties resolve by lexicographically-smallest id (attractor-style witnessed route selection).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteCandidate {
    pub id: String,
    pub weight: i32,
    pub plan: KernelPlan,
}

impl RouteCandidate {
    pub fn new(id: impl Into<String>, weight: i32, mut plan: KernelPlan) -> Self {
        let id = id.into();
        plan.route_id = id.clone();
        plan.route_weight = weight;
        Self { id, weight, plan }
    }
}

/// Select one candidate by a stable total order: `weight DESC, id ASC`.
pub fn select_route(candidates: impl IntoIterator<Item = RouteCandidate>) -> Option<KernelPlan> {
    candidates
        .into_iter()
        .min_by(|a, b| b.weight.cmp(&a.weight).then_with(|| a.id.cmp(&b.id)))
        .map(|c| c.plan)
}

fn plan(
    kernel: Kernel,
    route_id: &str,
    weight: i32,
    intent: String,
    repo: String,
    agent: Option<Agent>,
) -> KernelPlan {
    KernelPlan {
        kernel,
        route_id: route_id.to_string(),
        route_weight: weight,
        intent,
        repo,
        agent,
    }
}

/// Route a job to its kernel. Pure. Today each job kind has one candidate, but the selector contract
/// is active and tested so future multi-eligible routes are reproducible/auditable.
pub fn route(job: &JobSpec) -> KernelPlan {
    match &job.job {
        JobKind::Ci { repo, head_sha } => select_route([RouteCandidate::new(
            "ci.loop",
            100,
            plan(
                Kernel::LoopLib,
                "ci.loop",
                100,
                format!("ci build/test @ {head_sha}"),
                repo.clone(),
                None,
            ),
        )])
        .expect("one CI route candidate"),
        JobKind::ReviewGate {
            repo,
            pr_number,
            agent,
            ..
        } => select_route([RouteCandidate::new(
            "review.atc",
            100,
            plan(
                Kernel::Atc,
                "review.atc",
                100,
                format!("merge-gate review PR #{pr_number} via {agent}"),
                repo.clone(),
                Some(*agent),
            ),
        )])
        .expect("one review route candidate"),
        JobKind::AgentTask {
            repo,
            prompt_ref,
            agent,
        } => select_route([RouteCandidate::new(
            "agent.atc",
            100,
            plan(
                Kernel::Atc,
                "agent.atc",
                100,
                format!("agent task {prompt_ref} via {agent}"),
                repo.clone(),
                Some(*agent),
            ),
        )])
        .expect("one agent route candidate"),
        JobKind::LoopCycle { repo, task_id } => select_route([RouteCandidate::new(
            "cycle.handoff",
            100,
            plan(
                Kernel::Handoff,
                "cycle.handoff",
                100,
                format!("loop cycle / ship {task_id}"),
                repo.clone(),
                None,
            ),
        )])
        .expect("one cycle route candidate"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(kind: JobKind) -> JobSpec {
        JobSpec {
            id: "j".into(),
            correlation_id: "c".into(),
            from_fork: false,
            job: kind,
        }
    }

    #[test]
    fn ci_routes_to_loop_lib() {
        let p = route(&job(JobKind::Ci {
            repo: "r".into(),
            head_sha: "s".into(),
        }));
        assert_eq!(p.kernel, Kernel::LoopLib);
    }

    #[test]
    fn review_and_agent_route_to_atc() {
        assert_eq!(
            route(&job(JobKind::ReviewGate {
                repo: "r".into(),
                pr_number: 7,
                head_sha: "s".into(),
                agent: Agent::default(),
            }))
            .kernel,
            Kernel::Atc
        );
        assert_eq!(
            route(&job(JobKind::AgentTask {
                repo: "r".into(),
                prompt_ref: "p".into(),
                agent: Agent::default(),
            }))
            .kernel,
            Kernel::Atc
        );
    }

    #[test]
    fn agent_jobs_carry_the_selected_backend_into_the_plan() {
        // Default (Claude) when unspecified.
        let p = route(&job(JobKind::AgentTask {
            repo: "r".into(),
            prompt_ref: "p".into(),
            agent: Agent::default(),
        }));
        assert_eq!(p.agent, Some(Agent::Claude));
        assert!(p.intent.contains("claude"));

        // An explicit non-default backend is carried verbatim and surfaced in the intent.
        let p = route(&job(JobKind::ReviewGate {
            repo: "r".into(),
            pr_number: 7,
            head_sha: "s".into(),
            agent: Agent::Kimi,
        }));
        assert_eq!(p.agent, Some(Agent::Kimi));
        assert!(p.intent.contains("kimi"));
    }

    #[test]
    fn non_agent_kernels_have_no_agent_backend() {
        let ci = route(&job(JobKind::Ci {
            repo: "r".into(),
            head_sha: "s".into(),
        }));
        assert_eq!(ci.agent, None);
        let cycle = route(&job(JobKind::LoopCycle {
            repo: "r".into(),
            task_id: "t".into(),
        }));
        assert_eq!(cycle.agent, None);
    }

    #[test]
    fn loop_cycle_routes_to_handoff() {
        assert_eq!(
            route(&job(JobKind::LoopCycle {
                repo: "r".into(),
                task_id: "t".into(),
            }))
            .kernel,
            Kernel::Handoff
        );
    }

    #[test]
    fn route_selection_is_weight_desc_then_id_asc() {
        let mk = |id: &str, weight: i32, kernel: Kernel| {
            RouteCandidate::new(
                id,
                weight,
                plan(kernel, id, weight, format!("route {id}"), "r".into(), None),
            )
        };
        let selected = select_route([
            mk("b", 10, Kernel::Atc),
            mk("a", 10, Kernel::LoopLib),
            mk("z", 5, Kernel::Weave),
        ])
        .unwrap();
        assert_eq!(selected.kernel, Kernel::LoopLib);
        assert_eq!(selected.route_id, "a");
        assert_eq!(selected.route_weight, 10);
    }

    #[test]
    fn routed_plans_carry_selection_witness() {
        let p = route(&job(JobKind::Ci {
            repo: "r".into(),
            head_sha: "s".into(),
        }));
        assert_eq!(p.route_id, "ci.loop");
        assert_eq!(p.route_weight, 100);
    }

    #[test]
    fn program_names_are_the_real_binaries() {
        assert_eq!(Kernel::LoopLib.program(), "loop");
        assert_eq!(Kernel::Atc.program(), "atc");
        assert_eq!(Kernel::Handoff.program(), "hf");
        assert_eq!(Kernel::Weave.program(), "weave");
    }
}
