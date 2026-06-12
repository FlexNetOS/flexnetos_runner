//! Runner safety policy (ADR-0008 §2/§6): fork-PR isolation + runner rails.
//!
//! The headline rule (GitHub secure-use guidance): untrusted **fork** code must NEVER run
//! on self-hosted hardware. Placement is fail-safe — anything fork-triggered is hosted-only.

use crate::jobspec::JobSpec;

/// Where a job is allowed to execute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// Trusted: may run on the local self-hosted runner.
    SelfHosted,
    /// Untrusted (fork PR): must run on GitHub-hosted / sandboxed infra — never self-hosted.
    HostedOnly,
}

/// Decide placement for a job. Fork-triggered jobs are always [`Placement::HostedOnly`].
pub fn placement(job: &JobSpec) -> Placement {
    if job.from_fork {
        Placement::HostedOnly
    } else {
        Placement::SelfHosted
    }
}

/// The non-negotiable rails the supervisor must enforce when launching a self-hosted job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rails {
    pub non_root: bool,
    pub mount_docker_socket: bool,
    pub work_dir_on_tmpfs: bool,
    pub ephemeral_single_job: bool,
    pub labels: Vec<String>,
}

impl Default for Rails {
    fn default() -> Self {
        Self {
            non_root: true,
            mount_docker_socket: false,
            work_dir_on_tmpfs: true,
            ephemeral_single_job: true,
            labels: vec!["self-hosted".into(), "flexnetos".into()],
        }
    }
}

impl Rails {
    /// True iff the rails meet the ADR-0008 §6 minimums (the supervisor fails closed otherwise).
    pub fn is_safe(&self) -> bool {
        self.non_root
            && !self.mount_docker_socket
            && self.work_dir_on_tmpfs
            && self.ephemeral_single_job
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobspec::{JobKind, JobSpec};

    fn job(from_fork: bool) -> JobSpec {
        JobSpec {
            id: "j".into(),
            correlation_id: "c".into(),
            from_fork,
            job: JobKind::Ci {
                repo: "r".into(),
                head_sha: "s".into(),
            },
        }
    }

    #[test]
    fn fork_jobs_are_hosted_only() {
        assert_eq!(placement(&job(true)), Placement::HostedOnly);
        assert_eq!(placement(&job(false)), Placement::SelfHosted);
    }

    #[test]
    fn default_rails_are_safe() {
        assert!(Rails::default().is_safe());
    }

    #[test]
    fn unsafe_rails_are_detected() {
        let docker = Rails {
            mount_docker_socket: true,
            ..Rails::default()
        };
        assert!(!docker.is_safe());

        let root = Rails {
            non_root: false,
            ..Rails::default()
        };
        assert!(!root.is_safe());

        let persistent = Rails {
            ephemeral_single_job: false,
            ..Rails::default()
        };
        assert!(!persistent.is_safe());
    }
}
