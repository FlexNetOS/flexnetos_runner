//! Structural JobSpec lint (adapted from `strongdm/attractor`'s VALIDATE phase, which refuses a
//! run on any structural ERROR *before* doing real work).
//!
//! The runner-plane analogue: once a frame is authenticated (HMAC-verified, then parsed into a
//! typed [`JobSpec`]), check it is **structurally well-formed** before routing it to a kernel. A
//! malformed job — an empty `repo`, a `repo` that isn't `owner/name`, a blank `head_sha`,
//! `pr_number == 0` — would otherwise be carried all the way to a kernel only to fail there,
//! wasting a delegation (and, once `atc` reports cost, real money). Catching it here turns a late,
//! opaque kernel failure into an early, precise rejection the orchestrator can act on.
//!
//! Placement: this runs at the **earliest safe point** — immediately after authentication, before
//! the breaker / budget / route. (attractor lints *pre-verify*; in our model the equivalent is
//! "right after the HMAC proves the bytes are authentic", since `runner-core`'s wire contract is
//! *verify, then parse* — we never run logic on an unverified body.)
//!
//! Pure: it inspects the typed spec and returns the list of structural problems (empty = clean).
//! A structurally-invalid job is **not** retryable — re-dispatching the same bytes can't fix a
//! malformed field — so the recovery layer ([`crate::recovery`]) escalates it rather than retrying.

use crate::jobspec::{JobKind, JobSpec};

/// One structural problem found in a [`JobSpec`]: which field, and what's wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LintError {
    /// The offending field path (e.g. `"job.repo"`, `"id"`).
    pub field: &'static str,
    /// Human-readable description of the violation.
    pub message: String,
}

impl LintError {
    fn new(field: &'static str, message: impl Into<String>) -> Self {
        Self {
            field,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for LintError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.field, self.message)
    }
}

/// A `repo` must look like `owner/name`: two non-empty, whitespace-free segments.
fn repo_errors(repo: &str) -> Option<LintError> {
    if repo.trim().is_empty() {
        return Some(LintError::new("job.repo", "repo is empty"));
    }
    if repo.chars().any(char::is_whitespace) {
        return Some(LintError::new(
            "job.repo",
            format!("repo `{repo}` contains whitespace (expected owner/name)"),
        ));
    }
    let mut parts = repo.split('/');
    let owner = parts.next().unwrap_or("");
    let name = parts.next().unwrap_or("");
    let extra = parts.next();
    if owner.is_empty() || name.is_empty() || extra.is_some() {
        return Some(LintError::new(
            "job.repo",
            format!("repo `{repo}` is not `owner/name`"),
        ));
    }
    None
}

/// A field that must be a non-empty, non-blank string.
fn require_nonblank(value: &str, field: &'static str, label: &str) -> Option<LintError> {
    value
        .trim()
        .is_empty()
        .then(|| LintError::new(field, format!("{label} is empty")))
}

/// Collect every structural problem in `job` (empty ⇒ structurally valid). Pure.
pub fn structural_errors(job: &JobSpec) -> Vec<LintError> {
    let mut errs = Vec::new();

    // Envelope fields are required for every job kind.
    errs.extend(require_nonblank(&job.id, "id", "job id"));
    errs.extend(require_nonblank(
        &job.correlation_id,
        "correlation_id",
        "correlation id",
    ));

    match &job.job {
        JobKind::Ci { repo, head_sha } => {
            errs.extend(repo_errors(repo));
            errs.extend(require_nonblank(head_sha, "job.head_sha", "head_sha"));
        }
        JobKind::ReviewGate {
            repo,
            pr_number,
            head_sha,
            ..
        } => {
            errs.extend(repo_errors(repo));
            errs.extend(require_nonblank(head_sha, "job.head_sha", "head_sha"));
            if *pr_number == 0 {
                errs.push(LintError::new("job.pr_number", "pr_number must be > 0"));
            }
        }
        JobKind::AgentTask {
            repo, prompt_ref, ..
        } => {
            errs.extend(repo_errors(repo));
            errs.extend(require_nonblank(prompt_ref, "job.prompt_ref", "prompt_ref"));
        }
        JobKind::LoopCycle { repo, task_id } => {
            errs.extend(repo_errors(repo));
            errs.extend(require_nonblank(task_id, "job.task_id", "task_id"));
        }
    }
    errs
}

