//! Pre-dispatch content / injection scan of a [`JobSpec`]'s free-text fields (adapted from
//! `coleam00/Archon`'s `marketplace-security-scan.ts` — severity-graded regex **pattern banks**, a
//! **scan/decide split**, and **fail-closed on critical/high** — plus `kclaw0`'s `path-simulator.js`
//! risk scoring).
//!
//! ## Why a delegate-only runner needs only a *narrow* slice of this
//! Archon scans whole *repositories* of submitted marketplace code. The runner never sees source: a
//! [`JobSpec`] carries only **short string references** — `id`, `correlation_id`, `repo`, `head_sha`,
//! `prompt_ref`, `task_id`. But those strings are **interpolated downstream**: the P3 invoker will
//! splice them into a kernel command line, a workspace path, and audit-log lines. A `prompt_ref` of
//! `"x$(rm -rf ~)"`, a `task_id` with an embedded newline (log/CRLF injection), or a `../../`
//! traversal in any field is an injection vector the structural [`lint`](crate::lint) does **not**
//! catch (lint proves *shape* — `owner/name`, non-blank — not *safety*). This module is the
//! content-safety complement: it scans each free-text field against a severity-graded pattern bank
//! and returns a [`ScanReport`]; the dispatcher's [`ScanPolicy`] then decides whether to refuse.
//!
//! ## Scan / decide split (Archon)
//! [`scan`] always just *computes* the findings (pure, cheap, no I/O) — it never decides. A separate
//! [`ScanPolicy`] (an operator threshold) turns the report's [`max_severity`](ScanReport::max_severity)
//! into a block/allow verdict. This keeps the detector reusable (audit-only, or enforcing) and the
//! policy in one place. **Fail-closed:** a field at or above the threshold refuses the dispatch
//! before any kernel is touched; the same bytes can never become safe by re-dispatch, so recovery
//! escalates it to a human (never retries).
//!
//! ## Behaviour-preserving / opt-in
//! No regex dependency — the bank is plain substring / char-class checks. The [`ScanPolicy`] is
//! **disabled by default** (`block_at` unset), so the gate is inert until an operator opts in
//! (`FXRUN_SCAN_BLOCK_SEVERITY`). Seam-first: it is the acquisition-side guard for the P3 invoker's
//! string interpolation, defined before that interpolation exists. Orthogonal to model routing
//! (weave's domain) and delegate-only (the runner refuses; it never sanitizes/rewrites the job).

use crate::jobspec::{JobKind, JobSpec};

/// How dangerous a finding is. Ordered, so a policy can threshold on it (`>=`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// No issue.
    None,
    /// Suspicious but low-risk (e.g. redirection metacharacters).
    Low,
    /// Command-chaining shell metacharacters (`;`, `|`, `&`).
    Medium,
    /// Strong injection signal: command substitution / template, path traversal, control chars, CRLF.
    High,
    /// Almost certainly hostile: an embedded NUL byte (truncation / C-string smuggling).
    Critical,
}

impl Severity {
    /// A short lowercase label for banners / audit details / env parsing.
    pub fn label(&self) -> &'static str {
        match self {
            Severity::None => "none",
            Severity::Low => "low",
            Severity::Medium => "medium",
            Severity::High => "high",
            Severity::Critical => "critical",
        }
    }

    /// Parse a severity from an operator string (`low`/`medium`/`high`/`critical`; anything else,
    /// including `off`/`none`/empty, is `None`). Case-insensitive.
    pub fn parse(s: &str) -> Severity {
        match s.trim().to_ascii_lowercase().as_str() {
            "low" => Severity::Low,
            "medium" | "med" => Severity::Medium,
            "high" => Severity::High,
            "critical" | "crit" => Severity::Critical,
            _ => Severity::None,
        }
    }
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// One content problem found in a field: which field, which pattern matched, and how severe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// The offending field path (e.g. `"job.prompt_ref"`, `"correlation_id"`).
    pub field: &'static str,
    /// The name of the pattern that matched (e.g. `"command-substitution"`).
    pub pattern: &'static str,
    /// How dangerous this match is.
    pub severity: Severity,
}

impl std::fmt::Display for Finding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} [{}] {}", self.field, self.severity, self.pattern)
    }
}

/// The result of scanning a whole [`JobSpec`]: every [`Finding`], in field-then-bank order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScanReport {
    findings: Vec<Finding>,
}

impl ScanReport {
    /// All findings (empty ⇒ clean).
    pub fn findings(&self) -> &[Finding] {
        &self.findings
    }

    /// Whether the scan found nothing.
    pub fn is_clean(&self) -> bool {
        self.findings.is_empty()
    }

    /// The worst severity found (`None` when clean).
    pub fn max_severity(&self) -> Severity {
        self.findings
            .iter()
            .map(|f| f.severity)
            .max()
            .unwrap_or(Severity::None)
    }

