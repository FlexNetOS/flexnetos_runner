//! Kernel router (ADR-0008 §2/S5): map a job to the existing kernel that executes it.
//!
//! **Delegate-only.** It produces a typed [`KernelPlan`] (which kernel, what intent)
//! WITHOUT running anything. The dispatcher (P2) turns a plan into a subprocess call; the
//! runner NEVER reimplements loop_lib / atc / handoff / weave.

use crate::jobspec::{JobKind, JobSpec};

/// The existing kernels the runner delegates to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kernel {
    LoopLib,
    Atc,
    Handoff,
    Weave,
}

impl Kernel {
    /// The canonical binary name for this kernel.
    pub fn program(&self) -> &'static str {
        match self {
            Kernel::LoopLib => "loop",
            Kernel::Atc => "atc",
            Kernel::Handoff => "hf",
            Kernel::Weave => "weave",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KernelPlan {
    pub kernel: Kernel,
    /// Human-readable intent; exact argv is finalized by the dispatcher in P2.
    pub intent: String,
    pub repo: String,
}

/// Route a job to its kernel. Pure.
pub fn route(job: &JobSpec) -> KernelPlan {
    match &job.job {
        JobKind::Ci { repo, head_sha } => KernelPlan {
            kernel: Kernel::LoopLib,
            intent: format!("ci build/test @ {head_sha}"),
            repo: repo.clone(),
        },
        JobKind::ReviewGate {
            repo, pr_number, ..
        } => KernelPlan {
            kernel: Kernel::Atc,
            intent: format!("merge-gate review PR #{pr_number}"),
            repo: repo.clone(),
        },
        JobKind::AgentTask { repo, prompt_ref } => KernelPlan {
            kernel: Kernel::Atc,
            intent: format!("agent task {prompt_ref}"),
            repo: repo.clone(),
        },
        JobKind::LoopCycle { repo, task_id } => KernelPlan {
            kernel: Kernel::Handoff,
            intent: format!("loop cycle / ship {task_id}"),
            repo: repo.clone(),
        },
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
            }))
            .kernel,
            Kernel::Atc
        );
        assert_eq!(
            route(&job(JobKind::AgentTask {
                repo: "r".into(),
                prompt_ref: "p".into(),
            }))
            .kernel,
            Kernel::Atc
        );
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
    fn program_names_are_the_real_binaries() {
        assert_eq!(Kernel::LoopLib.program(), "loop");
        assert_eq!(Kernel::Atc.program(), "atc");
        assert_eq!(Kernel::Handoff.program(), "hf");
        assert_eq!(Kernel::Weave.program(), "weave");
    }
}