/// Whether `job` is structurally valid (no [`structural_errors`]).
pub fn is_structurally_valid(job: &JobSpec) -> bool {
    structural_errors(job).is_empty()
}

/// Render structural errors as a single, compact reason string for a rejection.
pub fn summarize(errors: &[LintError]) -> String {
    errors
        .iter()
        .map(LintError::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::Agent;

    fn ci(repo: &str, head_sha: &str) -> JobSpec {
        JobSpec {
            id: "job-1".into(),
            correlation_id: "corr-1".into(),
            from_fork: false,
            job: JobKind::Ci {
                repo: repo.into(),
                head_sha: head_sha.into(),
            },
        }
    }

    #[test]
    fn a_well_formed_job_is_clean() {
        let job = ci("FlexNetOS/meta", "abc123");
        assert!(is_structurally_valid(&job));
        assert!(structural_errors(&job).is_empty());
    }

    #[test]
    fn empty_repo_is_flagged() {
        let errs = structural_errors(&ci("", "abc"));
        assert!(errs.iter().any(|e| e.field == "job.repo"));
        assert!(!is_structurally_valid(&ci("", "abc")));
    }

    #[test]
    fn repo_without_owner_name_is_flagged() {
        assert!(!is_structurally_valid(&ci("justaname", "abc")));
        assert!(!is_structurally_valid(&ci("a/b/c", "abc")));
        assert!(!is_structurally_valid(&ci("owner/", "abc")));
        assert!(!is_structurally_valid(&ci("/name", "abc")));
        assert!(!is_structurally_valid(&ci("owner name/x", "abc")));
    }

    #[test]
    fn blank_head_sha_is_flagged() {
        let errs = structural_errors(&ci("FlexNetOS/meta", "   "));
        assert!(errs.iter().any(|e| e.field == "job.head_sha"));
    }

    #[test]
    fn review_gate_requires_positive_pr_number() {
        let job = JobSpec {
            id: "j".into(),
            correlation_id: "c".into(),
            from_fork: false,
            job: JobKind::ReviewGate {
                repo: "FlexNetOS/meta".into(),
                pr_number: 0,
                head_sha: "abc".into(),
                agent: Agent::default(),
            },
        };
        let errs = structural_errors(&job);
        assert!(errs.iter().any(|e| e.field == "job.pr_number"));
    }

    #[test]
    fn agent_task_requires_prompt_ref() {
        let job = JobSpec {
            id: "j".into(),
            correlation_id: "c".into(),
            from_fork: false,
            job: JobKind::AgentTask {
                repo: "FlexNetOS/meta".into(),
                prompt_ref: "".into(),
                agent: Agent::default(),
            },
        };
        assert!(structural_errors(&job)
            .iter()
            .any(|e| e.field == "job.prompt_ref"));
    }

    #[test]
    fn loop_cycle_requires_task_id() {
        let job = JobSpec {
            id: "j".into(),
            correlation_id: "c".into(),
            from_fork: false,
            job: JobKind::LoopCycle {
                repo: "FlexNetOS/meta".into(),
                task_id: " ".into(),
            },
        };
        assert!(structural_errors(&job)
            .iter()
            .any(|e| e.field == "job.task_id"));
    }

    #[test]
    fn missing_envelope_ids_are_flagged() {
        let mut job = ci("FlexNetOS/meta", "abc");
        job.id = "".into();
        job.correlation_id = "  ".into();
        let errs = structural_errors(&job);
        assert!(errs.iter().any(|e| e.field == "id"));
        assert!(errs.iter().any(|e| e.field == "correlation_id"));
    }

    #[test]
    fn multiple_problems_are_all_reported_and_summarized() {
        let errs = structural_errors(&ci("bad repo", ""));
        assert_eq!(errs.len(), 2);
        let summary = summarize(&errs);
        assert!(summary.contains("job.repo"));
        assert!(summary.contains("job.head_sha"));
        assert!(summary.contains("; "));
    }
}