    /// A compact one-line summary of the findings (worst first) for a rejection / audit detail.
    pub fn summary(&self) -> String {
        let mut sorted: Vec<&Finding> = self.findings.iter().collect();
        sorted.sort_by_key(|f| std::cmp::Reverse(f.severity));
        sorted
            .iter()
            .map(|f| f.to_string())
            .collect::<Vec<_>>()
            .join("; ")
    }
}

/// One pattern in the bank: a name, the severity if it matches, and a matcher over a field value.
struct Pattern {
    name: &'static str,
    severity: Severity,
    matches: fn(&str) -> bool,
}

/// The severity-graded injection pattern bank. Plain substring / char-class checks (no regex dep).
/// Ordered worst-first only for readability; [`scan_value`] reports every match.
const BANK: &[Pattern] = &[
    Pattern {
        name: "nul-byte",
        severity: Severity::Critical,
        matches: |v| v.contains('\0'),
    },
    Pattern {
        name: "control-char",
        // Any C0 control other than tab (newline/CR are reported by `crlf-injection` below; this
        // catches the rest: ESC for ANSI/log injection, etc.).
        severity: Severity::High,
        matches: |v| {
            v.chars()
                .any(|c| c.is_control() && c != '\t' && c != '\n' && c != '\r')
        },
    },
    Pattern {
        name: "crlf-injection",
        severity: Severity::High,
        matches: |v| v.contains('\n') || v.contains('\r'),
    },
    Pattern {
        name: "command-substitution",
        severity: Severity::High,
        matches: |v| v.contains("$(") || v.contains('`') || v.contains("${"),
    },
    Pattern {
        name: "path-traversal",
        severity: Severity::High,
        matches: |v| v.contains("../") || v.contains("..\\"),
    },
    Pattern {
        name: "shell-metacharacter",
        severity: Severity::Medium,
        matches: |v| v.contains(';') || v.contains('|') || v.contains("&&") || v.contains('&'),
    },
    Pattern {
        name: "redirection",
        severity: Severity::Low,
        matches: |v| v.contains('>') || v.contains('<'),
    },
];

/// Scan a single field value against the whole bank, pushing a [`Finding`] for each pattern that
/// matches. Pure.
fn scan_value(field: &'static str, value: &str, out: &mut Vec<Finding>) {
    for p in BANK {
        if (p.matches)(value) {
            out.push(Finding {
                field,
                pattern: p.name,
                severity: p.severity,
            });
        }
    }
}

/// Scan every free-text field of `job` for injection patterns. Pure — it only *detects*; the
/// [`ScanPolicy`] decides whether any finding is disqualifying.
pub fn scan(job: &JobSpec) -> ScanReport {
    let mut findings = Vec::new();
    // Envelope strings (interpolated into workspace labels + audit lines).
    scan_value("id", &job.id, &mut findings);
    scan_value("correlation_id", &job.correlation_id, &mut findings);
    // Per-kind payload strings (interpolated into the kernel command line in P3).
    match &job.job {
        JobKind::Ci { repo, head_sha } => {
            scan_value("job.repo", repo, &mut findings);
            scan_value("job.head_sha", head_sha, &mut findings);
        }
        JobKind::ReviewGate { repo, head_sha, .. } => {
            scan_value("job.repo", repo, &mut findings);
            scan_value("job.head_sha", head_sha, &mut findings);
        }
        JobKind::AgentTask {
            repo, prompt_ref, ..
        } => {
            scan_value("job.repo", repo, &mut findings);
            scan_value("job.prompt_ref", prompt_ref, &mut findings);
        }
        JobKind::LoopCycle { repo, task_id } => {
            scan_value("job.repo", repo, &mut findings);
            scan_value("job.task_id", task_id, &mut findings);
        }
    }
    ScanReport { findings }
}

/// The operator's decision threshold for the content scan: refuse a dispatch whose worst finding is
/// at or above `block_at`. `None` (the default) = the gate is **off** (scan results, if computed, are
/// advisory only) — behaviour-preserving until an operator opts in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ScanPolicy {
    block_at: Option<Severity>,
}

impl ScanPolicy {
    /// A policy that blocks at or above `severity`. `Severity::None` is treated as **off** (a policy
    /// can't sensibly block "nothing"); use [`Self::disabled`] for clarity.
    pub fn block_at(severity: Severity) -> Self {
        Self {
            block_at: (severity != Severity::None).then_some(severity),
        }
    }

    /// The behaviour-preserving default: the gate is off (no dispatch is refused on content).
    pub fn disabled() -> Self {
        Self { block_at: None }
    }

    /// Build from an operator string (`FXRUN_SCAN_BLOCK_SEVERITY=low|medium|high|critical`; anything
    /// else, including `off`/empty, disables the gate).
    pub fn from_env(value: &str) -> Self {
        Self::block_at(Severity::parse(value))
    }

    /// Whether the gate is enforcing (will ever refuse).
    pub fn is_active(&self) -> bool {
        self.block_at.is_some()
    }

    /// The configured block threshold, if any.
    pub fn threshold(&self) -> Option<Severity> {
        self.block_at
    }

    /// Decide whether `report` should block the dispatch under this policy: `true` iff the gate is
    /// active and the report's worst finding is at or above the threshold.
    pub fn blocks(&self, report: &ScanReport) -> bool {
        match self.block_at {
            Some(threshold) => report.max_severity() >= threshold,
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::Agent;

    fn agent_task(prompt_ref: &str) -> JobSpec {
        JobSpec {
            id: "job-1".into(),
            correlation_id: "corr-1".into(),
            from_fork: false,
            job: JobKind::AgentTask {
                repo: "FlexNetOS/meta".into(),
                prompt_ref: prompt_ref.into(),
                agent: Agent::default(),
            },
        }
    }

    #[test]
    fn a_clean_job_has_no_findings() {
        let report = scan(&agent_task("prompts/review-pr-42"));
        assert!(report.is_clean());
        assert_eq!(report.max_severity(), Severity::None);
    }

    #[test]
    fn command_substitution_is_high() {
        let report = scan(&agent_task("x$(rm -rf ~)"));
        assert!(!report.is_clean());
        assert_eq!(report.max_severity(), Severity::High);
        assert!(report
            .findings()
            .iter()
            .any(|f| f.pattern == "command-substitution" && f.field == "job.prompt_ref"));
    }

    #[test]
    fn nul_byte_is_critical() {
        let report = scan(&agent_task("ref\0truncated"));
        assert_eq!(report.max_severity(), Severity::Critical);
    }

    #[test]
    fn newline_is_crlf_injection_high() {
        let report = scan(&agent_task("line1\ninjected: log line"));
        assert_eq!(report.max_severity(), Severity::High);
        assert!(report
            .findings()
            .iter()
            .any(|f| f.pattern == "crlf-injection"));
    }

    #[test]
    fn path_traversal_is_high() {
        let report = scan(&agent_task("../../etc/passwd"));
        assert!(report
            .findings()
            .iter()
            .any(|f| f.pattern == "path-traversal" && f.severity == Severity::High));
    }

    #[test]
    fn shell_metacharacters_are_medium() {
        let report = scan(&agent_task("a;b"));
        assert_eq!(report.max_severity(), Severity::Medium);
        assert!(report
            .findings()
            .iter()
            .any(|f| f.pattern == "shell-metacharacter"));
    }

    #[test]
    fn envelope_fields_are_scanned_too() {
        let mut job = agent_task("clean");
        job.correlation_id = "corr`whoami`".into();
        let report = scan(&job);
        assert!(report
            .findings()
            .iter()
            .any(|f| f.field == "correlation_id" && f.pattern == "command-substitution"));
    }

    #[test]
    fn disabled_policy_never_blocks_even_a_critical_report() {
        let report = scan(&agent_task("ref\0nul"));
        assert_eq!(report.max_severity(), Severity::Critical);
        assert!(!ScanPolicy::disabled().blocks(&report));
        assert!(!ScanPolicy::disabled().is_active());
    }

    #[test]
    fn policy_blocks_at_or_above_its_threshold() {
        let high = scan(&agent_task("x$(id)")); // High
        let medium = scan(&agent_task("a|b")); // Medium
                                               // Block-at-High: refuses the High report, allows the Medium one.
        let p = ScanPolicy::block_at(Severity::High);
        assert!(p.blocks(&high));
        assert!(!p.blocks(&medium));
        // Block-at-Medium refuses both.
        let p = ScanPolicy::block_at(Severity::Medium);
        assert!(p.blocks(&high));
        assert!(p.blocks(&medium));
    }

    #[test]
    fn from_env_parses_threshold_and_treats_garbage_as_off() {
        assert_eq!(
            ScanPolicy::from_env("high").threshold(),
            Some(Severity::High)
        );
        assert_eq!(
            ScanPolicy::from_env("CRITICAL").threshold(),
            Some(Severity::Critical)
        );
        assert!(!ScanPolicy::from_env("off").is_active());
        assert!(!ScanPolicy::from_env("").is_active());
        assert!(!ScanPolicy::from_env("none").is_active());
    }

    #[test]
    fn summary_lists_worst_first() {
        let report = scan(&agent_task("a; ../x $(id)")); // medium (;) + high (traversal) + high (subst)
        let s = report.summary();
        // The first listed finding is a High one (worst-first ordering).
        assert!(s.starts_with("job.prompt_ref [high]"));
    }
}
