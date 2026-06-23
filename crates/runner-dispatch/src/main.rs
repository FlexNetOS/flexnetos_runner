//! `fxrun-dispatch` — the meta-native dispatcher (ADR-0008 §2/S7).
//!
//! Two modes:
//! - **`--socket <path>` (P2, Unix only):** bind a Unix-domain socket, accept signed
//!   [`DispatchRequest`] frames from `flexnetos_github_app`, verify the HMAC, enforce fork-PR
//!   isolation, route to a kernel via [`runner_core::router`], and delegate through the
//!   [`KernelInvoker`] seam — **never reimplementing** loop_lib / atc / handoff / weave.
//! - **stdin (P0):** read one JSON `JobSpec`, print the plan (dry-run smoke aid).
//!
//! Protocol: one request per connection — the client writes the JSON [`DispatchRequest`], shuts
//! down its write half, then reads the JSON [`DispatchResponse`]. The HMAC key comes from
//! envctl's vault in P3 (`FXRUN_DISPATCH_KEY` is the transitional source).
//!
//! The runner host is Unix (it supervises a self-hosted Actions runner); the UDS server is
//! therefore `#[cfg(unix)]`. The decision core ([`handle_request`]) is platform-independent and
//! unit-tested on every OS so the build stays green across the 3-OS matrix.

// On non-Unix the binary degrades to the stdin dry-run: the decision core + invoker seam are
// compiled and tested but not wired to a transport, so allow them to be "unused" there. The
// real target (Unix) keeps full dead-code checking.
#![cfg_attr(not(unix), allow(dead_code))]

use runner_core::approval::ApprovalPolicy;
use runner_core::authority::{AuthorityDecision, AuthorityPolicy};
use runner_core::constitution::{Constitution, ConstitutionStatus};
use runner_core::cost::JobCost;
use runner_core::deadline::DeadlinePolicy;
use runner_core::events::{DispatchEvent, EventCategory, EventSink, NullSink, Outcome};
use runner_core::governor::{Admission, Governor};
use runner_core::jobspec::JobSpec;
use runner_core::lint;
use runner_core::loopguard::{fingerprint, LoopGuard, Verdict};
use runner_core::quarantine::{QuarantineLedger, QuarantinePolicy};
use runner_core::ratelimit::{RateDecision, RateLimitPolicy, RateLimiter};
use runner_core::recovery::{
    classify_kernel_error, FailureKind, RecoveryDirective, RecoveryPolicy, RecoveryVerb,
    RetryLedger,
};
use runner_core::redact::{RedactingSink, Redactor};
use runner_core::risk::{RiskLedger, RiskPolicy, RiskScore};
use runner_core::router::{self, Kernel, KernelPlan};
use runner_core::safety::{self, Placement};
use runner_core::scan::{self, ScanPolicy};
use runner_core::singleflight::{FlightLease, SingleFlight};
use runner_core::targets::{TargetAllowlist, TargetDecision};
use runner_core::wire::{verify_frame, DispatchRequest, DispatchResponse};
use runner_core::workspace::{JobWorkspace, WorkspaceProvider};
use std::io::Read;

/// The delegation seam: turn a routed [`KernelPlan`] into a real kernel invocation. The dispatcher
/// NEVER reimplements a kernel — it shells out to the existing binary. Injected so the UDS path is
/// testable with a fake (no kernels spawned in CI).
///
/// **The invoker owns deadline enforcement.** A real subprocess invoker spawns the kernel and
/// **hard-kills its child** at the effective wall-clock `deadline` (attractor's "interrupt"; Archon's
/// `dockerStop`), returning [`Delegation::TimedOut`]; in-process invokers that cannot hang (the
/// dry-run, the test fakes) ignore the deadline. Enforcement lives here — not in a watchdog thread —
/// because only the entity that owns the child handle can kill it (a thread that merely *waits* on a
/// hung `child.wait()` would itself block forever and could not reclaim the process).
trait KernelInvoker {
    /// Invoke the kernel under an optional wall-clock `deadline`, returning the typed outcome
    /// (delivered-with-cost / failed / timed-out). The cost is the `atc → runner` seam; kernels that
    /// don't measure cost return [`JobCost::ZERO`].
    fn invoke(
        &self,
        plan: &KernelPlan,
        job: &JobSpec,
        deadline: Option<std::time::Duration>,
    ) -> Delegation;
}

/// The outcome of one delegation attempt.
enum Delegation {
    /// The kernel completed within the deadline, reporting its cost.
    Delivered(JobCost),
    /// The kernel returned an error (the string is the failure detail).
    Failed(String),
    /// The kernel did not finish within the effective deadline and was killed (the duration is the
    /// limit that was exceeded). The runner-plane bound on a *hung* job — the time axis the breaker /
    /// governor / quarantine don't cover.
    TimedOut(std::time::Duration),
}

/// Run one delegation under the effective `deadline`. The invoker owns enforcement (see
/// [`KernelInvoker`]); this is a thin seam kept so the call site reads as "delegate this plan".
fn run_delegation(
    invoker: &(dyn KernelInvoker + Sync),
    plan: &KernelPlan,
    job: &JobSpec,
    deadline: Option<std::time::Duration>,
) -> Delegation {
    invoker.invoke(plan, job, deadline)
}

/// Shared handling for a failed/timed-out delegation: advise recovery (retry-with-backoff →
/// escalate, per `kind`) and record the failure against the quarantine ledger (latching the
/// fingerprint once it reaches the threshold). A free function — not a closure — so it does not hold
/// the `retry`/`quarantine` mutable borrows across the delegation `match`'s success arm.
#[allow(clippy::too_many_arguments)]
fn handle_failure(
    policy: &RecoveryPolicy,
    retry: &mut RetryLedger,
    quarantine_policy: &QuarantinePolicy,
    quarantine: &mut QuarantineLedger,
    rate_limiter: &mut RateLimiter,
    now_secs: u64,
    risk: Option<RiskScore>,
    sink: &dyn EventSink,
    job: &JobSpec,
    program: &str,
    fp: &str,
    kind: FailureKind,
    outcome: Outcome,
    lead: String,
) -> DispatchResponse {
    let directive = policy.decide(retry, fp, kind);
    let now_quarantined = quarantine_policy.on_failure(quarantine, fp);
    // A kernel failure / timeout also starts this route's failure cooldown (automaton's error
    // backoff), so a burst of the same route's work backs off. No-op unless a cooldown is configured.
    rate_limiter.record_failure(job.job.class(), now_secs);
    let detail = format!(
        "{lead} | {}{}{}",
        directive.summary(),
        if now_quarantined {
            format!(
                " | fingerprint quarantined ({}x failures ≥ threshold {})",
                quarantine.failures(fp),
                quarantine_policy.threshold()
            )
        } else {
            String::new()
        },
        match risk {
            Some(r) => format!(" | {}", r.summary()),
            None => String::new(),
        }
    );
    sink.emit(
        &DispatchEvent::for_job(outcome, job)
            .with_kernel(program)
            .with_recovery(directive.clone())
            .with_detail(&detail)
            .with_risk(risk),
    );
    DispatchResponse::rejected(detail).with_recovery(directive)
}

/// Creates an isolated per-job work area as a real temp directory and guarantees its teardown on
/// every exit path (Archon "fail → zero residue"). The P3 invoker will create a tmpfs *worktree*
/// here instead; the contract (a [`JobWorkspace`] guard whose `Drop` removes the tree) is identical.
#[cfg(unix)]
#[derive(Default)]
struct TempDirProvider;

#[cfg(unix)]
impl WorkspaceProvider for TempDirProvider {
    fn acquire(&self, label: &str) -> Result<JobWorkspace, String> {
        // A unique, isolated directory per job. (P3: a tmpfs worktree under the rails' work dir.)
        let safe: String = label
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        let root = std::env::temp_dir().join(format!("fxrun-ws-{}-{safe}", std::process::id()));
        std::fs::create_dir_all(&root).map_err(|e| format!("workspace create failed: {e}"))?;
        let cleanup_root = root.clone();
        Ok(JobWorkspace::new(root, move || {
            std::fs::remove_dir_all(&cleanup_root).map_err(|e| e.to_string())
        })
        .with_residue_reporter(|root, reason| {
            eprintln!(
                "fxrun-dispatch: WORKSPACE RESIDUE — failed to remove {} ({reason})",
                root.display()
            );
        }))
    }
}

/// The dry-run invoker: logs the delegation it *would* perform (no subprocess), inside an isolated
/// workspace whose teardown is guaranteed on every exit. Kept as the behaviour-preserving default
/// when kernel execution is not enabled (the runner still routes + governs + audits without spawning
/// real kernels), and so the full admission pipeline stays exercisable with no kernels installed.
/// Only wired into the Unix `serve` path; the decision core is exercised cross-platform via tests.
#[cfg(unix)]
struct DryRunInvoker<P: WorkspaceProvider> {
    workspace: P,
}

#[cfg(unix)]
impl<P: WorkspaceProvider> KernelInvoker for DryRunInvoker<P> {
    fn invoke(
        &self,
        plan: &KernelPlan,
        job: &JobSpec,
        _deadline: Option<std::time::Duration>,
    ) -> Delegation {
        // Acquire the isolated work area. Its guard tears the tree down when this scope ends (Archon
        // zero-residue). The dry-run never hangs, so it ignores the deadline.
        let _ws = match self.workspace.acquire(&job.id) {
            Ok(ws) => ws,
            Err(e) => return Delegation::Failed(e),
        };
        let agent = match plan.agent {
            Some(a) => format!(", agent {a}"),
            None => String::new(),
        };
        eprintln!(
            "  delegate (dry-run) → `{}` : {} (job {}, corr {}, repo {}{}, ws {})",
            plan.kernel.program(),
            plan.intent,
            job.id,
            job.correlation_id,
            plan.repo,
            agent,
            _ws.root().display()
        );
        // The dry-run measures no cost (the seam is inert until a real kernel reports).
        Delegation::Delivered(JobCost::ZERO)
        // `_ws` drops here → the workspace is torn down (guaranteed, every path).
    }
}

/// Resolves a [`Kernel`] to the executable command the runner spawns. Default = the canonical
/// program name on `PATH` ([`Kernel::program`]); overridable per kernel via
/// `FXRUN_KERNEL_CMD_{LOOP,ATC,HF,WEAVE}` so an operator can point at the real installed binary (an
/// absolute path or a wrapper) and the test suite can point at a stub kernel. Delegate-only: this is
/// *which existing binary to shell out to*, never a reimplementation.
#[cfg(unix)]
#[derive(Debug, Clone)]
struct KernelCommands {
    loop_lib: String,
    atc: String,
    hf: String,
    weave: String,
}

#[cfg(unix)]
impl KernelCommands {
    /// Read the per-kernel overrides from the environment, falling back to the canonical program name.
    fn from_env() -> Self {
        let resolve = |var: &str, default: &str| {
            std::env::var(var)
                .ok()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| default.to_string())
        };
        Self {
            loop_lib: resolve("FXRUN_KERNEL_CMD_LOOP", Kernel::LoopLib.program()),
            atc: resolve("FXRUN_KERNEL_CMD_ATC", Kernel::Atc.program()),
            hf: resolve("FXRUN_KERNEL_CMD_HF", Kernel::Handoff.program()),
            weave: resolve("FXRUN_KERNEL_CMD_WEAVE", Kernel::Weave.program()),
        }
    }

    /// The command to spawn for `kernel`.
    fn for_kernel(&self, kernel: Kernel) -> &str {
        match kernel {
            Kernel::LoopLib => &self.loop_lib,
            Kernel::Atc => &self.atc,
            Kernel::Handoff => &self.hf,
            Kernel::Weave => &self.weave,
        }
    }
}

/// The secret relay (the P3 "envctl relay-bearer" flow): envctl injects named secrets into the
/// **runner's** environment; the runner relays the configured subset (`FXRUN_INJECT_SECRETS=A,B,C`)
/// into each delegated kernel's child environment, and registers their *values* with the [`Redactor`]
/// so they can never surface in the audit log or an error reply. Delegate-only: the runner only
/// passes the secret through to the kernel that needs it; it never uses it itself.
#[cfg(unix)]
fn resolve_injected_secrets() -> Vec<(String, String)> {
    std::env::var("FXRUN_INJECT_SECRETS")
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .filter_map(|name| std::env::var(name).ok().map(|val| (name.to_string(), val)))
        .collect()
}

/// The **P3** invoker: spawns the real kernel binary as a child process inside the job's isolated
/// workspace (cwd), hands it the JobSpec on stdin + handoff/secret env vars, **enforces the deadline
/// by killing the child** at the wall-clock limit, and relays the cost report the kernel writes to
/// `FXRUN_COST_FILE`. The runner stays delegate-only: it spawns + bounds + reclaims; the kernel owns
/// *how* the work runs. Teardown of the workspace (incl. any orphaned child output) is guaranteed by
/// the [`JobWorkspace`] guard on every exit path.
#[cfg(unix)]
struct SubprocessInvoker<P: WorkspaceProvider> {
    workspace: P,
    commands: KernelCommands,
    /// Resolved (name, value) secrets to inject into the kernel child's env (see the relay above).
    secrets: Vec<(String, String)>,
}

#[cfg(unix)]
impl<P: WorkspaceProvider> KernelInvoker for SubprocessInvoker<P> {
    fn invoke(
        &self,
        plan: &KernelPlan,
        job: &JobSpec,
        deadline: Option<std::time::Duration>,
    ) -> Delegation {
        use std::io::Write;
        use std::process::{Command, Stdio};

        let ws = match self.workspace.acquire(&job.id) {
            Ok(ws) => ws,
            Err(e) => return Delegation::Failed(e),
        };
        let cmd_path = self.commands.for_kernel(plan.kernel).to_string();
        let cost_file = ws.root().join("fxrun-cost.json");
        let stderr_file = ws.root().join("fxrun-stderr.log");
        let spec_json = match serde_json::to_string(job) {
            Ok(s) => s,
            Err(e) => return Delegation::Failed(format!("jobspec encode failed: {e}")),
        };
        // stderr → a file in the workspace (avoids a pipe-buffer deadlock with a chatty kernel; read
        // back on failure). stdout is inherited so the kernel's own progress is visible to the operator.
        let stderr_sink = match std::fs::File::create(&stderr_file) {
            Ok(f) => f,
            Err(e) => return Delegation::Failed(format!("workspace stderr file failed: {e}")),
        };
        let mut cmd = Command::new(&cmd_path);
        cmd.current_dir(ws.root())
            .env("FXRUN_JOB_ID", &job.id)
            .env("FXRUN_CORRELATION_ID", &job.correlation_id)
            .env("FXRUN_KERNEL", plan.kernel.program())
            .env("FXRUN_INTENT", &plan.intent)
            .env("FXRUN_REPO", &plan.repo)
            .env("FXRUN_COST_FILE", &cost_file)
            .stdin(Stdio::piped())
            .stdout(Stdio::inherit())
            .stderr(Stdio::from(stderr_sink));
        if let Some(agent) = plan.agent {
            cmd.env("FXRUN_AGENT", agent.as_str());
        }
        for (k, v) in &self.secrets {
            cmd.env(k, v);
        }

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => return Delegation::Failed(format!("spawn `{cmd_path}` failed: {e}")),
        };
        // Hand the kernel its JobSpec on stdin, then close it (the spec is small — well under the pipe
        // buffer — so this never blocks).
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(spec_json.as_bytes());
        }

        // Wait, enforcing the deadline by polling try_wait and killing the child on expiry.
        let status = match deadline {
            None => match child.wait() {
                Ok(s) => s,
                Err(e) => return Delegation::Failed(format!("wait on `{cmd_path}` failed: {e}")),
            },
            Some(limit) => {
                let start = std::time::Instant::now();
                loop {
                    match child.try_wait() {
                        Ok(Some(s)) => break s,
                        Ok(None) => {}
                        Err(e) => {
                            return Delegation::Failed(format!("wait on `{cmd_path}` failed: {e}"))
                        }
                    }
                    if start.elapsed() >= limit {
                        // Hard-kill the hung child (attractor "interrupt" / Archon dockerStop), reap it,
                        // and report the timeout. The workspace guard reclaims its tree on return.
                        let _ = child.kill();
                        let _ = child.wait();
                        return Delegation::TimedOut(limit);
                    }
                    std::thread::sleep(std::time::Duration::from_millis(25));
                }
            }
        };

        if status.success() {
            let report = std::fs::read_to_string(&cost_file).unwrap_or_default();
            Delegation::Delivered(JobCost::from_report(&report))
        } else {
            let stderr_tail = std::fs::read_to_string(&stderr_file)
                .unwrap_or_default()
                .lines()
                .rev()
                .take(3)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join("; ");
            let code = status
                .code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "signal".into());
            Delegation::Failed(format!("kernel `{cmd_path}` exited {code}: {stderr_tail}"))
        }
        // `ws` drops here → workspace torn down (guaranteed), incl. the cost/stderr files.
    }
}

/// Append-only NDJSON audit sink: one JSON object per line to `FXRUN_EVENT_LOG`. Best-effort —
/// an I/O error is logged to stderr but never fails a dispatch (the audit log must not become a
/// new failure mode). Opens in append mode per event, so the log survives restarts and concurrent
/// readers can `tail -f` it. The runner-core `EventSink` keeps `runner-core` itself I/O-free.
#[cfg(unix)]
struct FileSink {
    path: std::path::PathBuf,
}

#[cfg(unix)]
impl EventSink for FileSink {
    fn emit(&self, event: &DispatchEvent) {
        use std::io::Write;
        let line = event.to_ndjson();
        let write = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .and_then(|mut f| writeln!(f, "{line}"));
        if let Err(e) = write {
            eprintln!(
                "fxrun-dispatch: audit log write failed ({}): {e}",
                self.path.display()
            );
        }
    }
}

/// Routes each event to up to two NDJSON streams: `all` receives every event (the full audit log),
/// while `policy` receives only admission/guardrail ([`EventCategory::Policy`]) events — a distinct
/// `policy_decisions` stream (automaton) so the guardrail layer can be audited / tamper-checked on
/// its own. Either stream may be absent; both absent is handled by using [`NullSink`] instead.
#[cfg(unix)]
struct RoutingSink {
    all: Option<FileSink>,
    policy: Option<FileSink>,
}

#[cfg(unix)]
impl EventSink for RoutingSink {
    fn emit(&self, event: &DispatchEvent) {
        if let Some(sink) = &self.all {
            sink.emit(event);
        }
        if event.category() == EventCategory::Policy {
            if let Some(sink) = &self.policy {
                sink.emit(event);
            }
        }
    }
}

/// RAII release for the per-target single-flight mutex. Once a job has acquired its mutable target,
/// every later admission/execution exit path must release it — clean delegation, policy refusal,
/// kernel failure, timeout, or panic. This guard localizes that guarantee to the dispatcher binary
/// while `runner-core::singleflight` stays pure.
struct SingleFlightPermit<'a> {
    ledger: &'a mut SingleFlight,
    lease: Option<FlightLease>,
}

impl<'a> SingleFlightPermit<'a> {
    fn new(ledger: &'a mut SingleFlight, lease: FlightLease) -> Self {
        Self {
            ledger,
            lease: Some(lease),
        }
    }
}

impl Drop for SingleFlightPermit<'_> {
    fn drop(&mut self) {
        if let Some(lease) = self.lease.take() {
            let _ = self.ledger.release(&lease);
        }
    }
}

/// Handle one received frame end-to-end. Pure over its inputs (no socket), so the
/// accept→verify→lint→isolate→breaker→budget→route→delegate decision is unit-tested directly.
/// Fail-closed: every non-happy path is a `DispatchResponse::rejected`. `guard`, `governor`, and
/// `retry` persist across connections (the runaway-loop breaker, the dispatch budget, and the
/// per-fingerprint retry ledger are stateful by design); every terminal decision is also written to
/// the audit `sink` (kclaw0 `event-system.js` lineage). A failed dispatch carries a
/// [`runner_core::recovery::RecoveryDirective`] back to the orchestrator (retry-with-backoff vs.
/// escalate-to-human; `policy` configures it).
#[allow(clippy::too_many_arguments)]
fn handle_request(
    key: &[u8],
    invoker: &(dyn KernelInvoker + Sync),
    constitution: &ConstitutionStatus,
    approval: &ApprovalPolicy,
    authority: &AuthorityPolicy,
    target_allowlist: &TargetAllowlist,
    singleflight: &mut SingleFlight,
    guard: &mut LoopGuard,
    governor: &mut Governor,
    policy: &RecoveryPolicy,
    retry: &mut RetryLedger,
    quarantine_policy: &QuarantinePolicy,
    quarantine: &mut QuarantineLedger,
    rate_limiter: &mut RateLimiter,
    now_secs: u64,
    scan_policy: &ScanPolicy,
    risk_policy: &RiskPolicy,
    risk_ledger: &mut RiskLedger,
    deadline: &DeadlinePolicy,
    sink: &dyn EventSink,
    raw: &[u8],
) -> DispatchResponse {
    // Constitution-immutability gate (FIRST, matching dark-factory's immutability→… order): if the
    // runner's own governing files changed mid-run, an agent may be weakening its guardrails — refuse
    // everything, before even parsing the frame. Inert unless files are sealed (FXRUN_CONSTITUTION).
    if let ConstitutionStatus::Violated { changed } = constitution {
        let detail = format!(
            "constitution violated: {} changed mid-run; refusing all dispatch until re-armed",
            changed.join(", ")
        );
        sink.emit(&DispatchEvent::untied(
            Outcome::ConstitutionViolated,
            &detail,
        ));
        return DispatchResponse::rejected(detail);
    }

    let req: DispatchRequest = match serde_json::from_slice(raw) {
        Ok(r) => r,
        Err(e) => {
            let detail = format!("unparseable dispatch frame: {e}");
            sink.emit(&DispatchEvent::untied(Outcome::Unparseable, &detail));
            return DispatchResponse::rejected(detail);
        }
    };
    let job = match verify_frame(key, &req) {
        Ok(j) => j,
        Err(e) => {
            let detail = format!("frame rejected: {e}");
            sink.emit(&DispatchEvent::untied(Outcome::VerifyFailed, &detail));
            return DispatchResponse::rejected(detail);
        }
    };

    // Dispatch provenance / authority gate (automaton authority tiers + access-broker prior art):
    // the envelope can state *who* submitted the dispatch, and the operator can set per-route floors
    // for privileged classes. This is intentionally before structural/content inspection: once the
    // signed JobSpec parsed, decide whether this origin may ask for the route at all. Inert unless
    // FXRUN_AUTHORITY_RULES is configured, preserving older App frames with no submitter.
    if let AuthorityDecision::Denied {
        route,
        required,
        actual,
        submitter,
    } = authority.check(&job, req.submitter.as_ref())
    {
        let directive = policy.decide(retry, &fingerprint(&job), FailureKind::AuthorityDenied);
        let who = submitter.unwrap_or_else(|| "<none>".into());
        let detail = format!(
            "authority gate denied route `{route}` for submitter `{who}`: required ≥ {required}, actual {actual} | {}",
            directive.summary()
        );
        sink.emit(
            &DispatchEvent::for_job(Outcome::AuthorityDenied, &job)
                .with_recovery(directive.clone())
                .with_detail(&detail),
        );
        return DispatchResponse::rejected(detail).with_recovery(directive);
    }

    // Structural lint (attractor VALIDATE phase): the earliest safe gate — now that the bytes are
    // authenticated, refuse a malformed job (bad repo / blank head_sha / pr_number 0) before any
    // kernel is touched, rather than letting it fail opaquely at the kernel. A malformed job can
    // never become valid by re-dispatch, so recovery escalates it to a human.
    let structural = lint::structural_errors(&job);
    if !structural.is_empty() {
        let fp = fingerprint(&job);
        let directive = policy.decide(retry, &fp, FailureKind::Malformed);
        let detail = format!(
            "structurally-invalid job: {} | {}",
            lint::summarize(&structural),
            directive.summary()
        );
        sink.emit(
            &DispatchEvent::for_job(Outcome::Malformed, &job)
                .with_recovery(directive.clone())
                .with_detail(&detail),
        );
        return DispatchResponse::rejected(detail).with_recovery(directive);
    }

    // Content / injection scan (Archon marketplace-security-scan): structural lint proved the spec's
    // *shape*; this checks the *safety* of its free-text fields, which the P3 invoker will interpolate
    // into a kernel command line / workspace path / audit line. A field whose worst finding meets the
    // operator's block threshold is refused here — after lint, before fork/route — fail-closed. Like a
    // malformed job, hostile content can't be fixed by re-dispatch, so recovery escalates (never
    // retries). Inert unless FXRUN_SCAN_BLOCK_SEVERITY is set (scan/decide split: the scan is cheap,
    // the policy decides).
    if scan_policy.is_active() {
        let report = scan::scan(&job);
        if scan_policy.blocks(&report) {
            let directive = policy.decide(retry, &fingerprint(&job), FailureKind::ContentRejected);
            let detail = format!(
                "content scan blocked the job (worst severity {}, threshold {}): {} | {}",
                report.max_severity(),
                scan_policy
                    .threshold()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "none".into()),
                report.summary(),
                directive.summary()
            );
            sink.emit(
                &DispatchEvent::for_job(Outcome::ContentRejected, &job)
                    .with_recovery(directive.clone())
                    .with_detail(&detail),
            );
            return DispatchResponse::rejected(detail).with_recovery(directive);
        }
    }

    // Fork-PR isolation (ADR-0008 §6): untrusted fork code must NEVER run on self-hosted hardware.
    // Enforced HERE, before any kernel is touched, so a forged-but-signed fork job still can't run.
    let placement = safety::placement(&job);
    if placement == Placement::HostedOnly {
        let detail = "fork-triggered job must run on GitHub-hosted infra, not the self-hosted \
                      dispatcher";
        sink.emit(&DispatchEvent::for_job(Outcome::ForkRejected, &job).with_detail(detail));
        return DispatchResponse::rejected(detail);
    }

    // Human-approval gate (Archon ApprovalNode / attractor wait.human): if this job's class is in an
    // operator-enabled approval band, hold it unless the frame carries a valid approval grant (an
    // HMAC over the job fingerprint a human authorized). Placed before the breaker/budget so a
    // held job consumes neither the loop window nor the budget — the orchestrator approves out of
    // band and re-dispatches with the grant. Inert unless a band is enabled (FXRUN_APPROVAL_BANDS).
    if approval.requires(&job) {
        let approved = req
            .approval
            .as_ref()
            .is_some_and(|grant| grant.verify(key, &fingerprint(&job)));
        if !approved {
            let directive = policy.decide(retry, &fingerprint(&job), FailureKind::ApprovalRequired);
            let detail = format!(
                "job class requires human approval and no valid grant was presented | {}",
                directive.summary()
            );
            sink.emit(
                &DispatchEvent::for_job(Outcome::ApprovalRequired, &job)
                    .with_recovery(directive.clone())
                    .with_detail(&detail),
            );
            return DispatchResponse::rejected(detail).with_recovery(directive);
        }
    }

    // Quarantine gate (automaton child `→ dead` lifecycle): a fingerprint that has failed at the
    // kernel `threshold` times is in a terminal quarantined state — refuse it fail-closed here, BEFORE
    // the breaker/budget (so it pollutes neither the loop window nor the budget) and before the
    // kernel. This is the enforcement teeth behind recovery's "escalate" advice: recovery only
    // *recommends* a human look; quarantine *stops* re-dispatch of structurally-doomed work until an
    // operator re-arms. Inert unless FXRUN_QUARANTINE_THRESHOLD is set.
    if quarantine_policy.is_active() && quarantine.is_quarantined(&fingerprint(&job)) {
        let directive = policy.decide(retry, &fingerprint(&job), FailureKind::Quarantined);
        let detail = format!(
            "job fingerprint is quarantined ({}x kernel failures ≥ threshold {}); refusing \
             re-dispatch until the runner is re-armed | {}",
            quarantine.failures(&fingerprint(&job)),
            quarantine_policy.threshold(),
            directive.summary()
        );
        sink.emit(
            &DispatchEvent::for_job(Outcome::Quarantined, &job)
                .with_recovery(directive.clone())
                .with_detail(&detail),
        );
        return DispatchResponse::rejected(detail).with_recovery(directive);
    }

    // Delegation-target allowlist (fail-closed kernel reachability): route the authenticated job to
    // its kernel, then ensure that kernel endpoint is currently reachable by operator policy before
    // it can consume rate slots, breaker window, budget, or a real subprocess. This is kernel
    // reachability only (`loop`/`atc`/`hf`/`weave`), not model/vendor selection (weave owns that).
    let plan = router::route(&job);
    let program = plan.kernel.program();
    if let TargetDecision::Denied { kernel, allowed } = target_allowlist.check(plan.kernel) {
        let directive = policy.decide(retry, &fingerprint(&job), FailureKind::TargetDenied);
        let detail = format!(
            "delegation target `{}` denied by FXRUN_KERNEL_ALLOWLIST (allowed: {allowed}); refusing before kernel invocation | {}",
            kernel.program(),
            directive.summary()
        );
        sink.emit(
            &DispatchEvent::for_job(Outcome::TargetDenied, &job)
                .with_kernel(kernel.program())
                .with_recovery(directive.clone())
                .with_detail(&detail),
        );
        return DispatchResponse::rejected(detail).with_recovery(directive);
    }

    // Per-target single-flight mutex (Archon older-wins locks + kclaw0 maxContainers prior art):
    // serialize mutable work for the same repo/target. The server is currently one-connection at a
    // time, so the global max-in-flight cap remains P3-concurrency-gated; this per-target seam is
    // still meaningful and tested now, and becomes active as soon as dispatches can overlap. A busy
    // target pollutes neither rate slots, breaker window, nor budget.
    let _singleflight_permit = match singleflight.try_acquire(&job) {
        Ok(lease) => SingleFlightPermit::new(singleflight, lease),
        Err(denied) => {
            let directive =
                policy.decide(retry, &fingerprint(&job), FailureKind::SingleFlightDenied);
            let detail = format!(
                "single-flight target `{}` is already held by older job `{}` (seq {}); incoming job `{}` (seq {}) waits/escalates | {}",
                denied.target,
                denied.holder_job_id,
                denied.holder_sequence,
                denied.incoming_job_id,
                denied.incoming_sequence,
                directive.summary()
            );
            sink.emit(
                &DispatchEvent::for_job(Outcome::SingleFlightDenied, &job)
                    .with_recovery(directive.clone())
                    .with_detail(&detail),
            );
            return DispatchResponse::rejected(detail).with_recovery(directive);
        }
    };

    // Dispatch rate limit + per-route failure cooldown (automaton hourly/daily caps + 5-min error
    // backoff): bound the *rate* of distinct, in-budget dispatches — the timing axis the breaker
    // (same-job loops), governor (lifetime budget), and quarantine (repeat-failure) don't cover.
    // Placed before the breaker/budget so a rate-refused job pollutes neither the loop window nor the
    // budget. A rate refusal is NOT a job failure: it carries a retry-after the orchestrator honours,
    // and it never touches the recovery retry budget (a busy runner must not escalate a job to a
    // human). Inert unless FXRUN_RATE_MAX / FXRUN_ROUTE_COOLDOWN_SECS is set.
    if rate_limiter.policy().is_active() {
        let route = job.job.class();
        if let RateDecision::Denied {
            reason,
            retry_after_secs,
        } = rate_limiter.check(route, now_secs)
        {
            // Advise a plain retry-after — attempt 0 so it never consumes the per-fingerprint retry
            // ledger (this is back-pressure, not a failing job).
            let directive = RecoveryDirective {
                action: RecoveryVerb::Retry,
                attempt: 0,
                max_retries: policy.max_retries(),
                backoff_secs: retry_after_secs,
                reason: reason.clone(),
            };
            let detail = format!("{reason} | {}", directive.summary());
            sink.emit(
                &DispatchEvent::for_job(Outcome::RateLimited, &job)
                    .with_recovery(directive.clone())
                    .with_detail(&detail),
            );
            return DispatchResponse::rejected(detail).with_recovery(directive);
        }
    }

    // Runaway-loop circuit breaker: a self-hosted autonomous loop dispatching the SAME work over
    // and over is the #1 unattended-loop failure mode (cost blowups). Trip fail-closed before the
    // kernel is touched. Distinct work and normal retries pass; only a tight identical loop trips.
    if let Verdict::Trip { count } = guard.observe(&job) {
        // A tripped loop is NOT retryable — retrying is exactly what's going wrong. Recovery
        // escalates to a human (open a review PR) rather than advising another re-dispatch.
        let directive = policy.decide(retry, &fingerprint(&job), FailureKind::LoopTripped);
        let detail = format!(
            "loop breaker tripped: identical job dispatched {count}x within the recent window \
             (runaway-loop guard) | {}",
            directive.summary()
        );
        sink.emit(
            &DispatchEvent::for_job(Outcome::LoopTripped, &job)
                .with_recovery(directive.clone())
                .with_detail(&detail),
        );
        return DispatchResponse::rejected(detail).with_recovery(directive);
    }

    // Dispatch budget (bounded autonomy): refuse once any operator-set ceiling (jobs/tokens/USD) is
    // already met, so an unattended loop can't run away. Checked after the breaker so a refused-loop
    // job costs no budget. Unlimited by default; cost dimensions only bite once atc reports cost.
    if let Admission::Denied { reason } = governor.admit() {
        let detail =
            format!("{reason} (bounded-autonomy kill-switch); re-arm the runner to continue");
        sink.emit(&DispatchEvent::for_job(Outcome::BudgetDenied, &job).with_detail(&detail));
        return DispatchResponse::rejected(detail);
    }

    let fp = fingerprint(&job);
    // History-calibrated risk score for this fingerprint (advice-only) — computed from the record
    // *before* this dispatch's own outcome, so it predicts rather than reflects. `None` when risk
    // annotation is disabled (the default), leaving the audit line unchanged.
    let risk = risk_policy.assess(risk_ledger, &fp);
    // Effective wall-clock deadline for THIS delegation: the tighter of the operator ceiling
    // (FXRUN_DEFAULT_DEADLINE_SECS) and the job's own envelope request. `None` → no bound (the
    // watchdog stays disengaged and the invoker is called directly — the default, unchanged path).
    let effective_deadline = deadline.effective(req.deadline_secs);
    match run_delegation(invoker, &plan, &job, effective_deadline) {
        Delegation::Delivered(cost) => {
            // Charge the cost atc reported so the NEXT admit sees it (fail-open: ZERO is a no-op).
            governor.charge(cost);
            // A clean delegation clears this fingerprint's retry budget AND its quarantine failure
            // count, so a *later* transient failure of the same work starts its own fresh count.
            retry.clear(&fp);
            quarantine.clear(&fp);
            // A healthy delegation also releases the route's failure cooldown early (one success
            // means the route is working again).
            rate_limiter.clear_route(job.job.class());
            // Record this clean outcome so the *next* dispatch's risk score reflects it.
            risk_ledger.record(&fp, true);
            let mut event = DispatchEvent::for_job(Outcome::Delegated, &job)
                .with_kernel(program)
                .with_risk(risk);
            if cost.is_measured() {
                event = event.with_cost(cost);
            }
            // Advisory notes in the audit detail: the survival tier once a budget dimension passes 75%
            // (automaton's balance ladder), and the history-calibrated risk score (kclaw0
            // path-simulator) — both observability the operator/weave acts on, neither blocks.
            let tier = governor.tier();
            let mut notes = Vec::new();
            if tier.is_degraded() {
                notes.push(format!("survival tier: {tier}"));
            }
            if let Some(r) = risk {
                notes.push(r.summary());
            }
            if !notes.is_empty() {
                event = event.with_detail(notes.join(" | "));
            }
            sink.emit(&event);
            DispatchResponse {
                accepted: true,
                kernel: Some(program.to_string()),
                placement: Some(format!("{placement:?}")),
                intent: Some(plan.intent.clone()),
                error: None,
                recovery: None,
            }
        }
        // A kernel error is usually transient → recovery advises a backed-off retry, escalating to a
        // human only once the retry ceiling is exceeded; the failure also feeds the quarantine ledger.
        Delegation::Failed(e) => {
            risk_ledger.record(&fp, false);
            // Classify the kernel error (Archon FATAL-before-TRANSIENT precedence): an auth/permission/
            // config failure is fatal → escalate immediately rather than burning the retry budget on a
            // job that can only fail the same way; anything else is transient → retry-with-backoff.
            let (kind, outcome, lead) = match classify_kernel_error(&e) {
                FailureKind::KernelFatal => (
                    FailureKind::KernelFatal,
                    Outcome::KernelFatal,
                    format!("kernel `{program}` returned a FATAL (unrecoverable) error: {e}"),
                ),
                _ => (
                    FailureKind::KernelFailed,
                    Outcome::KernelFailed,
                    format!("kernel `{program}` invocation failed: {e}"),
                ),
            };
            handle_failure(
                policy,
                retry,
                quarantine_policy,
                quarantine,
                rate_limiter,
                now_secs,
                risk,
                sink,
                &job,
                program,
                &fp,
                kind,
                outcome,
                lead,
            )
        }
        // A hung / over-long delegation: bounded by the wall-clock deadline, abandoned, and routed
        // through the same recovery + quarantine path as a kernel error (the time axis the breaker /
        // governor / quarantine-by-failure don't otherwise cover).
        Delegation::TimedOut(limit) => {
            risk_ledger.record(&fp, false);
            handle_failure(
                policy,
                retry,
                quarantine_policy,
                quarantine,
                rate_limiter,
                now_secs,
                risk,
                sink,
                &job,
                program,
                &fp,
                FailureKind::DeadlineExceeded,
                Outcome::DeadlineExceeded,
                format!(
                    "kernel `{program}` exceeded its {}s wall-clock deadline (hung / ran long) — abandoned",
                    limit.as_secs()
                ),
            )
        }
    }
}

/// Accept exactly one connection, handle its frame, and write the reply. Factored out so the loop
/// (and tests) can drive a single round-trip.
#[cfg(unix)]
#[allow(clippy::too_many_arguments)]
fn serve_once(
    listener: &std::os::unix::net::UnixListener,
    key: &[u8],
    invoker: &(dyn KernelInvoker + Sync),
    constitution: &Constitution,
    approval: &ApprovalPolicy,
    authority: &AuthorityPolicy,
    target_allowlist: &TargetAllowlist,
    singleflight: &mut SingleFlight,
    guard: &mut LoopGuard,
    governor: &mut Governor,
    policy: &RecoveryPolicy,
    retry: &mut RetryLedger,
    quarantine_policy: &QuarantinePolicy,
    quarantine: &mut QuarantineLedger,
    rate_limiter: &mut RateLimiter,
    now_secs: u64,
    scan_policy: &ScanPolicy,
    risk_policy: &RiskPolicy,
    risk_ledger: &mut RiskLedger,
    deadline: &DeadlinePolicy,
    redactor: &Redactor,
    sink: &dyn EventSink,
) -> std::io::Result<()> {
    use std::io::Write;
    let (mut stream, _addr) = listener.accept()?;
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw)?;
    // Re-verify the runner's constitution against the files on disk right now (I/O lives here).
    let status = constitution.verify(|name| std::fs::read(name).ok());
    let resp = handle_request(
        key,
        invoker,
        &status,
        approval,
        authority,
        target_allowlist,
        singleflight,
        guard,
        governor,
        policy,
        retry,
        quarantine_policy,
        quarantine,
        rate_limiter,
        now_secs,
        scan_policy,
        risk_policy,
        risk_ledger,
        deadline,
        sink,
        &raw,
    );
    // Scrub any registered secret out of the error reply before it crosses the socket (the audit-log
    // half is already scrubbed by the RedactingSink that wraps `sink`).
    let resp = redact_response(redactor, resp);
    let bytes = serde_json::to_vec(&resp)
        .unwrap_or_else(|_| br#"{"accepted":false,"error":"response encode failed"}"#.to_vec());
    stream.write_all(&bytes)?;
    stream.flush()?;
    Ok(())
}

/// Bind `socket_path` and serve forever (one job per connection — the ephemeral-runner model).
/// Removes a stale socket first; a per-connection error is logged and the loop continues. The
/// [`LoopGuard`] is owned here so its runaway-loop history spans connections.
#[cfg(unix)]
#[allow(clippy::too_many_arguments)]
fn serve(
    socket_path: &std::path::Path,
    key: &[u8],
    invoker: &(dyn KernelInvoker + Sync),
    constitution: &Constitution,
    approval: &ApprovalPolicy,
    authority: &AuthorityPolicy,
    target_allowlist: &TargetAllowlist,
    singleflight: &mut SingleFlight,
    guard: &mut LoopGuard,
    governor: &mut Governor,
    policy: &RecoveryPolicy,
    retry: &mut RetryLedger,
    quarantine_policy: &QuarantinePolicy,
    quarantine: &mut QuarantineLedger,
    rate_limiter: &mut RateLimiter,
    scan_policy: &ScanPolicy,
    risk_policy: &RiskPolicy,
    risk_ledger: &mut RiskLedger,
    deadline: &DeadlinePolicy,
    redactor: &Redactor,
    sink: &dyn EventSink,
) -> std::io::Result<()> {
    use std::os::unix::net::UnixListener;
    if socket_path.exists() {
        std::fs::remove_file(socket_path)?;
    }
    // Monotonic clock for the rate limiter: seconds since the server started. Read per connection in
    // the loop below (clock I/O lives in the binary; runner-core's RateLimiter stays clock-free).
    let started = std::time::Instant::now();
    let listener = UnixListener::bind(socket_path)?;
    let constitution_note = if constitution.is_empty() {
        "constitution: none".to_string()
    } else {
        format!("constitution: {} sealed", constitution.len())
    };
    let approval_note = if approval.is_active() {
        format!("approval bands: {}", approval.enabled_bands().join(","))
    } else {
        "approval: none".to_string()
    };
    let authority_note = if authority.is_active() {
        format!("authority: {}", authority.describe())
    } else {
        "authority: off".to_string()
    };
    let target_note = if target_allowlist.is_active() {
        format!("target allowlist: {}", target_allowlist.describe())
    } else {
        "target allowlist: off".to_string()
    };
    let singleflight_note = "single-flight: per-target older-wins".to_string();
    let quarantine_note = if quarantine_policy.is_active() {
        format!("quarantine: {} failures", quarantine_policy.threshold())
    } else {
        "quarantine: off".to_string()
    };
    let deadline_note = match deadline.default_secs() {
        Some(s) => format!("deadline: {s}s cap"),
        None => "deadline: none".to_string(),
    };
    let redaction_note = if redactor.is_active() {
        format!("redaction: {} secret(s)", redactor.secret_count())
    } else {
        "redaction: off".to_string()
    };
    let rate_note = {
        let p = rate_limiter.policy();
        match (p.max_per_window(), p.route_cooldown_secs()) {
            (None, None) => "rate limit: off".to_string(),
            (max, cooldown) => {
                let mut parts = Vec::new();
                if let Some(m) = max {
                    parts.push(format!("{m}/{}s", p.window_secs()));
                }
                if let Some(c) = cooldown {
                    parts.push(format!("{c}s route cooldown"));
                }
                format!("rate limit: {}", parts.join(" + "))
            }
        }
    };
    let scan_note = match scan_policy.threshold() {
        Some(s) => format!("content scan: block ≥ {s}"),
        None => "content scan: off".to_string(),
    };
    let risk_note = if risk_policy.is_active() {
        "risk score: on".to_string()
    } else {
        "risk score: off".to_string()
    };
    eprintln!(
        "fxrun-dispatch: listening on {} (loop breaker: {} identical / window {}; dispatch budget: {}; recovery: {} retries / {}s base backoff; {}; {}; {}; {}; {}; {}; {}; {}; {}; {}; {})",
        socket_path.display(),
        guard.trip_threshold(),
        guard.window(),
        render_budget(&governor.budget()),
        policy.max_retries(),
        policy.base_backoff_secs(),
        quarantine_note,
        deadline_note,
        rate_note,
        scan_note,
        risk_note,
        redaction_note,
        approval_note,
        authority_note,
        target_note,
        singleflight_note,
        constitution_note
    );
    loop {
        // Read the monotonic clock just before each connection is served. Under load `accept`
        // returns promptly so this is fresh; under idle the staleness is irrelevant (no rate pressure).
        let now_secs = started.elapsed().as_secs();
        if let Err(e) = serve_once(
            &listener,
            key,
            invoker,
            constitution,
            approval,
            authority,
            target_allowlist,
            singleflight,
            guard,
            governor,
            policy,
            retry,
            quarantine_policy,
            quarantine,
            rate_limiter,
            now_secs,
            scan_policy,
            risk_policy,
            risk_ledger,
            deadline,
            redactor,
            sink,
        ) {
            eprintln!("fxrun-dispatch: connection error: {e}");
        }
    }
}

fn stdin_dry_run(key: &str) -> anyhow::Result<()> {
    let sig = std::env::var("FXRUN_DISPATCH_SIG").unwrap_or_default();
    let mut buf = String::new();
    std::io::stdin().read_to_string(&mut buf)?;
    if buf.trim().is_empty() {
        eprintln!(
            "fxrun-dispatch: pipe a JSON JobSpec on stdin (dry-run), or run with `--socket <path>` \
             to serve. Set FXRUN_DISPATCH_KEY (+ FXRUN_DISPATCH_SIG for stdin) to verify."
        );
        return Ok(());
    }
    let job: JobSpec = serde_json::from_str(&buf)?;
    let verified = if key.is_empty() {
        eprintln!("WARNING: no dispatch key (P3: envctl); treating spec as UNVERIFIED.");
        false
    } else {
        job.verify(key.as_bytes(), &sig)
            .map_err(|e| anyhow::anyhow!("signature verification failed: {e}"))?;
        true
    };
    let plan = router::route(&job);
    let place = safety::placement(&job);
    let agent = match plan.agent {
        Some(a) => format!(" agent={a}"),
        None => String::new(),
    };
    println!(
        "verified={verified} placement={place:?} kernel={:?} program={}{agent} intent='{}'",
        plan.kernel,
        plan.kernel.program(),
        plan.intent
    );
    Ok(())
}

/// Build the egress redactor from the dispatcher's configured secrets: the HMAC dispatch `key`
/// (the primary secret the runner holds) plus any extra comma-separated strings in
/// `FXRUN_REDACT_SECRETS` (e.g. an envctl-injected bearer the App embeds). Each candidate is filtered
/// by [`Redactor::register`] (too-short stand-ins are dropped). Not `#[cfg(unix)]`: it's pure and
/// unit-tested on every OS; the top-of-file `allow(dead_code)` covers its non-Unix unused state.
fn build_redactor(key: &str) -> Redactor {
    let mut redactor = Redactor::new();
    // The dispatch key itself must never surface in a log line or an error reply.
    redactor.register(key);
    if let Ok(extra) = std::env::var("FXRUN_REDACT_SECRETS") {
        for secret in extra.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            redactor.register(secret);
        }
    }
    redactor
}

/// Scrub a [`DispatchResponse`]'s `error` (the only free-text field that crosses the socket) through
/// `redactor` before it is serialized to the client — the wire-reply half of the redaction seam
/// (the audit-log half is the [`RedactingSink`] decorator). When nothing matches, the original
/// `error` string is reused (no allocation). Pure / cross-platform (testable on every OS).
fn redact_response(redactor: &Redactor, mut resp: DispatchResponse) -> DispatchResponse {
    if let Some(err) = resp.error.take() {
        resp.error = Some(match redactor.redact(&err) {
            std::borrow::Cow::Borrowed(_) => err,
            std::borrow::Cow::Owned(scrubbed) => scrubbed,
        });
    }
    resp
}

/// Read a positive `usize` from `var`, falling back to `default` when unset/empty/unparseable.
#[cfg(unix)]
fn env_usize(var: &str, default: usize) -> usize {
    std::env::var(var)
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(default)
}

/// Read a positive `u64` from `var` (`0`/unset/unparseable → `0`, meaning "uncapped").
#[cfg(unix)]
fn env_u64(var: &str) -> u64 {
    std::env::var(var)
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(0)
}

/// Whether `var` is set to a truthy flag (`1`/`true`/`yes`/`on`, case-insensitive).
#[cfg(unix)]
fn env_flag(var: &str) -> bool {
    std::env::var(var)
        .map(|v| {
            matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

/// Render a [`Budget`] for the startup banner: the capped dimensions, or "unlimited".
#[cfg(unix)]
fn render_budget(b: &runner_core::governor::Budget) -> String {
    let mut parts = Vec::new();
    if let Some(j) = b.jobs {
        parts.push(format!("{j} jobs"));
    }
    if let Some(t) = b.tokens {
        parts.push(format!("{t} tokens"));
    }
    if let Some(u) = b.usd_micros {
        parts.push(format!("${:.4}", JobCost::new(0, u).usd()));
    }
    if parts.is_empty() {
        return "unlimited".to_string();
    }
    let mut rendered = parts.join(" / ");
    if b.grace > 0 {
        rendered.push_str(&format!(" (grace {})", b.grace));
    }
    rendered
}

fn main() -> anyhow::Result<()> {
    // P3: fetch from envctl's vault, not the environment.
    let key = std::env::var("FXRUN_DISPATCH_KEY").unwrap_or_default();
    let args: Vec<String> = std::env::args().collect();

    if let Some(i) = args.iter().position(|a| a == "--socket") {
        #[cfg(unix)]
        {
            let path = args
                .get(i + 1)
                .ok_or_else(|| anyhow::anyhow!("--socket requires a path"))?;
            // Fail-closed: a server that can't verify frames must not start.
            if key.is_empty() {
                anyhow::bail!(
                    "refusing to serve without FXRUN_DISPATCH_KEY (P3: injected from envctl's vault)"
                );
            }
            // The breaker spans the whole server lifetime. Operators may tune it via env
            // (FXRUN_LOOP_WINDOW / FXRUN_LOOP_THRESHOLD); defaults mirror kclaw0 (4 in 8).
            let window = env_usize("FXRUN_LOOP_WINDOW", 8);
            let threshold = env_usize("FXRUN_LOOP_THRESHOLD", 4);
            let mut guard = LoopGuard::new(window, threshold);
            // Per-target single-flight mutex: serializes mutable work for the same repo/target with
            // deterministic older-wins semantics. The current accept loop is single-connection, so a
            // global max-in-flight cap remains concurrency-gated; this seam becomes active as soon as
            // serve goes concurrent.
            let mut singleflight = SingleFlight::new();
            // Bounded-autonomy ceiling across jobs/tokens/USD: 0/unset → uncapped per dimension
            // (behaviour-preserving default). Token/USD caps only bite once atc reports cost.
            // FXRUN_BUDGET_GRACE is the debounced floor (admits allowed past a met cap before halt;
            // 0 = strict cliff, the default — preserves the pre-existing deny-at-cap behaviour).
            let mut governor = Governor::from_env(
                env_usize("FXRUN_DISPATCH_BUDGET", 0),
                env_u64("FXRUN_TOKEN_BUDGET"),
                env_u64("FXRUN_USD_MICROS_BUDGET"),
                env_usize("FXRUN_BUDGET_GRACE", 0),
            );
            // Declarative recovery: a failed dispatch is answered with retry-with-backoff (transient
            // kernel errors) or escalate-to-human advice the orchestrator acts on. Tunable via env
            // (FXRUN_MAX_RETRIES / FXRUN_RETRY_BACKOFF_SECS); defaults to 2 retries, 5s base backoff.
            // `0` retries is a valid choice (escalate immediately) and is honored, not overridden.
            let max_retries = std::env::var("FXRUN_MAX_RETRIES")
                .ok()
                .and_then(|v| v.trim().parse::<u32>().ok())
                .unwrap_or(2);
            let base_backoff = std::env::var("FXRUN_RETRY_BACKOFF_SECS")
                .ok()
                .and_then(|v| v.trim().parse::<u64>().ok())
                .filter(|n| *n > 0)
                .unwrap_or(5);
            let policy = RecoveryPolicy::new(max_retries, base_backoff);
            let mut retry = RetryLedger::new();
            // Quarantine: after FXRUN_QUARANTINE_THRESHOLD kernel failures of the same fingerprint,
            // latch it terminal and refuse re-dispatch until the runner is re-armed. 0/unset = off
            // (behaviour-preserving) — the enforcement teeth behind recovery's escalate advice.
            let quarantine_threshold = std::env::var("FXRUN_QUARANTINE_THRESHOLD")
                .ok()
                .and_then(|v| v.trim().parse::<u32>().ok())
                .unwrap_or(0);
            let quarantine_policy = QuarantinePolicy::new(quarantine_threshold);
            let mut quarantine = QuarantineLedger::new();
            // Per-job wall-clock deadline ceiling: FXRUN_DEFAULT_DEADLINE_SECS bounds a *hung*
            // delegation (the time axis the breaker/governor/quarantine don't cover). 0/unset = no
            // cap (behaviour-preserving) — a job may still request a tighter deadline on its envelope.
            let deadline = DeadlinePolicy::from_secs(env_u64("FXRUN_DEFAULT_DEADLINE_SECS"));
            // Dispatch rate limit + per-route failure cooldown: at most FXRUN_RATE_MAX dispatches per
            // FXRUN_RATE_WINDOW_SECS (default 60s), and FXRUN_ROUTE_COOLDOWN_SECS of backoff for a
            // route after it fails. The timing axis the other guards don't cover. 0/unset = off
            // (behaviour-preserving).
            let mut rate_limiter = RateLimiter::new(RateLimitPolicy::from_env(
                env_usize("FXRUN_RATE_MAX", 0) as u32,
                env_u64("FXRUN_RATE_WINDOW_SECS"),
                env_u64("FXRUN_ROUTE_COOLDOWN_SECS"),
            ));
            // Content/injection scan: refuse a job whose free-text fields trip the pattern bank at or
            // above FXRUN_SCAN_BLOCK_SEVERITY (low|medium|high|critical). Unset/off → inert
            // (behaviour-preserving) — the scan never runs and no job is refused on content.
            let scan_policy = ScanPolicy::from_env(
                &std::env::var("FXRUN_SCAN_BLOCK_SEVERITY").unwrap_or_default(),
            );
            // History-calibrated risk score: when FXRUN_RISK_ANNOTATE is truthy, annotate each
            // delegated/failed audit event with a smoothed per-fingerprint failure probability
            // (advice-only; never blocks). Off by default — the audit stream is unchanged.
            let risk_policy =
                RiskPolicy::from_env(&std::env::var("FXRUN_RISK_ANNOTATE").unwrap_or_default());
            let mut risk_ledger = RiskLedger::new();
            // Constitution: seal the runner's own governing files (comma-separated paths in
            // FXRUN_CONSTITUTION) at startup; a mid-run change refuses all dispatch. Inert if unset.
            let constitution = {
                let paths = std::env::var("FXRUN_CONSTITUTION").unwrap_or_default();
                let names: Vec<String> = paths
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(String::from)
                    .collect();
                let c = Constitution::seal(&names, |name| std::fs::read(name).ok());
                if !c.is_empty() {
                    eprintln!(
                        "fxrun-dispatch: sealed constitution ({} files): {}",
                        c.len(),
                        c.names().collect::<Vec<_>>().join(", ")
                    );
                }
                c
            };
            // Human-approval bands: job classes (ci/review/agent/cycle) that require a human grant
            // before dispatch. Empty/unset → nothing requires approval (behaviour-preserving).
            let approval = ApprovalPolicy::from_bands(
                &std::env::var("FXRUN_APPROVAL_BANDS").unwrap_or_default(),
            );
            // Dispatch provenance / authority gate: optional per-route floors such as
            // `FXRUN_AUTHORITY_RULES=cycle=maintainer,agent=owner`. Older App frames carry no
            // submitter and still pass when no floor is configured; once a floor exists, missing
            // submitter provenance is `guest` and fails closed.
            let authority = AuthorityPolicy::from_rules(
                &std::env::var("FXRUN_AUTHORITY_RULES").unwrap_or_default(),
            )
            .map_err(|e| anyhow::anyhow!("invalid FXRUN_AUTHORITY_RULES: {e}"))?;
            // Delegation-target allowlist: optional fail-closed kernel reachability registry.
            // Unset → inert/backward compatible; set empty → active deny-all; set names (loop,atc,hf,
            // weave) → only those kernels can be reached. This is target reachability, not model
            // selection.
            let target_allowlist =
                TargetAllowlist::from_env(std::env::var("FXRUN_KERNEL_ALLOWLIST").ok().as_deref())
                    .map_err(|e| anyhow::anyhow!("invalid FXRUN_KERNEL_ALLOWLIST: {e}"))?;
            // Audit trail: every event to FXRUN_EVENT_LOG (when set); admission/guardrail
            // (policy-category) events ALSO to a distinct FXRUN_POLICY_LOG stream — automaton's
            // separate `policy_decisions` table, so guardrail tampering is auditable on its own.
            // Both unset → a no-op sink (default).
            let env_path = |var: &str| {
                std::env::var(var)
                    .ok()
                    .map(|v| v.trim().to_string())
                    .filter(|v| !v.is_empty())
                    .map(std::path::PathBuf::from)
            };
            let all_log = env_path("FXRUN_EVENT_LOG");
            let policy_log = env_path("FXRUN_POLICY_LOG");
            let base_sink: Box<dyn EventSink> = if all_log.is_none() && policy_log.is_none() {
                Box::new(NullSink)
            } else {
                if let Some(p) = &all_log {
                    eprintln!("fxrun-dispatch: audit log (all) → {}", p.display());
                }
                if let Some(p) = &policy_log {
                    eprintln!("fxrun-dispatch: policy-decision log → {}", p.display());
                }
                Box::new(RoutingSink {
                    all: all_log.map(|path| FileSink { path }),
                    policy: policy_log.map(|path| FileSink { path }),
                })
            };
            // Secret redaction: scrub the dispatch key (+ any FXRUN_REDACT_SECRETS) out of every
            // audit-log `detail` and every error reply, so key material can never land on disk or
            // cross the socket (Archon repo.ts token scrub). The key is always set in serve mode
            // (fail-closed above), so redaction is active here; inert only if no qualifying secret.
            // The secrets relayed into each kernel child (envctl → runner env → kernel child env).
            // Resolved here so their VALUES are registered with the redactor below (they must never
            // surface in the audit log / error replies, just like the dispatch key).
            let injected_secrets = resolve_injected_secrets();
            let mut redactor = build_redactor(&key);
            for (_name, value) in &injected_secrets {
                redactor.register(value);
            }
            if redactor.is_active() {
                eprintln!(
                    "fxrun-dispatch: secret redaction active ({} secret(s) scrubbed from audit log + error replies)",
                    redactor.secret_count()
                );
            }
            // Wrap the audit sink so every emitted event's detail is scrubbed before the file write.
            let sink: Box<dyn EventSink> =
                Box::new(RedactingSink::new(base_sink, redactor.clone()));
            // P3: spawn real kernels when execution is enabled (FXRUN_KERNEL_EXEC truthy, or any
            // FXRUN_KERNEL_CMD_* override set); otherwise keep the dry-run invoker so the runner still
            // routes + governs + audits with no kernels installed (behaviour-preserving default).
            let exec_enabled = env_flag("FXRUN_KERNEL_EXEC")
                || [
                    "FXRUN_KERNEL_CMD_LOOP",
                    "FXRUN_KERNEL_CMD_ATC",
                    "FXRUN_KERNEL_CMD_HF",
                    "FXRUN_KERNEL_CMD_WEAVE",
                ]
                .iter()
                .any(|v| {
                    std::env::var(v)
                        .map(|s| !s.trim().is_empty())
                        .unwrap_or(false)
                });
            let invoker: Box<dyn KernelInvoker + Sync> = if exec_enabled {
                let commands = KernelCommands::from_env();
                eprintln!(
                    "fxrun-dispatch: kernel EXECUTION enabled — loop=`{}` atc=`{}` hf=`{}` weave=`{}`; {} secret(s) injected",
                    commands.loop_lib, commands.atc, commands.hf, commands.weave, injected_secrets.len()
                );
                Box::new(SubprocessInvoker {
                    workspace: TempDirProvider,
                    commands,
                    secrets: injected_secrets,
                })
            } else {
                eprintln!(
                    "fxrun-dispatch: kernel execution DISABLED (dry-run) — set FXRUN_KERNEL_EXEC=1 (or FXRUN_KERNEL_CMD_*) to spawn real kernels"
                );
                Box::new(DryRunInvoker {
                    workspace: TempDirProvider,
                })
            };
            serve(
                std::path::Path::new(path),
                key.as_bytes(),
                invoker.as_ref(),
                &constitution,
                &approval,
                &authority,
                &target_allowlist,
                &mut singleflight,
                &mut guard,
                &mut governor,
                &policy,
                &mut retry,
                &quarantine_policy,
                &mut quarantine,
                &mut rate_limiter,
                &scan_policy,
                &risk_policy,
                &mut risk_ledger,
                &deadline,
                &redactor,
                sink.as_ref(),
            )?;
            return Ok(());
        }
        #[cfg(not(unix))]
        {
            let _ = i;
            anyhow::bail!("--socket (UDS dispatch) is only supported on Unix");
        }
    }

    stdin_dry_run(&key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use runner_core::authority::{AuthorityTier, Submitter};
    use runner_core::jobspec::{JobKind, JobSpec};
    use runner_core::wire::sign_frame;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default)]
    struct RecordingInvoker {
        calls: AtomicUsize,
        /// Cost each invocation reports back (the `atc → runner` seam); default ZERO (unmeasured).
        cost: JobCost,
    }
    impl RecordingInvoker {
        fn calls(&self) -> usize {
            self.calls.load(Ordering::Relaxed)
        }
        fn reporting(cost: JobCost) -> Self {
            Self {
                cost,
                ..Self::default()
            }
        }
    }
    impl KernelInvoker for RecordingInvoker {
        fn invoke(
            &self,
            _plan: &KernelPlan,
            _job: &JobSpec,
            _deadline: Option<std::time::Duration>,
        ) -> Delegation {
            self.calls.fetch_add(1, Ordering::Relaxed);
            Delegation::Delivered(self.cost)
        }
    }

    /// An invoker that always fails — exercises the kernel-failure → recovery path.
    #[derive(Default)]
    struct FailingInvoker {
        calls: AtomicUsize,
    }
    impl FailingInvoker {
        fn calls(&self) -> usize {
            self.calls.load(Ordering::Relaxed)
        }
    }
    impl KernelInvoker for FailingInvoker {
        fn invoke(
            &self,
            _plan: &KernelPlan,
            _job: &JobSpec,
            _deadline: Option<std::time::Duration>,
        ) -> Delegation {
            self.calls.fetch_add(1, Ordering::Relaxed);
            Delegation::Failed("kernel exploded".into())
        }
    }

    /// An invoker that returns a FATAL (auth) error — exercises the FATAL-first classifier path.
    #[derive(Default)]
    struct FatalInvoker {
        calls: AtomicUsize,
    }
    impl FatalInvoker {
        fn calls(&self) -> usize {
            self.calls.load(Ordering::Relaxed)
        }
    }
    impl KernelInvoker for FatalInvoker {
        fn invoke(
            &self,
            _plan: &KernelPlan,
            _job: &JobSpec,
            _deadline: Option<std::time::Duration>,
        ) -> Delegation {
            self.calls.fetch_add(1, Ordering::Relaxed);
            Delegation::Failed("HTTP 401 Unauthorized: invalid credentials".into())
        }
    }

    /// An invoker that self-enforces the deadline (as a real subprocess invoker does): if a deadline
    /// is set and shorter than its `sleep`, it reports [`Delegation::TimedOut`] (simulating a child
    /// kill) without actually sleeping; otherwise it sleeps and delivers. Deterministic — exercises
    /// the timeout → recovery → audit path with no wall-clock race.
    struct SlowInvoker {
        sleep: std::time::Duration,
    }
    impl KernelInvoker for SlowInvoker {
        fn invoke(
            &self,
            _plan: &KernelPlan,
            _job: &JobSpec,
            deadline: Option<std::time::Duration>,
        ) -> Delegation {
            match deadline {
                Some(limit) if self.sleep > limit => Delegation::TimedOut(limit),
                _ => {
                    std::thread::sleep(self.sleep);
                    Delegation::Delivered(JobCost::ZERO)
                }
            }
        }
    }

    fn ci_spec(from_fork: bool) -> JobSpec {
        JobSpec {
            id: "job-1".into(),
            correlation_id: "delivery-9".into(),
            from_fork,
            job: JobKind::Ci {
                repo: "FlexNetOS/meta".into(),
                head_sha: "abc123".into(),
            },
        }
    }

    #[test]
    fn accepts_signed_non_fork_job_and_delegates() {
        let inv = RecordingInvoker::default();
        let frame = sign_frame(b"k", &ci_spec(false)).unwrap();
        let raw = serde_json::to_vec(&frame).unwrap();
        let resp = handle_request(
            b"k",
            &inv,
            &ConstitutionStatus::Intact,
            &ApprovalPolicy::none(),
            &AuthorityPolicy::disabled(),
            &TargetAllowlist::disabled(),
            &mut SingleFlight::new(),
            &mut LoopGuard::default(),
            &mut Governor::unlimited(),
            &RecoveryPolicy::default(),
            &mut RetryLedger::new(),
            &QuarantinePolicy::disabled(),
            &mut QuarantineLedger::new(),
            &mut RateLimiter::disabled(),
            0,
            &ScanPolicy::disabled(),
            &RiskPolicy::disabled(),
            &mut RiskLedger::new(),
            &DeadlinePolicy::disabled(),
            &NullSink,
            &raw,
        );
        assert!(resp.accepted);
        assert_eq!(resp.kernel.as_deref(), Some("loop"));
        assert_eq!(resp.placement.as_deref(), Some("SelfHosted"));
        assert_eq!(inv.calls(), 1);
    }

    #[test]
    fn authority_gate_denies_low_tier_submitter_before_delegation() {
        let inv = RecordingInvoker::default();
        let mut frame = sign_frame(b"k", &ci_spec(false)).unwrap();
        frame.submitter = Some(Submitter::new("bot", AuthorityTier::Agent));
        let raw = serde_json::to_vec(&frame).unwrap();
        let resp = handle_request(
            b"k",
            &inv,
            &ConstitutionStatus::Intact,
            &ApprovalPolicy::none(),
            &AuthorityPolicy::from_rules("ci=maintainer").unwrap(),
            &TargetAllowlist::disabled(),
            &mut SingleFlight::new(),
            &mut LoopGuard::default(),
            &mut Governor::unlimited(),
            &RecoveryPolicy::default(),
            &mut RetryLedger::new(),
            &QuarantinePolicy::disabled(),
            &mut QuarantineLedger::new(),
            &mut RateLimiter::disabled(),
            0,
            &ScanPolicy::disabled(),
            &RiskPolicy::disabled(),
            &mut RiskLedger::new(),
            &DeadlinePolicy::disabled(),
            &NullSink,
            &raw,
        );
        assert!(!resp.accepted);
        assert!(resp
            .error
            .as_deref()
            .unwrap()
            .contains("authority gate denied"));
        assert_eq!(
            resp.recovery.as_ref().unwrap().action,
            RecoveryVerb::Escalate
        );
        assert_eq!(
            inv.calls(),
            0,
            "unauthorized submitter must never reach a kernel"
        );
    }

    #[test]
    fn authority_gate_allows_sufficient_submitter() {
        let inv = RecordingInvoker::default();
        let mut frame = sign_frame(b"k", &ci_spec(false)).unwrap();
        frame.submitter = Some(Submitter::new("alice", AuthorityTier::Maintainer));
        let raw = serde_json::to_vec(&frame).unwrap();
        let resp = handle_request(
            b"k",
            &inv,
            &ConstitutionStatus::Intact,
            &ApprovalPolicy::none(),
            &AuthorityPolicy::from_rules("ci=maintainer").unwrap(),
            &TargetAllowlist::disabled(),
            &mut SingleFlight::new(),
            &mut LoopGuard::default(),
            &mut Governor::unlimited(),
            &RecoveryPolicy::default(),
            &mut RetryLedger::new(),
            &QuarantinePolicy::disabled(),
            &mut QuarantineLedger::new(),
            &mut RateLimiter::disabled(),
            0,
            &ScanPolicy::disabled(),
            &RiskPolicy::disabled(),
            &mut RiskLedger::new(),
            &DeadlinePolicy::disabled(),
            &NullSink,
            &raw,
        );
        assert!(resp.accepted);
        assert_eq!(inv.calls(), 1);
    }

    #[test]
    fn singleflight_denies_competing_same_target_before_delegation() {
        let inv = RecordingInvoker::default();
        let mut singleflight = SingleFlight::new();
        let older = ci_spec(false);
        let _older_lease = singleflight.try_acquire(&older).unwrap();

        let mut newer = ci_spec(false);
        newer.id = "job-2".into();
        newer.correlation_id = "delivery-10".into();
        let frame = sign_frame(b"k", &newer).unwrap();
        let raw = serde_json::to_vec(&frame).unwrap();

        let resp = handle_request(
            b"k",
            &inv,
            &ConstitutionStatus::Intact,
            &ApprovalPolicy::none(),
            &AuthorityPolicy::disabled(),
            &TargetAllowlist::disabled(),
            &mut singleflight,
            &mut LoopGuard::default(),
            &mut Governor::unlimited(),
            &RecoveryPolicy::default(),
            &mut RetryLedger::new(),
            &QuarantinePolicy::disabled(),
            &mut QuarantineLedger::new(),
            &mut RateLimiter::disabled(),
            0,
            &ScanPolicy::disabled(),
            &RiskPolicy::disabled(),
            &mut RiskLedger::new(),
            &DeadlinePolicy::disabled(),
            &NullSink,
            &raw,
        );

        assert!(!resp.accepted);
        assert!(resp
            .error
            .as_deref()
            .unwrap()
            .contains("single-flight target"));
        assert_eq!(
            resp.recovery.as_ref().unwrap().action,
            RecoveryVerb::Escalate
        );
        assert_eq!(inv.calls(), 0, "busy target must never reach a kernel");
    }

    #[test]
    fn singleflight_releases_target_after_terminal_delegation() {
        let inv = RecordingInvoker::default();
        let mut singleflight = SingleFlight::new();

        for id in ["job-1", "job-2"] {
            let mut spec = ci_spec(false);
            spec.id = id.into();
            spec.correlation_id = format!("delivery-{id}");
            let frame = sign_frame(b"k", &spec).unwrap();
            let raw = serde_json::to_vec(&frame).unwrap();
            let resp = handle_request(
                b"k",
                &inv,
                &ConstitutionStatus::Intact,
                &ApprovalPolicy::none(),
                &AuthorityPolicy::disabled(),
                &TargetAllowlist::disabled(),
                &mut singleflight,
                &mut LoopGuard::default(),
                &mut Governor::unlimited(),
                &RecoveryPolicy::default(),
                &mut RetryLedger::new(),
                &QuarantinePolicy::disabled(),
                &mut QuarantineLedger::new(),
                &mut RateLimiter::disabled(),
                0,
                &ScanPolicy::disabled(),
                &RiskPolicy::disabled(),
                &mut RiskLedger::new(),
                &DeadlinePolicy::disabled(),
                &NullSink,
                &raw,
            );
            assert!(
                resp.accepted,
                "{id} should acquire after previous terminal release"
            );
        }
        assert_eq!(inv.calls(), 2);
        assert_eq!(singleflight.active_len(), 0);
    }

    #[test]
    fn target_allowlist_denies_disallowed_kernel_before_delegation() {
        let inv = RecordingInvoker::default();
        let frame = sign_frame(b"k", &ci_spec(false)).unwrap(); // CI routes to loop
        let raw = serde_json::to_vec(&frame).unwrap();
        let resp = handle_request(
            b"k",
            &inv,
            &ConstitutionStatus::Intact,
            &ApprovalPolicy::none(),
            &AuthorityPolicy::disabled(),
            &TargetAllowlist::from_env(Some("atc,hf,weave")).unwrap(),
            &mut SingleFlight::new(),
            &mut LoopGuard::default(),
            &mut Governor::unlimited(),
            &RecoveryPolicy::default(),
            &mut RetryLedger::new(),
            &QuarantinePolicy::disabled(),
            &mut QuarantineLedger::new(),
            &mut RateLimiter::disabled(),
            0,
            &ScanPolicy::disabled(),
            &RiskPolicy::disabled(),
            &mut RiskLedger::new(),
            &DeadlinePolicy::disabled(),
            &NullSink,
            &raw,
        );
        assert!(!resp.accepted);
        assert!(resp
            .error
            .as_deref()
            .unwrap()
            .contains("delegation target `loop` denied"));
        assert_eq!(
            resp.recovery.as_ref().unwrap().action,
            RecoveryVerb::Escalate
        );
        assert_eq!(
            inv.calls(),
            0,
            "disallowed target must never reach a kernel"
        );
    }

    #[test]
    fn target_allowlist_allows_listed_kernel() {
        let inv = RecordingInvoker::default();
        let frame = sign_frame(b"k", &ci_spec(false)).unwrap(); // CI routes to loop
        let raw = serde_json::to_vec(&frame).unwrap();
        let resp = handle_request(
            b"k",
            &inv,
            &ConstitutionStatus::Intact,
            &ApprovalPolicy::none(),
            &AuthorityPolicy::disabled(),
            &TargetAllowlist::from_env(Some("loop")).unwrap(),
            &mut SingleFlight::new(),
            &mut LoopGuard::default(),
            &mut Governor::unlimited(),
            &RecoveryPolicy::default(),
            &mut RetryLedger::new(),
            &QuarantinePolicy::disabled(),
            &mut QuarantineLedger::new(),
            &mut RateLimiter::disabled(),
            0,
            &ScanPolicy::disabled(),
            &RiskPolicy::disabled(),
            &mut RiskLedger::new(),
            &DeadlinePolicy::disabled(),
            &NullSink,
            &raw,
        );
        assert!(resp.accepted);
        assert_eq!(inv.calls(), 1);
    }

    #[test]
    fn fork_job_is_rejected_and_never_delegated() {
        let inv = RecordingInvoker::default();
        let frame = sign_frame(b"k", &ci_spec(true)).unwrap();
        let raw = serde_json::to_vec(&frame).unwrap();
        let resp = handle_request(
            b"k",
            &inv,
            &ConstitutionStatus::Intact,
            &ApprovalPolicy::none(),
            &AuthorityPolicy::disabled(),
            &TargetAllowlist::disabled(),
            &mut SingleFlight::new(),
            &mut LoopGuard::default(),
            &mut Governor::unlimited(),
            &RecoveryPolicy::default(),
            &mut RetryLedger::new(),
            &QuarantinePolicy::disabled(),
            &mut QuarantineLedger::new(),
            &mut RateLimiter::disabled(),
            0,
            &ScanPolicy::disabled(),
            &RiskPolicy::disabled(),
            &mut RiskLedger::new(),
            &DeadlinePolicy::disabled(),
            &NullSink,
            &raw,
        );
        assert!(!resp.accepted);
        assert!(resp.error.unwrap().contains("fork"));
        assert_eq!(inv.calls(), 0, "fork job must never reach a kernel");
    }

    #[test]
    fn bad_signature_is_rejected() {
        let inv = RecordingInvoker::default();
        let frame = sign_frame(b"k", &ci_spec(false)).unwrap();
        let raw = serde_json::to_vec(&frame).unwrap();
        let resp = handle_request(
            b"wrong-key",
            &inv,
            &ConstitutionStatus::Intact,
            &ApprovalPolicy::none(),
            &AuthorityPolicy::disabled(),
            &TargetAllowlist::disabled(),
            &mut SingleFlight::new(),
            &mut LoopGuard::default(),
            &mut Governor::unlimited(),
            &RecoveryPolicy::default(),
            &mut RetryLedger::new(),
            &QuarantinePolicy::disabled(),
            &mut QuarantineLedger::new(),
            &mut RateLimiter::disabled(),
            0,
            &ScanPolicy::disabled(),
            &RiskPolicy::disabled(),
            &mut RiskLedger::new(),
            &DeadlinePolicy::disabled(),
            &NullSink,
            &raw,
        );
        assert!(!resp.accepted);
        assert_eq!(inv.calls(), 0);
    }

    #[test]
    fn unparseable_frame_is_rejected() {
        let inv = RecordingInvoker::default();
        let resp = handle_request(
            b"k",
            &inv,
            &ConstitutionStatus::Intact,
            &ApprovalPolicy::none(),
            &AuthorityPolicy::disabled(),
            &TargetAllowlist::disabled(),
            &mut SingleFlight::new(),
            &mut LoopGuard::default(),
            &mut Governor::unlimited(),
            &RecoveryPolicy::default(),
            &mut RetryLedger::new(),
            &QuarantinePolicy::disabled(),
            &mut QuarantineLedger::new(),
            &mut RateLimiter::disabled(),
            0,
            &ScanPolicy::disabled(),
            &RiskPolicy::disabled(),
            &mut RiskLedger::new(),
            &DeadlinePolicy::disabled(),
            &NullSink,
            b"this is not json",
        );
        assert!(!resp.accepted);
        assert_eq!(inv.calls(), 0);
    }

    #[test]
    fn audit_log_records_the_outcome_of_each_decision() {
        use runner_core::events::DispatchEvent;
        use std::cell::RefCell;

        struct Recorder(RefCell<Vec<DispatchEvent>>);
        impl EventSink for Recorder {
            fn emit(&self, e: &DispatchEvent) {
                self.0.borrow_mut().push(e.clone());
            }
        }
        let sink = Recorder(RefCell::new(Vec::new()));
        let inv = RecordingInvoker::default();
        let mut guard = LoopGuard::default();
        let mut gov = Governor::unlimited();

        // A delegated job and a fork-rejected job produce one audit event each.
        let ok = sign_frame(b"k", &ci_spec(false)).unwrap();
        handle_request(
            b"k",
            &inv,
            &ConstitutionStatus::Intact,
            &ApprovalPolicy::none(),
            &AuthorityPolicy::disabled(),
            &TargetAllowlist::disabled(),
            &mut SingleFlight::new(),
            &mut guard,
            &mut gov,
            &RecoveryPolicy::default(),
            &mut RetryLedger::new(),
            &QuarantinePolicy::disabled(),
            &mut QuarantineLedger::new(),
            &mut RateLimiter::disabled(),
            0,
            &ScanPolicy::disabled(),
            &RiskPolicy::disabled(),
            &mut RiskLedger::new(),
            &DeadlinePolicy::disabled(),
            &sink,
            &serde_json::to_vec(&ok).unwrap(),
        );
        let forked = sign_frame(b"k", &ci_spec(true)).unwrap();
        handle_request(
            b"k",
            &inv,
            &ConstitutionStatus::Intact,
            &ApprovalPolicy::none(),
            &AuthorityPolicy::disabled(),
            &TargetAllowlist::disabled(),
            &mut SingleFlight::new(),
            &mut guard,
            &mut gov,
            &RecoveryPolicy::default(),
            &mut RetryLedger::new(),
            &QuarantinePolicy::disabled(),
            &mut QuarantineLedger::new(),
            &mut RateLimiter::disabled(),
            0,
            &ScanPolicy::disabled(),
            &RiskPolicy::disabled(),
            &mut RiskLedger::new(),
            &DeadlinePolicy::disabled(),
            &sink,
            &serde_json::to_vec(&forked).unwrap(),
        );
        // An unparseable frame is audited too — with no job fields.
        handle_request(
            b"k",
            &inv,
            &ConstitutionStatus::Intact,
            &ApprovalPolicy::none(),
            &AuthorityPolicy::disabled(),
            &TargetAllowlist::disabled(),
            &mut SingleFlight::new(),
            &mut guard,
            &mut gov,
            &RecoveryPolicy::default(),
            &mut RetryLedger::new(),
            &QuarantinePolicy::disabled(),
            &mut QuarantineLedger::new(),
            &mut RateLimiter::disabled(),
            0,
            &ScanPolicy::disabled(),
            &RiskPolicy::disabled(),
            &mut RiskLedger::new(),
            &DeadlinePolicy::disabled(),
            &sink,
            b"garbage",
        );

        let events = sink.0.into_inner();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].outcome, Outcome::Delegated);
        assert_eq!(events[0].kernel.as_deref(), Some("loop"));
        assert_eq!(
            events[0].fingerprint,
            Some(runner_core::fingerprint(&ci_spec(false)))
        );
        assert_eq!(events[1].outcome, Outcome::ForkRejected);
        assert_eq!(events[2].outcome, Outcome::Unparseable);
        assert!(
            events[2].job_id.is_none(),
            "pre-parse event carries no job id"
        );
    }

    #[test]
    fn constitution_violation_refuses_everything_before_the_kernel() {
        let inv = RecordingInvoker::default();
        let mut guard = LoopGuard::default();
        let mut gov = Governor::unlimited();
        let frame = sign_frame(b"k", &ci_spec(false)).unwrap();
        let raw = serde_json::to_vec(&frame).unwrap();

        // A tampered constitution refuses a perfectly valid, signed, non-fork job — first gate.
        let violated = ConstitutionStatus::Violated {
            changed: vec![".handoff/policy.toml".to_string()],
        };
        let resp = handle_request(
            b"k",
            &inv,
            &violated,
            &ApprovalPolicy::none(),
            &AuthorityPolicy::disabled(),
            &TargetAllowlist::disabled(),
            &mut SingleFlight::new(),
            &mut guard,
            &mut gov,
            &RecoveryPolicy::default(),
            &mut RetryLedger::new(),
            &QuarantinePolicy::disabled(),
            &mut QuarantineLedger::new(),
            &mut RateLimiter::disabled(),
            0,
            &ScanPolicy::disabled(),
            &RiskPolicy::disabled(),
            &mut RiskLedger::new(),
            &DeadlinePolicy::disabled(),
            &NullSink,
            &raw,
        );
        assert!(!resp.accepted);
        assert!(resp.error.unwrap().contains("constitution violated"));
        assert_eq!(
            inv.calls(),
            0,
            "a tampered constitution must reach no kernel"
        );

        // Intact → the same job is delegated.
        let ok = handle_request(
            b"k",
            &inv,
            &ConstitutionStatus::Intact,
            &ApprovalPolicy::none(),
            &AuthorityPolicy::disabled(),
            &TargetAllowlist::disabled(),
            &mut SingleFlight::new(),
            &mut guard,
            &mut gov,
            &RecoveryPolicy::default(),
            &mut RetryLedger::new(),
            &QuarantinePolicy::disabled(),
            &mut QuarantineLedger::new(),
            &mut RateLimiter::disabled(),
            0,
            &ScanPolicy::disabled(),
            &RiskPolicy::disabled(),
            &mut RiskLedger::new(),
            &DeadlinePolicy::disabled(),
            &NullSink,
            &raw,
        );
        assert!(ok.accepted);
        assert_eq!(inv.calls(), 1);
    }

    #[test]
    fn loop_breaker_trips_on_repeated_identical_dispatch_and_spares_the_kernel() {
        let inv = RecordingInvoker::default();
        // Tight breaker: trip on the 2nd identical dispatch within a window of 4.
        let mut guard = LoopGuard::new(4, 2);
        let frame = sign_frame(b"k", &ci_spec(false)).unwrap();
        let raw = serde_json::to_vec(&frame).unwrap();

        // First identical dispatch is delegated…
        let first = handle_request(
            b"k",
            &inv,
            &ConstitutionStatus::Intact,
            &ApprovalPolicy::none(),
            &AuthorityPolicy::disabled(),
            &TargetAllowlist::disabled(),
            &mut SingleFlight::new(),
            &mut guard,
            &mut Governor::unlimited(),
            &RecoveryPolicy::default(),
            &mut RetryLedger::new(),
            &QuarantinePolicy::disabled(),
            &mut QuarantineLedger::new(),
            &mut RateLimiter::disabled(),
            0,
            &ScanPolicy::disabled(),
            &RiskPolicy::disabled(),
            &mut RiskLedger::new(),
            &DeadlinePolicy::disabled(),
            &NullSink,
            &raw,
        );
        assert!(first.accepted);
        assert_eq!(inv.calls(), 1);

        // …the second identical dispatch trips the breaker and never reaches the kernel.
        let second = handle_request(
            b"k",
            &inv,
            &ConstitutionStatus::Intact,
            &ApprovalPolicy::none(),
            &AuthorityPolicy::disabled(),
            &TargetAllowlist::disabled(),
            &mut SingleFlight::new(),
            &mut guard,
            &mut Governor::unlimited(),
            &RecoveryPolicy::default(),
            &mut RetryLedger::new(),
            &QuarantinePolicy::disabled(),
            &mut QuarantineLedger::new(),
            &mut RateLimiter::disabled(),
            0,
            &ScanPolicy::disabled(),
            &RiskPolicy::disabled(),
            &mut RiskLedger::new(),
            &DeadlinePolicy::disabled(),
            &NullSink,
            &raw,
        );
        assert!(!second.accepted);
        assert!(second.error.unwrap().contains("loop breaker"));
        assert_eq!(inv.calls(), 1, "tripped job must not be delegated");
    }

    #[test]
    fn dispatch_budget_denies_past_the_ceiling_and_spares_the_kernel() {
        let inv = RecordingInvoker::default();
        let mut guard = LoopGuard::default();
        let mut governor = Governor::with_jobs(2);

        let mut dispatch = |sha: &str| {
            let spec = JobSpec {
                id: format!("job-{sha}"),
                correlation_id: "c".into(),
                from_fork: false,
                job: JobKind::Ci {
                    repo: "FlexNetOS/meta".into(),
                    head_sha: sha.into(),
                },
            };
            let frame = sign_frame(b"k", &spec).unwrap();
            let raw = serde_json::to_vec(&frame).unwrap();
            handle_request(
                b"k",
                &inv,
                &ConstitutionStatus::Intact,
                &ApprovalPolicy::none(),
                &AuthorityPolicy::disabled(),
                &TargetAllowlist::disabled(),
                &mut SingleFlight::new(),
                &mut guard,
                &mut governor,
                &RecoveryPolicy::default(),
                &mut RetryLedger::new(),
                &QuarantinePolicy::disabled(),
                &mut QuarantineLedger::new(),
                &mut RateLimiter::disabled(),
                0,
                &ScanPolicy::disabled(),
                &RiskPolicy::disabled(),
                &mut RiskLedger::new(),
                &DeadlinePolicy::disabled(),
                &NullSink,
                &raw,
            )
        };

        assert!(dispatch("a").accepted); // 1/2
        assert!(dispatch("b").accepted); // 2/2
        let denied = dispatch("c"); // 3rd → over budget
        assert!(!denied.accepted);
        assert!(denied.error.unwrap().contains("budget exhausted"));
        assert_eq!(inv.calls(), 2, "over-budget job must not be delegated");
    }

    #[test]
    fn reported_cost_charges_the_token_budget_and_lands_in_the_audit_log() {
        use runner_core::events::DispatchEvent;
        use std::cell::RefCell;
        struct Rec(RefCell<Vec<DispatchEvent>>);
        impl EventSink for Rec {
            fn emit(&self, e: &DispatchEvent) {
                self.0.borrow_mut().push(e.clone());
            }
        }
        let sink = Rec(RefCell::new(Vec::new()));
        // atc reports 600 tokens per job; cap the session at 1000 tokens.
        let inv = RecordingInvoker::reporting(JobCost::new(600, 0));
        let mut guard = LoopGuard::default();
        let mut gov = Governor::with_budget(runner_core::governor::Budget {
            tokens: Some(1000),
            ..Default::default()
        });

        let mut dispatch = |sha: &str| {
            let spec = JobSpec {
                id: format!("job-{sha}"),
                correlation_id: "c".into(),
                from_fork: false,
                job: JobKind::Ci {
                    repo: "FlexNetOS/meta".into(),
                    head_sha: sha.into(),
                },
            };
            let frame = sign_frame(b"k", &spec).unwrap();
            handle_request(
                b"k",
                &inv,
                &ConstitutionStatus::Intact,
                &ApprovalPolicy::none(),
                &AuthorityPolicy::disabled(),
                &TargetAllowlist::disabled(),
                &mut SingleFlight::new(),
                &mut guard,
                &mut gov,
                &RecoveryPolicy::default(),
                &mut RetryLedger::new(),
                &QuarantinePolicy::disabled(),
                &mut QuarantineLedger::new(),
                &mut RateLimiter::disabled(),
                0,
                &ScanPolicy::disabled(),
                &RiskPolicy::disabled(),
                &mut RiskLedger::new(),
                &DeadlinePolicy::disabled(),
                &sink,
                &serde_json::to_vec(&frame).unwrap(),
            )
        };

        assert!(dispatch("a").accepted); // spent 600
        assert!(dispatch("b").accepted); // spent 1200 (admit happened before charge crossed cap)
        let denied = dispatch("c"); // 1200 >= 1000 → denied
        assert!(!denied.accepted);
        assert!(denied.error.unwrap().contains("token budget exhausted"));

        // The two delegated events carry the reported cost.
        let events = sink.0.into_inner();
        let costed: Vec<_> = events.iter().filter_map(|e| e.cost).collect();
        assert_eq!(costed, vec![JobCost::new(600, 0), JobCost::new(600, 0)]);
    }

    #[test]
    fn distinct_work_is_not_tripped_by_the_breaker() {
        let inv = RecordingInvoker::default();
        let mut guard = LoopGuard::new(4, 2);
        // Each dispatch is distinct work (varying head_sha) → never trips.
        for i in 0..6 {
            let spec = JobSpec {
                id: format!("job-{i}"),
                correlation_id: "c".into(),
                from_fork: false,
                job: JobKind::Ci {
                    repo: "FlexNetOS/meta".into(),
                    head_sha: format!("sha{i}"),
                },
            };
            let frame = sign_frame(b"k", &spec).unwrap();
            let raw = serde_json::to_vec(&frame).unwrap();
            assert!(
                handle_request(
                    b"k",
                    &inv,
                    &ConstitutionStatus::Intact,
                    &ApprovalPolicy::none(),
                    &AuthorityPolicy::disabled(),
                    &TargetAllowlist::disabled(),
                    &mut SingleFlight::new(),
                    &mut guard,
                    &mut Governor::unlimited(),
                    &RecoveryPolicy::default(),
                    &mut RetryLedger::new(),
                    &QuarantinePolicy::disabled(),
                    &mut QuarantineLedger::new(),
                    &mut RateLimiter::disabled(),
                    0,
                    &ScanPolicy::disabled(),
                    &RiskPolicy::disabled(),
                    &mut RiskLedger::new(),
                    &DeadlinePolicy::disabled(),
                    &NullSink,
                    &raw
                )
                .accepted
            );
        }
        assert_eq!(inv.calls(), 6);
    }

    #[test]
    fn grace_floor_lets_jobs_past_the_cap_then_halts_and_audits_the_tier() {
        use runner_core::events::DispatchEvent;
        use runner_core::governor::Budget;
        use std::cell::RefCell;
        struct Rec(RefCell<Vec<DispatchEvent>>);
        impl EventSink for Rec {
            fn emit(&self, e: &DispatchEvent) {
                self.0.borrow_mut().push(e.clone());
            }
        }
        let sink = Rec(RefCell::new(Vec::new()));
        let inv = RecordingInvoker::default();
        let mut guard = LoopGuard::default();
        // 1-job cap with a grace of 1: one normal admit, one distress (grace) admit, then halt.
        let mut gov = Governor::with_budget(Budget {
            jobs: Some(1),
            grace: 1,
            ..Default::default()
        });
        let policy = RecoveryPolicy::default();
        let mut retry = RetryLedger::new();

        let mut dispatch = |sha: &str, gov: &mut Governor| {
            let spec = JobSpec {
                id: format!("job-{sha}"),
                correlation_id: "c".into(),
                from_fork: false,
                job: JobKind::Ci {
                    repo: "FlexNetOS/meta".into(),
                    head_sha: sha.into(),
                },
            };
            let frame = sign_frame(b"k", &spec).unwrap();
            handle_request(
                b"k",
                &inv,
                &ConstitutionStatus::Intact,
                &ApprovalPolicy::none(),
                &AuthorityPolicy::disabled(),
                &TargetAllowlist::disabled(),
                &mut SingleFlight::new(),
                &mut guard,
                gov,
                &policy,
                &mut retry,
                &QuarantinePolicy::disabled(),
                &mut QuarantineLedger::new(),
                &mut RateLimiter::disabled(),
                0,
                &ScanPolicy::disabled(),
                &RiskPolicy::disabled(),
                &mut RiskLedger::new(),
                &DeadlinePolicy::disabled(),
                &sink,
                &serde_json::to_vec(&frame).unwrap(),
            )
        };

        assert!(dispatch("a", &mut gov).accepted); // 1/1 (under cap)
        assert!(dispatch("b", &mut gov).accepted); // grace admit past the cap (distress)
        assert!(!dispatch("c", &mut gov).accepted); // grace exhausted → halt
        assert_eq!(
            inv.calls(),
            2,
            "only the two admitted jobs reached the kernel"
        );

        // The grace (distress) delegation is audited with a survival-tier note (the cap is met).
        let events = sink.0.into_inner();
        let delegated: Vec<_> = events
            .iter()
            .filter(|e| e.outcome == Outcome::Delegated)
            .collect();
        assert_eq!(delegated.len(), 2);
        assert!(
            delegated[1]
                .detail
                .as_deref()
                .is_some_and(|d| d.contains("survival tier: halted")),
            "the over-cap grace admit should carry a halted-tier audit note"
        );
    }

    #[test]
    fn malformed_job_is_rejected_before_the_kernel_with_an_escalate_directive() {
        use runner_core::recovery::RecoveryVerb;
        let inv = RecordingInvoker::default();
        // Structurally invalid: empty repo. Signed correctly so it passes auth, then fails the lint.
        let spec = JobSpec {
            id: "job-x".into(),
            correlation_id: "c".into(),
            from_fork: false,
            job: JobKind::Ci {
                repo: "".into(),
                head_sha: "abc".into(),
            },
        };
        let frame = sign_frame(b"k", &spec).unwrap();
        let resp = handle_request(
            b"k",
            &inv,
            &ConstitutionStatus::Intact,
            &ApprovalPolicy::none(),
            &AuthorityPolicy::disabled(),
            &TargetAllowlist::disabled(),
            &mut SingleFlight::new(),
            &mut LoopGuard::default(),
            &mut Governor::unlimited(),
            &RecoveryPolicy::default(),
            &mut RetryLedger::new(),
            &QuarantinePolicy::disabled(),
            &mut QuarantineLedger::new(),
            &mut RateLimiter::disabled(),
            0,
            &ScanPolicy::disabled(),
            &RiskPolicy::disabled(),
            &mut RiskLedger::new(),
            &DeadlinePolicy::disabled(),
            &NullSink,
            &serde_json::to_vec(&frame).unwrap(),
        );
        assert!(!resp.accepted);
        assert!(resp.error.unwrap().contains("structurally-invalid"));
        let directive = resp
            .recovery
            .expect("malformed job carries a recovery directive");
        assert_eq!(directive.action, RecoveryVerb::Escalate);
        assert_eq!(inv.calls(), 0, "a malformed job must never reach a kernel");
    }

    #[test]
    fn kernel_failure_advises_retry_then_escalates_after_the_ceiling() {
        use runner_core::recovery::RecoveryVerb;
        let inv = FailingInvoker::default();
        let mut guard = LoopGuard::default();
        let mut gov = Governor::unlimited();
        // 1 retry, then escalate (5s base backoff). One shared ledger across the calls.
        let policy = RecoveryPolicy::new(1, 5);
        let mut retry = RetryLedger::new();

        // Each call uses distinct work (varying sha) so the *loop breaker* never fires — we are
        // exercising the kernel-failure recovery path, not the breaker. The retry ledger keys on
        // fingerprint, so to see attempt-counting we re-dispatch the SAME work.
        let frame = sign_frame(b"k", &ci_spec(false)).unwrap();
        let raw = serde_json::to_vec(&frame).unwrap();

        // 1st failure → retry (attempt 1)…
        let r1 = handle_request(
            b"k",
            &inv,
            &ConstitutionStatus::Intact,
            &ApprovalPolicy::none(),
            &AuthorityPolicy::disabled(),
            &TargetAllowlist::disabled(),
            &mut SingleFlight::new(),
            &mut guard,
            &mut gov,
            &policy,
            &mut retry,
            &QuarantinePolicy::disabled(),
            &mut QuarantineLedger::new(),
            &mut RateLimiter::disabled(),
            0,
            &ScanPolicy::disabled(),
            &RiskPolicy::disabled(),
            &mut RiskLedger::new(),
            &DeadlinePolicy::disabled(),
            &NullSink,
            &raw,
        );
        let d1 = r1.recovery.expect("kernel failure carries recovery");
        assert_eq!(d1.action, RecoveryVerb::Retry);
        assert_eq!(d1.attempt, 1);
        assert_eq!(d1.backoff_secs, 5);

        // The breaker would trip on the 4th identical dispatch (default 4-in-8); use a fresh guard
        // so only the retry ledger advances. 2nd failure of the same work → over the 1-retry
        // ceiling → escalate.
        let r2 = handle_request(
            b"k",
            &inv,
            &ConstitutionStatus::Intact,
            &ApprovalPolicy::none(),
            &AuthorityPolicy::disabled(),
            &TargetAllowlist::disabled(),
            &mut SingleFlight::new(),
            &mut LoopGuard::default(),
            &mut gov,
            &policy,
            &mut retry,
            &QuarantinePolicy::disabled(),
            &mut QuarantineLedger::new(),
            &mut RateLimiter::disabled(),
            0,
            &ScanPolicy::disabled(),
            &RiskPolicy::disabled(),
            &mut RiskLedger::new(),
            &DeadlinePolicy::disabled(),
            &NullSink,
            &raw,
        );
        let d2 = r2.recovery.expect("kernel failure carries recovery");
        assert_eq!(d2.action, RecoveryVerb::Escalate);
        assert_eq!(inv.calls(), 2, "both attempts actually reached the kernel");
    }

    #[test]
    fn repeated_kernel_failures_quarantine_the_fingerprint_then_refuse_re_dispatch() {
        use runner_core::quarantine::{QuarantineLedger, QuarantinePolicy};
        use runner_core::recovery::RecoveryVerb;

        // Quarantine after 2 kernel failures of the same fingerprint. Fresh guards each call so the
        // *loop breaker* never fires — we are exercising quarantine, an independent gate.
        let qpolicy = QuarantinePolicy::new(2);
        let mut qledger = QuarantineLedger::new();
        let policy = RecoveryPolicy::new(5, 1); // generous retry budget so recovery still advises retry
        let mut retry = RetryLedger::new();
        let frame = sign_frame(b"k", &ci_spec(false)).unwrap();
        let raw = serde_json::to_vec(&frame).unwrap();
        let fp = runner_core::fingerprint(&ci_spec(false));

        let fail = FailingInvoker::default();
        let fail_once = |q: &mut QuarantineLedger, r: &mut RetryLedger| {
            handle_request(
                b"k",
                &fail,
                &ConstitutionStatus::Intact,
                &ApprovalPolicy::none(),
                &AuthorityPolicy::disabled(),
                &TargetAllowlist::disabled(),
                &mut SingleFlight::new(),
                &mut LoopGuard::default(),
                &mut Governor::unlimited(),
                &policy,
                r,
                &qpolicy,
                q,
                &mut RateLimiter::disabled(),
                0,
                &ScanPolicy::disabled(),
                &RiskPolicy::disabled(),
                &mut RiskLedger::new(),
                &DeadlinePolicy::disabled(),
                &NullSink,
                &raw,
            )
        };

        // 1st failure: kernel reached, not yet quarantined.
        let r1 = fail_once(&mut qledger, &mut retry);
        assert!(!r1.accepted);
        assert!(!qledger.is_quarantined(&fp));
        assert_eq!(fail.calls(), 1);

        // 2nd failure: reaches the kernel, then latches quarantine (2 ≥ threshold 2).
        let r2 = fail_once(&mut qledger, &mut retry);
        assert!(!r2.accepted);
        assert!(qledger.is_quarantined(&fp));
        assert_eq!(fail.calls(), 2);

        // 3rd dispatch: refused at the quarantine gate — the kernel is NOT touched again.
        let ok = RecordingInvoker::default();
        let r3 = handle_request(
            b"k",
            &ok,
            &ConstitutionStatus::Intact,
            &ApprovalPolicy::none(),
            &AuthorityPolicy::disabled(),
            &TargetAllowlist::disabled(),
            &mut SingleFlight::new(),
            &mut LoopGuard::default(),
            &mut Governor::unlimited(),
            &policy,
            &mut retry,
            &qpolicy,
            &mut qledger,
            &mut RateLimiter::disabled(),
            0,
            &ScanPolicy::disabled(),
            &RiskPolicy::disabled(),
            &mut RiskLedger::new(),
            &DeadlinePolicy::disabled(),
            &NullSink,
            &raw,
        );
        assert!(!r3.accepted);
        assert!(r3.error.unwrap().contains("quarantined"));
        assert_eq!(
            r3.recovery.expect("quarantine carries recovery").action,
            RecoveryVerb::Escalate
        );
        assert_eq!(
            ok.calls(),
            0,
            "a quarantined fingerprint must never reach the kernel"
        );

        // A clean delegation of the same work (after an operator re-arm releases it) clears the
        // quarantine, so the gate is not permanent once the underlying failure is resolved.
        qledger.clear(&fp);
        let r4 = handle_request(
            b"k",
            &ok,
            &ConstitutionStatus::Intact,
            &ApprovalPolicy::none(),
            &AuthorityPolicy::disabled(),
            &TargetAllowlist::disabled(),
            &mut SingleFlight::new(),
            &mut LoopGuard::default(),
            &mut Governor::unlimited(),
            &policy,
            &mut retry,
            &qpolicy,
            &mut qledger,
            &mut RateLimiter::disabled(),
            0,
            &ScanPolicy::disabled(),
            &RiskPolicy::disabled(),
            &mut RiskLedger::new(),
            &DeadlinePolicy::disabled(),
            &NullSink,
            &raw,
        );
        assert!(r4.accepted);
        assert_eq!(ok.calls(), 1);
        assert!(!qledger.is_quarantined(&fp));
    }

    #[test]
    fn quarantine_disabled_by_default_never_refuses() {
        use runner_core::quarantine::{QuarantineLedger, QuarantinePolicy};
        // With the gate disabled (default), even many kernel failures never latch a refusal — the
        // behaviour-preserving guarantee.
        let qpolicy = QuarantinePolicy::disabled();
        let mut qledger = QuarantineLedger::new();
        let fail = FailingInvoker::default();
        let frame = sign_frame(b"k", &ci_spec(false)).unwrap();
        let raw = serde_json::to_vec(&frame).unwrap();
        for _ in 0..10 {
            let r = handle_request(
                b"k",
                &fail,
                &ConstitutionStatus::Intact,
                &ApprovalPolicy::none(),
                &AuthorityPolicy::disabled(),
                &TargetAllowlist::disabled(),
                &mut SingleFlight::new(),
                &mut LoopGuard::default(),
                &mut Governor::unlimited(),
                &RecoveryPolicy::new(5, 1),
                &mut RetryLedger::new(),
                &qpolicy,
                &mut qledger,
                &mut RateLimiter::disabled(),
                0,
                &ScanPolicy::disabled(),
                &RiskPolicy::disabled(),
                &mut RiskLedger::new(),
                &DeadlinePolicy::disabled(),
                &NullSink,
                &raw,
            );
            // Each is a kernel failure (not a quarantine refusal).
            assert!(r.error.unwrap().contains("kernel"));
        }
        assert_eq!(qledger.quarantined_count(), 0);
        assert_eq!(fail.calls(), 10, "every dispatch still reached the kernel");
    }

    #[test]
    fn deadline_watchdog_times_out_a_slow_delegation_and_passes_a_fast_one() {
        use std::time::Duration;
        let plan = router::route(&ci_spec(false));
        let job = ci_spec(false);

        // A fast invoker under a deadline → delivered (the watchdog does not interfere).
        let fast = RecordingInvoker::default();
        assert!(matches!(
            run_delegation(&fast, &plan, &job, Some(Duration::from_millis(500))),
            Delegation::Delivered(_)
        ));

        // A slow invoker over a short deadline → timed out (the hung-job bound fires).
        let slow = SlowInvoker {
            sleep: Duration::from_millis(150),
        };
        assert!(matches!(
            run_delegation(&slow, &plan, &job, Some(Duration::from_millis(20))),
            Delegation::TimedOut(_)
        ));

        // No deadline configured → the invoker is called directly; even a "slow" one delivers (the
        // default path is unchanged — the watchdog is not engaged).
        assert!(matches!(
            run_delegation(&slow, &plan, &job, None),
            Delegation::Delivered(_)
        ));
    }

    #[test]
    fn envelope_deadline_drives_a_timeout_through_recovery_and_the_audit_log() {
        use runner_core::events::DispatchEvent;
        use runner_core::recovery::RecoveryVerb;
        use std::cell::RefCell;
        struct Rec(RefCell<Vec<DispatchEvent>>);
        impl EventSink for Rec {
            fn emit(&self, e: &DispatchEvent) {
                self.0.borrow_mut().push(e.clone());
            }
        }
        let sink = Rec(RefCell::new(Vec::new()));

        // The kernel sleeps ~1.1s; the job requests a 1s deadline on its envelope (no operator cap).
        let slow = SlowInvoker {
            sleep: std::time::Duration::from_millis(1100),
        };
        let mut frame = sign_frame(b"k", &ci_spec(false)).unwrap();
        frame.deadline_secs = Some(1);

        let resp = handle_request(
            b"k",
            &slow,
            &ConstitutionStatus::Intact,
            &ApprovalPolicy::none(),
            &AuthorityPolicy::disabled(),
            &TargetAllowlist::disabled(),
            &mut SingleFlight::new(),
            &mut LoopGuard::default(),
            &mut Governor::unlimited(),
            &RecoveryPolicy::default(),
            &mut RetryLedger::new(),
            &QuarantinePolicy::disabled(),
            &mut QuarantineLedger::new(),
            &mut RateLimiter::disabled(),
            0,
            &ScanPolicy::disabled(),
            &RiskPolicy::disabled(),
            &mut RiskLedger::new(),
            &DeadlinePolicy::disabled(), // no operator cap — the per-job envelope deadline applies
            &sink,
            &serde_json::to_vec(&frame).unwrap(),
        );

        assert!(!resp.accepted, "a timed-out job is not accepted");
        assert!(resp.error.as_deref().unwrap().contains("deadline"));
        // First attempt of a transient timeout → recovery advises a backed-off retry.
        let d = resp
            .recovery
            .expect("deadline carries a recovery directive");
        assert_eq!(d.action, RecoveryVerb::Retry);
        // The audit log records the timeout as an Execution-category DeadlineExceeded event.
        let events = sink.0.into_inner();
        let timeout = events
            .iter()
            .find(|e| e.outcome == Outcome::DeadlineExceeded)
            .expect("a DeadlineExceeded event was audited");
        assert_eq!(timeout.category(), EventCategory::Execution);
        assert_eq!(timeout.kernel.as_deref(), Some("loop"));
    }

    #[test]
    fn loop_trip_escalates_and_a_clean_delegation_resets_the_retry_budget() {
        use runner_core::recovery::RecoveryVerb;
        // A clean delegation must clear the fingerprint so a later transient failure starts fresh.
        let mut retry = RetryLedger::new();
        let mut gov = Governor::unlimited();
        let policy = RecoveryPolicy::new(2, 1);

        // First, a failing invoker bumps the fingerprint's retry count to 1.
        let fail = FailingInvoker::default();
        let frame = sign_frame(b"k", &ci_spec(false)).unwrap();
        let raw = serde_json::to_vec(&frame).unwrap();
        handle_request(
            b"k",
            &fail,
            &ConstitutionStatus::Intact,
            &ApprovalPolicy::none(),
            &AuthorityPolicy::disabled(),
            &TargetAllowlist::disabled(),
            &mut SingleFlight::new(),
            &mut LoopGuard::default(),
            &mut gov,
            &policy,
            &mut retry,
            &QuarantinePolicy::disabled(),
            &mut QuarantineLedger::new(),
            &mut RateLimiter::disabled(),
            0,
            &ScanPolicy::disabled(),
            &RiskPolicy::disabled(),
            &mut RiskLedger::new(),
            &DeadlinePolicy::disabled(),
            &NullSink,
            &raw,
        );
        assert_eq!(
            retry.attempts(&runner_core::fingerprint(&ci_spec(false))),
            1
        );

        // Now the same work succeeds → the retry budget for that fingerprint is cleared.
        let ok = RecordingInvoker::default();
        let resp = handle_request(
            b"k",
            &ok,
            &ConstitutionStatus::Intact,
            &ApprovalPolicy::none(),
            &AuthorityPolicy::disabled(),
            &TargetAllowlist::disabled(),
            &mut SingleFlight::new(),
            &mut LoopGuard::default(),
            &mut gov,
            &policy,
            &mut retry,
            &QuarantinePolicy::disabled(),
            &mut QuarantineLedger::new(),
            &mut RateLimiter::disabled(),
            0,
            &ScanPolicy::disabled(),
            &RiskPolicy::disabled(),
            &mut RiskLedger::new(),
            &DeadlinePolicy::disabled(),
            &NullSink,
            &raw,
        );
        assert!(resp.accepted);
        assert_eq!(
            retry.attempts(&runner_core::fingerprint(&ci_spec(false))),
            0,
            "a clean delegation clears the fingerprint's retry count"
        );

        // And a loop trip escalates immediately (independent of the retry ledger).
        let mut tight = LoopGuard::new(2, 1); // trips on the 1st observation
        let tripped = handle_request(
            b"k",
            &ok,
            &ConstitutionStatus::Intact,
            &ApprovalPolicy::none(),
            &AuthorityPolicy::disabled(),
            &TargetAllowlist::disabled(),
            &mut SingleFlight::new(),
            &mut tight,
            &mut gov,
            &policy,
            &mut retry,
            &QuarantinePolicy::disabled(),
            &mut QuarantineLedger::new(),
            &mut RateLimiter::disabled(),
            0,
            &ScanPolicy::disabled(),
            &RiskPolicy::disabled(),
            &mut RiskLedger::new(),
            &DeadlinePolicy::disabled(),
            &NullSink,
            &raw,
        );
        assert!(!tripped.accepted);
        assert_eq!(
            tripped.recovery.expect("loop trip carries recovery").action,
            RecoveryVerb::Escalate
        );
    }

    #[test]
    fn approval_band_holds_a_job_without_a_grant_then_admits_with_one() {
        use runner_core::recovery::RecoveryVerb;
        use runner_core::wire::Approval;
        let inv = RecordingInvoker::default();
        // CI jobs require approval in this policy.
        let approval = ApprovalPolicy::from_bands("ci");
        let mut guard = LoopGuard::default();
        let mut gov = Governor::unlimited();
        let policy = RecoveryPolicy::default();
        let mut retry = RetryLedger::new();

        let spec = ci_spec(false);
        let frame = sign_frame(b"k", &spec).unwrap();

        // No grant → held (ApprovalRequired), never reaches the kernel, advises escalation.
        let held = handle_request(
            b"k",
            &inv,
            &ConstitutionStatus::Intact,
            &approval,
            &AuthorityPolicy::disabled(),
            &TargetAllowlist::disabled(),
            &mut SingleFlight::new(),
            &mut guard,
            &mut gov,
            &policy,
            &mut retry,
            &QuarantinePolicy::disabled(),
            &mut QuarantineLedger::new(),
            &mut RateLimiter::disabled(),
            0,
            &ScanPolicy::disabled(),
            &RiskPolicy::disabled(),
            &mut RiskLedger::new(),
            &DeadlinePolicy::disabled(),
            &NullSink,
            &serde_json::to_vec(&frame).unwrap(),
        );
        assert!(!held.accepted);
        assert!(held.error.unwrap().contains("requires human approval"));
        assert_eq!(
            held.recovery.expect("held job carries recovery").action,
            RecoveryVerb::Escalate
        );
        assert_eq!(inv.calls(), 0, "a held job must never reach a kernel");

        // Re-dispatch WITH a valid grant bound to the fingerprint → admitted and delegated.
        let mut approved_frame = sign_frame(b"k", &spec).unwrap();
        approved_frame.approval = Some(Approval::grant(
            b"k",
            &runner_core::fingerprint(&spec),
            "alice",
        ));
        let ok = handle_request(
            b"k",
            &inv,
            &ConstitutionStatus::Intact,
            &approval,
            &AuthorityPolicy::disabled(),
            &TargetAllowlist::disabled(),
            &mut SingleFlight::new(),
            &mut guard,
            &mut gov,
            &policy,
            &mut retry,
            &QuarantinePolicy::disabled(),
            &mut QuarantineLedger::new(),
            &mut RateLimiter::disabled(),
            0,
            &ScanPolicy::disabled(),
            &RiskPolicy::disabled(),
            &mut RiskLedger::new(),
            &DeadlinePolicy::disabled(),
            &NullSink,
            &serde_json::to_vec(&approved_frame).unwrap(),
        );
        assert!(ok.accepted);
        assert_eq!(inv.calls(), 1);

        // A grant for a DIFFERENT job's fingerprint does not unlock this one.
        let mut forged = sign_frame(b"k", &spec).unwrap();
        forged.approval = Some(Approval::grant(b"k", "some-other-fingerprint", "alice"));
        let rejected = handle_request(
            b"k",
            &inv,
            &ConstitutionStatus::Intact,
            &approval,
            &AuthorityPolicy::disabled(),
            &TargetAllowlist::disabled(),
            &mut SingleFlight::new(),
            &mut LoopGuard::default(), // fresh guard so the breaker doesn't trip on the repeat
            &mut gov,
            &policy,
            &mut retry,
            &QuarantinePolicy::disabled(),
            &mut QuarantineLedger::new(),
            &mut RateLimiter::disabled(),
            0,
            &ScanPolicy::disabled(),
            &RiskPolicy::disabled(),
            &mut RiskLedger::new(),
            &DeadlinePolicy::disabled(),
            &NullSink,
            &serde_json::to_vec(&forged).unwrap(),
        );
        assert!(
            !rejected.accepted,
            "a mismatched grant must not unlock the job"
        );
        assert_eq!(inv.calls(), 1, "still only the one approved delegation");
    }

    #[cfg(unix)]
    #[test]
    fn workspace_is_torn_down_even_when_the_kernel_fails() {
        use runner_core::workspace::{JobWorkspace, WorkspaceProvider};
        use std::sync::Mutex;

        // A provider that creates a real temp dir and records the path it handed out, so the test
        // can assert the directory is gone after the (failing) invocation returns. `Mutex` (not
        // `RefCell`) so the invoker is `Sync` — `handle_request` now needs `KernelInvoker + Sync`
        // for the deadline watchdog's scoped worker thread.
        struct RecordingProvider {
            created: Mutex<Vec<std::path::PathBuf>>,
        }
        impl WorkspaceProvider for RecordingProvider {
            fn acquire(&self, label: &str) -> Result<JobWorkspace, String> {
                let root = std::env::temp_dir()
                    .join(format!("fxrun-wstest-{}-{label}", std::process::id()));
                std::fs::create_dir_all(&root).unwrap();
                self.created.lock().unwrap().push(root.clone());
                let cleanup_root = root.clone();
                Ok(JobWorkspace::new(root, move || {
                    std::fs::remove_dir_all(&cleanup_root).map_err(|e| e.to_string())
                }))
            }
        }

        // An invoker that acquires a workspace and THEN fails — the fail path must still tear down.
        struct FailAfterWorkspace {
            provider: RecordingProvider,
        }
        impl KernelInvoker for FailAfterWorkspace {
            fn invoke(
                &self,
                _plan: &KernelPlan,
                job: &JobSpec,
                _deadline: Option<std::time::Duration>,
            ) -> Delegation {
                let ws = match self.provider.acquire(&job.id) {
                    Ok(ws) => ws,
                    Err(e) => return Delegation::Failed(e),
                };
                assert!(ws.root().exists(), "workspace exists during the job");
                Delegation::Failed("kernel blew up mid-job".into())
                // `ws` drops on this early return → cleanup runs (Archon zero-residue).
            }
        }

        let inv = FailAfterWorkspace {
            provider: RecordingProvider {
                created: Mutex::new(Vec::new()),
            },
        };
        let frame = sign_frame(b"k", &ci_spec(false)).unwrap();
        let resp = handle_request(
            b"k",
            &inv,
            &ConstitutionStatus::Intact,
            &ApprovalPolicy::none(),
            &AuthorityPolicy::disabled(),
            &TargetAllowlist::disabled(),
            &mut SingleFlight::new(),
            &mut LoopGuard::default(),
            &mut Governor::unlimited(),
            &RecoveryPolicy::default(),
            &mut RetryLedger::new(),
            &QuarantinePolicy::disabled(),
            &mut QuarantineLedger::new(),
            &mut RateLimiter::disabled(),
            0,
            &ScanPolicy::disabled(),
            &RiskPolicy::disabled(),
            &mut RiskLedger::new(),
            &DeadlinePolicy::disabled(),
            &NullSink,
            &serde_json::to_vec(&frame).unwrap(),
        );
        assert!(!resp.accepted, "the kernel failed");
        let created = inv.provider.created.into_inner().unwrap();
        assert_eq!(created.len(), 1);
        assert!(
            !created[0].exists(),
            "the workspace must be torn down on the kernel-failure path (zero residue)"
        );
    }

    #[cfg(unix)]
    #[test]
    fn routing_sink_tees_only_policy_events_to_the_policy_stream() {
        let dir = std::env::temp_dir();
        let n = format!("{}-{}", std::process::id(), line!());
        let all_path = dir.join(format!("fxrun-all-{n}.ndjson"));
        let policy_path = dir.join(format!("fxrun-policy-{n}.ndjson"));
        let _ = std::fs::remove_file(&all_path);
        let _ = std::fs::remove_file(&policy_path);

        let sink = RoutingSink {
            all: Some(FileSink {
                path: all_path.clone(),
            }),
            policy: Some(FileSink {
                path: policy_path.clone(),
            }),
        };
        // One execution event and one policy event.
        sink.emit(&DispatchEvent::for_job(Outcome::Delegated, &ci_spec(false)).with_kernel("loop"));
        sink.emit(
            &DispatchEvent::for_job(Outcome::ForkRejected, &ci_spec(true)).with_detail("fork"),
        );

        let all = std::fs::read_to_string(&all_path).unwrap();
        let policy = std::fs::read_to_string(&policy_path).unwrap();
        // The full log has both; the policy stream has only the policy event.
        assert_eq!(all.lines().count(), 2);
        assert_eq!(policy.lines().count(), 1);
        assert!(policy.contains("fork_rejected"));
        assert!(
            !policy.contains("delegated"),
            "execution events stay out of the policy stream"
        );

        let _ = std::fs::remove_file(&all_path);
        let _ = std::fs::remove_file(&policy_path);
    }

    #[cfg(unix)]
    #[test]
    fn real_uds_roundtrip() {
        use std::io::{Read, Write};
        use std::net::Shutdown;
        use std::os::unix::net::{UnixListener, UnixStream};
        use std::sync::Arc;

        fn unique_sock() -> std::path::PathBuf {
            static N: AtomicUsize = AtomicUsize::new(0);
            let n = N.fetch_add(1, Ordering::Relaxed);
            std::env::temp_dir().join(format!("fxrun-dispatch-{}-{n}.sock", std::process::id()))
        }

        let key = b"dispatch-key".to_vec();
        let sock = unique_sock();
        let listener = UnixListener::bind(&sock).unwrap();

        let recorder = Arc::new(RecordingInvoker::default());
        let rec_srv = recorder.clone();
        let key_srv = key.clone();
        let handle = std::thread::spawn(move || {
            serve_once(
                &listener,
                &key_srv,
                &*rec_srv,
                &Constitution::default(),
                &ApprovalPolicy::none(),
                &AuthorityPolicy::disabled(),
                &TargetAllowlist::disabled(),
                &mut SingleFlight::new(),
                &mut LoopGuard::default(),
                &mut Governor::unlimited(),
                &RecoveryPolicy::default(),
                &mut RetryLedger::new(),
                &QuarantinePolicy::disabled(),
                &mut QuarantineLedger::new(),
                &mut RateLimiter::disabled(),
                0,
                &ScanPolicy::disabled(),
                &RiskPolicy::disabled(),
                &mut RiskLedger::new(),
                &DeadlinePolicy::disabled(),
                &Redactor::new(),
                &NullSink,
            )
            .unwrap();
        });

        let frame = sign_frame(&key, &ci_spec(false)).unwrap();
        let mut stream = UnixStream::connect(&sock).unwrap();
        stream
            .write_all(&serde_json::to_vec(&frame).unwrap())
            .unwrap();
        stream.shutdown(Shutdown::Write).unwrap();
        let mut resp_bytes = Vec::new();
        stream.read_to_end(&mut resp_bytes).unwrap();
        handle.join().unwrap();

        let resp: DispatchResponse = serde_json::from_slice(&resp_bytes).unwrap();
        assert!(resp.accepted);
        assert_eq!(resp.kernel.as_deref(), Some("loop"));
        assert_eq!(recorder.calls(), 1);
        let _ = std::fs::remove_file(&sock);
    }

    /// An invoker whose error string embeds a secret — exercises the redaction seam on the real
    /// kernel-failure path (the error becomes a `detail` + the response `error`).
    struct LeakyInvoker {
        secret: String,
    }
    impl KernelInvoker for LeakyInvoker {
        fn invoke(
            &self,
            _plan: &KernelPlan,
            _job: &JobSpec,
            _deadline: Option<std::time::Duration>,
        ) -> Delegation {
            Delegation::Failed(format!("upstream auth failed using token {}", self.secret))
        }
    }

    /// End-to-end over a real Unix socket AND a real on-disk audit file: a kernel failure whose error
    /// embeds a registered secret must reach NEITHER the wire reply NOR the audit log un-redacted.
    #[cfg(unix)]
    #[test]
    fn secret_is_redacted_from_both_the_reply_and_the_audit_log() {
        use std::io::{Read, Write};
        use std::net::Shutdown;
        use std::os::unix::net::{UnixListener, UnixStream};

        fn unique(stem: &str) -> std::path::PathBuf {
            static N: AtomicUsize = AtomicUsize::new(0);
            let n = N.fetch_add(1, Ordering::Relaxed);
            std::env::temp_dir().join(format!("fxrun-redact-{}-{stem}-{n}", std::process::id()))
        }

        const SECRET: &str = "s3cr3t-bearer-do-not-log";
        let key = b"dispatch-key".to_vec();
        let sock = unique("sock");
        let log_path = unique("audit.ndjson");
        let listener = UnixListener::bind(&sock).unwrap();

        // The dispatch key is also a secret — confirm it never leaks either (it isn't in the message
        // here, but the redactor registers it regardless).
        let redactor = build_redactor("dispatch-key").with_secret(SECRET);
        assert!(redactor.is_active());

        let log_srv = log_path.clone();
        let key_srv = key.clone();
        let handle = std::thread::spawn(move || {
            // The real FileSink (writes the NDJSON file), wrapped in the real RedactingSink.
            let file_sink: Box<dyn EventSink> = Box::new(RoutingSink {
                all: Some(FileSink { path: log_srv }),
                policy: None,
            });
            let sink = RedactingSink::new(file_sink, redactor.clone());
            serve_once(
                &listener,
                &key_srv,
                &LeakyInvoker {
                    secret: SECRET.to_string(),
                },
                &Constitution::default(),
                &ApprovalPolicy::none(),
                &AuthorityPolicy::disabled(),
                &TargetAllowlist::disabled(),
                &mut SingleFlight::new(),
                &mut LoopGuard::default(),
                &mut Governor::unlimited(),
                &RecoveryPolicy::new(0, 5), // 0 retries → escalate (still a rejection carrying detail)
                &mut RetryLedger::new(),
                &QuarantinePolicy::disabled(),
                &mut QuarantineLedger::new(),
                &mut RateLimiter::disabled(),
                0,
                &ScanPolicy::disabled(),
                &RiskPolicy::disabled(),
                &mut RiskLedger::new(),
                &DeadlinePolicy::disabled(),
                &redactor,
                &sink,
            )
            .unwrap();
        });

        let frame = sign_frame(&key, &ci_spec(false)).unwrap();
        let mut stream = UnixStream::connect(&sock).unwrap();
        stream
            .write_all(&serde_json::to_vec(&frame).unwrap())
            .unwrap();
        stream.shutdown(Shutdown::Write).unwrap();
        let mut resp_bytes = Vec::new();
        stream.read_to_end(&mut resp_bytes).unwrap();
        handle.join().unwrap();

        // (1) The wire reply: rejected, error present, secret scrubbed, placeholder shown.
        let resp: DispatchResponse = serde_json::from_slice(&resp_bytes).unwrap();
        assert!(!resp.accepted);
        let err = resp.error.expect("a rejection carries an error");
        assert!(
            !err.contains(SECRET),
            "secret leaked into the wire reply: {err}"
        );
        assert!(
            err.contains(Redactor::PLACEHOLDER),
            "reply should show the placeholder: {err}"
        );

        // (2) The on-disk audit log: the secret must not appear anywhere in the file.
        let logged = std::fs::read_to_string(&log_path).unwrap();
        assert!(
            !logged.contains(SECRET),
            "secret leaked into the audit log:\n{logged}"
        );
        assert!(
            logged.contains(Redactor::PLACEHOLDER),
            "audit log should show the placeholder:\n{logged}"
        );
        assert!(logged.contains("kernel_failed"), "the failure was audited");

        let _ = std::fs::remove_file(&sock);
        let _ = std::fs::remove_file(&log_path);
    }

    #[test]
    fn rate_limit_denies_a_burst_past_the_window_cap_without_consuming_retries() {
        let inv = RecordingInvoker::default();
        let mut guard = LoopGuard::default();
        let mut gov = Governor::unlimited();
        let mut retry = RetryLedger::new();
        let mut q = QuarantineLedger::new();
        // 2 dispatches per 60s window; all calls share now=0 so they fall in one window.
        let mut rl = RateLimiter::new(RateLimitPolicy::new(2, 60, 0));
        let raw = serde_json::to_vec(&sign_frame(b"k", &ci_spec(false)).unwrap()).unwrap();

        // The first two within the window admit and delegate (2 reach the kernel — under the 4/8
        // breaker threshold, so the breaker does not trip).
        for _ in 0..2 {
            let r = handle_request(
                b"k",
                &inv,
                &ConstitutionStatus::Intact,
                &ApprovalPolicy::none(),
                &AuthorityPolicy::disabled(),
                &TargetAllowlist::disabled(),
                &mut SingleFlight::new(),
                &mut guard,
                &mut gov,
                &RecoveryPolicy::default(),
                &mut retry,
                &QuarantinePolicy::disabled(),
                &mut q,
                &mut rl,
                0,
                &ScanPolicy::disabled(),
                &RiskPolicy::disabled(),
                &mut RiskLedger::new(),
                &DeadlinePolicy::disabled(),
                &NullSink,
                &raw,
            );
            assert!(r.accepted);
        }
        // The third dispatch in the same window is rate-limited (and never reaches the kernel).
        let denied = handle_request(
            b"k",
            &inv,
            &ConstitutionStatus::Intact,
            &ApprovalPolicy::none(),
            &AuthorityPolicy::disabled(),
            &TargetAllowlist::disabled(),
            &mut SingleFlight::new(),
            &mut guard,
            &mut gov,
            &RecoveryPolicy::default(),
            &mut retry,
            &QuarantinePolicy::disabled(),
            &mut q,
            &mut rl,
            0,
            &ScanPolicy::disabled(),
            &RiskPolicy::disabled(),
            &mut RiskLedger::new(),
            &DeadlinePolicy::disabled(),
            &NullSink,
            &raw,
        );
        assert!(!denied.accepted);
        assert!(denied.error.unwrap().contains("rate limit"));
        let rec = denied
            .recovery
            .expect("a rate-limit refusal carries a retry-after directive");
        assert_eq!(rec.action, RecoveryVerb::Retry);
        assert_eq!(
            rec.attempt, 0,
            "back-pressure must not consume the retry budget"
        );
        assert!(rec.backoff_secs > 0, "retry-after hint is set");
        assert_eq!(
            inv.calls(),
            2,
            "the rate-limited job never reached the kernel"
        );
        assert_eq!(
            retry.tracked(),
            0,
            "no fingerprint entered the retry ledger"
        );
    }

    #[test]
    fn route_failure_cooldown_holds_the_route_after_a_kernel_failure() {
        let inv = FailingInvoker::default();
        let mut guard = LoopGuard::default();
        let mut gov = Governor::unlimited();
        let mut retry = RetryLedger::new();
        let mut q = QuarantineLedger::new();
        // No window cap; a 30s per-route cooldown after a failure.
        let mut rl = RateLimiter::new(RateLimitPolicy::new(0, 1, 30));
        let raw = serde_json::to_vec(&sign_frame(b"k", &ci_spec(false)).unwrap()).unwrap();

        // First dispatch reaches the kernel, fails, and starts the "ci" route cooldown.
        let first = handle_request(
            b"k",
            &inv,
            &ConstitutionStatus::Intact,
            &ApprovalPolicy::none(),
            &AuthorityPolicy::disabled(),
            &TargetAllowlist::disabled(),
            &mut SingleFlight::new(),
            &mut guard,
            &mut gov,
            &RecoveryPolicy::default(),
            &mut retry,
            &QuarantinePolicy::disabled(),
            &mut q,
            &mut rl,
            0,
            &ScanPolicy::disabled(),
            &RiskPolicy::disabled(),
            &mut RiskLedger::new(),
            &DeadlinePolicy::disabled(),
            &NullSink,
            &raw,
        );
        assert!(!first.accepted);
        assert_eq!(inv.calls(), 1);

        // A second "ci" dispatch 5s later is held by the cooldown — it never reaches the kernel again.
        let cooled = handle_request(
            b"k",
            &inv,
            &ConstitutionStatus::Intact,
            &ApprovalPolicy::none(),
            &AuthorityPolicy::disabled(),
            &TargetAllowlist::disabled(),
            &mut SingleFlight::new(),
            &mut guard,
            &mut gov,
            &RecoveryPolicy::default(),
            &mut retry,
            &QuarantinePolicy::disabled(),
            &mut q,
            &mut rl,
            5,
            &ScanPolicy::disabled(),
            &RiskPolicy::disabled(),
            &mut RiskLedger::new(),
            &DeadlinePolicy::disabled(),
            &NullSink,
            &raw,
        );
        assert!(!cooled.accepted);
        assert!(cooled.error.unwrap().contains("cooldown"));
        assert_eq!(
            inv.calls(),
            1,
            "the cooling route must not reach the kernel again"
        );
    }

    #[test]
    fn content_scan_blocks_an_injection_job_and_escalates_without_reaching_the_kernel() {
        use runner_core::scan::Severity;
        let inv = RecordingInvoker::default();
        // A LoopCycle whose task_id smuggles a command substitution — structurally valid (non-blank,
        // good repo), so only the content scan catches it.
        let spec = JobSpec {
            id: "job-x".into(),
            correlation_id: "corr-x".into(),
            from_fork: false,
            job: runner_core::jobspec::JobKind::LoopCycle {
                repo: "FlexNetOS/meta".into(),
                task_id: "T-1; rm -rf $(echo ~)".into(),
            },
        };
        let raw = serde_json::to_vec(&sign_frame(b"k", &spec).unwrap()).unwrap();

        // Gate OFF (default): the job passes content (and delegates).
        let off = handle_request(
            b"k",
            &inv,
            &ConstitutionStatus::Intact,
            &ApprovalPolicy::none(),
            &AuthorityPolicy::disabled(),
            &TargetAllowlist::disabled(),
            &mut SingleFlight::new(),
            &mut LoopGuard::default(),
            &mut Governor::unlimited(),
            &RecoveryPolicy::default(),
            &mut RetryLedger::new(),
            &QuarantinePolicy::disabled(),
            &mut QuarantineLedger::new(),
            &mut RateLimiter::disabled(),
            0,
            &ScanPolicy::disabled(),
            &RiskPolicy::disabled(),
            &mut RiskLedger::new(),
            &DeadlinePolicy::disabled(),
            &NullSink,
            &raw,
        );
        assert!(off.accepted, "scan off → not refused on content");
        assert_eq!(inv.calls(), 1);

        // Gate ON at `high`: the same job is refused before the kernel, and escalates (not retry).
        let blocked = handle_request(
            b"k",
            &inv,
            &ConstitutionStatus::Intact,
            &ApprovalPolicy::none(),
            &AuthorityPolicy::disabled(),
            &TargetAllowlist::disabled(),
            &mut SingleFlight::new(),
            &mut LoopGuard::default(),
            &mut Governor::unlimited(),
            &RecoveryPolicy::default(),
            &mut RetryLedger::new(),
            &QuarantinePolicy::disabled(),
            &mut QuarantineLedger::new(),
            &mut RateLimiter::disabled(),
            0,
            &ScanPolicy::block_at(Severity::High),
            &RiskPolicy::disabled(),
            &mut RiskLedger::new(),
            &DeadlinePolicy::disabled(),
            &NullSink,
            &raw,
        );
        assert!(!blocked.accepted);
        let err = blocked.error.unwrap();
        assert!(err.contains("content scan blocked"));
        assert!(err.contains("job.task_id"));
        assert_eq!(
            blocked.recovery.unwrap().action,
            RecoveryVerb::Escalate,
            "hostile content escalates to a human, never retries"
        );
        assert_eq!(
            inv.calls(),
            1,
            "the blocked job never reached the kernel (still 1 from the gate-off case)"
        );
    }

    #[test]
    fn risk_score_climbs_with_failure_history_and_rides_the_audit_event() {
        use runner_core::events::DispatchEvent;
        use runner_core::risk::{RiskBand, RiskModel, RiskPolicy};
        use std::cell::RefCell;

        struct Recorder(RefCell<Vec<DispatchEvent>>);
        impl EventSink for Recorder {
            fn emit(&self, e: &DispatchEvent) {
                self.0.borrow_mut().push(e.clone());
            }
        }

        let inv = FailingInvoker::default();
        let sink = Recorder(RefCell::new(Vec::new()));
        let risk_policy = RiskPolicy::enabled(RiskModel::standard());
        let mut risk_ledger = RiskLedger::new();
        let raw = serde_json::to_vec(&sign_frame(b"k", &ci_spec(false)).unwrap()).unwrap();

        // Drive the same (always-failing) job several times; each failure feeds the risk ledger.
        for _ in 0..6 {
            handle_request(
                b"k",
                &inv,
                &ConstitutionStatus::Intact,
                &ApprovalPolicy::none(),
                &AuthorityPolicy::disabled(),
                &TargetAllowlist::disabled(),
                &mut SingleFlight::new(),
                &mut LoopGuard::new(100, 100), // keep the breaker out of the way for this run
                &mut Governor::unlimited(),
                &RecoveryPolicy::new(99, 1), // never escalate within this run (stay on the retry path)
                &mut RetryLedger::new(),
                &QuarantinePolicy::disabled(),
                &mut QuarantineLedger::new(),
                &mut RateLimiter::disabled(),
                0,
                &ScanPolicy::disabled(),
                &risk_policy,
                &mut risk_ledger,
                &DeadlinePolicy::disabled(),
                &sink,
                &raw,
            );
        }

        let events = sink.0.into_inner();
        // The FIRST event sees no history → score at the base rate (low band).
        let first_risk = events[0].risk.expect("risk annotation is on");
        assert_eq!(first_risk.band, RiskBand::Low);
        assert_eq!(first_risk.samples, 0);
        // By the LAST event the accumulated failures have driven the score into the high band.
        let last_risk = events.last().unwrap().risk.expect("risk annotation is on");
        assert_eq!(last_risk.band, RiskBand::High);
        assert!(last_risk.score > first_risk.score);
        assert_eq!(
            last_risk.samples, 5,
            "5 prior failures informed the 6th assessment"
        );
        // The detail line carries the human-readable risk summary too.
        assert!(events
            .last()
            .unwrap()
            .detail
            .as_ref()
            .unwrap()
            .contains("risk: high"));
    }

    #[test]
    fn fatal_kernel_error_escalates_immediately_instead_of_retrying() {
        use runner_core::events::DispatchEvent;
        use std::cell::RefCell;

        struct Recorder(RefCell<Vec<DispatchEvent>>);
        impl EventSink for Recorder {
            fn emit(&self, e: &DispatchEvent) {
                self.0.borrow_mut().push(e.clone());
            }
        }

        let inv = FatalInvoker::default();
        let sink = Recorder(RefCell::new(Vec::new()));
        let mut retry = RetryLedger::new();
        // A generous retry budget — a TRANSIENT error would retry; a FATAL one must NOT.
        let resp = handle_request(
            b"k",
            &inv,
            &ConstitutionStatus::Intact,
            &ApprovalPolicy::none(),
            &AuthorityPolicy::disabled(),
            &TargetAllowlist::disabled(),
            &mut SingleFlight::new(),
            &mut LoopGuard::default(),
            &mut Governor::unlimited(),
            &RecoveryPolicy::new(5, 1),
            &mut retry,
            &QuarantinePolicy::disabled(),
            &mut QuarantineLedger::new(),
            &mut RateLimiter::disabled(),
            0,
            &ScanPolicy::disabled(),
            &RiskPolicy::disabled(),
            &mut RiskLedger::new(),
            &DeadlinePolicy::disabled(),
            &sink,
            &serde_json::to_vec(&sign_frame(b"k", &ci_spec(false)).unwrap()).unwrap(),
        );
        assert!(!resp.accepted);
        let rec = resp
            .recovery
            .expect("a failure carries a recovery directive");
        assert_eq!(
            rec.action,
            RecoveryVerb::Escalate,
            "a fatal (auth) error escalates immediately, never retries"
        );
        assert_eq!(rec.attempt, 0, "fatal failure consumes no retry budget");
        assert_eq!(
            retry.tracked(),
            0,
            "no fingerprint entered the retry ledger"
        );
        assert_eq!(inv.calls(), 1);
        // The audit event is the fixed-enum KernelFatal class (the no-text telemetry seam).
        let event = sink.0.into_inner().pop().unwrap();
        assert_eq!(event.outcome, Outcome::KernelFatal);
    }

    // ---- P3: SubprocessInvoker (real kernel spawn) unit tests, driven by a stub kernel script ----
    #[cfg(unix)]
    mod subprocess {
        use super::*;
        use std::path::{Path, PathBuf};
        use std::time::Duration;

        /// A unique scratch dir OUTSIDE any job workspace, for the stub kernel to write assertions to
        /// (the job workspace is torn down when `invoke` returns, so markers must live elsewhere).
        fn scratch(stem: &str) -> PathBuf {
            static N: AtomicUsize = AtomicUsize::new(0);
            let n = N.fetch_add(1, Ordering::Relaxed);
            let d =
                std::env::temp_dir().join(format!("fxrun-p3-{}-{stem}-{n}", std::process::id()));
            std::fs::create_dir_all(&d).unwrap();
            d
        }

        /// Write an executable `/bin/sh` stub kernel with `body`, return its path.
        fn stub(dir: &Path, body: &str) -> PathBuf {
            use std::os::unix::fs::PermissionsExt;
            let path = dir.join("stub-kernel.sh");
            std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            path
        }

        fn commands_using(stub: &Path) -> KernelCommands {
            let s = stub.display().to_string();
            KernelCommands {
                loop_lib: s.clone(),
                atc: s.clone(),
                hf: s.clone(),
                weave: s,
            }
        }

        /// A CI job with a process-unique id, so each test gets its own workspace dir (TempDirProvider
        /// keys on pid + job id; a shared id would collide across the parallel test threads).
        fn ci() -> JobSpec {
            static N: AtomicUsize = AtomicUsize::new(0);
            let n = N.fetch_add(1, Ordering::Relaxed);
            JobSpec {
                id: format!("p3-job-{}-{n}", std::process::id()),
                correlation_id: "p3-corr".into(),
                from_fork: false,
                job: JobKind::Ci {
                    repo: "FlexNetOS/meta".into(),
                    head_sha: "deadbeef".into(),
                },
            }
        }

        #[test]
        fn spawns_the_kernel_and_relays_its_cost_report() {
            let dir = scratch("ok");
            // The kernel writes its cost report to FXRUN_COST_FILE and exits 0.
            let k = stub(
                &dir,
                r#"echo '{"tokens":1500,"usd_micros":7500}' > "$FXRUN_COST_FILE""#,
            );
            let inv = SubprocessInvoker {
                workspace: TempDirProvider,
                commands: commands_using(&k),
                secrets: vec![],
            };
            let job = ci();
            match inv.invoke(&router::route(&job), &job, None) {
                Delegation::Delivered(cost) => assert_eq!(cost, JobCost::new(1500, 7500)),
                other => panic!("expected Delivered, got {:?}", DelegDbg(&other)),
            }
            let _ = std::fs::remove_dir_all(&dir);
        }

        #[test]
        fn a_nonzero_exit_is_a_failure_carrying_the_stderr_tail() {
            let dir = scratch("fail");
            let k = stub(&dir, "echo 'boom: the kernel broke' >&2\nexit 3");
            let inv = SubprocessInvoker {
                workspace: TempDirProvider,
                commands: commands_using(&k),
                secrets: vec![],
            };
            let job = ci();
            match inv.invoke(&router::route(&job), &job, None) {
                Delegation::Failed(msg) => {
                    assert!(msg.contains("exited 3"), "msg: {msg}");
                    assert!(
                        msg.contains("boom: the kernel broke"),
                        "stderr tail captured: {msg}"
                    );
                }
                other => panic!("expected Failed, got {:?}", DelegDbg(&other)),
            }
            let _ = std::fs::remove_dir_all(&dir);
        }

        #[test]
        fn a_hung_kernel_is_killed_at_the_deadline() {
            let dir = scratch("hang");
            let k = stub(&dir, "sleep 30"); // would run far past the deadline
            let inv = SubprocessInvoker {
                workspace: TempDirProvider,
                commands: commands_using(&k),
                secrets: vec![],
            };
            let job = ci();
            let start = std::time::Instant::now();
            let outcome = inv.invoke(&router::route(&job), &job, Some(Duration::from_millis(300)));
            let elapsed = start.elapsed();
            match outcome {
                Delegation::TimedOut(limit) => assert_eq!(limit, Duration::from_millis(300)),
                other => panic!("expected TimedOut, got {:?}", DelegDbg(&other)),
            }
            // The child was actually killed — we returned promptly, not after the 30s sleep.
            assert!(
                elapsed < Duration::from_secs(3),
                "should kill at the deadline, took {elapsed:?}"
            );
            let _ = std::fs::remove_dir_all(&dir);
        }

        #[test]
        fn hands_the_kernel_its_jobspec_on_stdin_and_runs_in_the_workspace() {
            let dir = scratch("handoff");
            let marker = dir.join("seen.txt");
            let stdin_copy = dir.join("stdin.json");
            // Record the handoff env + cwd to an external marker, and capture stdin.
            let k = stub(
                &dir,
                &format!(
                    "echo \"$FXRUN_JOB_ID|$FXRUN_KERNEL|$FXRUN_REPO|$FXRUN_INTENT|$(pwd)\" > {m}\n\
                     cat > {s}",
                    m = marker.display(),
                    s = stdin_copy.display(),
                ),
            );
            let inv = SubprocessInvoker {
                workspace: TempDirProvider,
                commands: commands_using(&k),
                secrets: vec![],
            };
            let job = ci();
            assert!(matches!(
                inv.invoke(&router::route(&job), &job, None),
                Delegation::Delivered(_)
            ));
            let seen = std::fs::read_to_string(&marker).unwrap();
            assert!(seen.contains("p3-job"), "FXRUN_JOB_ID handed off: {seen}");
            assert!(seen.contains("loop"), "FXRUN_KERNEL handed off: {seen}");
            assert!(
                seen.contains("FlexNetOS/meta"),
                "FXRUN_REPO handed off: {seen}"
            );
            assert!(
                seen.contains("fxrun-ws-"),
                "cwd is the isolated workspace: {seen}"
            );
            let spec_seen = std::fs::read_to_string(&stdin_copy).unwrap();
            let parsed: JobSpec = serde_json::from_str(spec_seen.trim()).unwrap();
            assert_eq!(parsed.id, job.id, "the JobSpec arrived on stdin");
            let _ = std::fs::remove_dir_all(&dir);
        }

        #[test]
        fn injects_secrets_into_the_kernel_environment() {
            let dir = scratch("secret");
            let marker = dir.join("secret-seen.txt");
            let k = stub(
                &dir,
                &format!("printf '%s' \"$KERNEL_TOKEN\" > {}", marker.display()),
            );
            let inv = SubprocessInvoker {
                workspace: TempDirProvider,
                commands: commands_using(&k),
                secrets: vec![("KERNEL_TOKEN".into(), "inject3d-s3cret".into())],
            };
            let job = ci();
            assert!(matches!(
                inv.invoke(&router::route(&job), &job, None),
                Delegation::Delivered(_)
            ));
            assert_eq!(std::fs::read_to_string(&marker).unwrap(), "inject3d-s3cret");
            let _ = std::fs::remove_dir_all(&dir);
        }

        #[test]
        fn a_missing_cost_report_is_fail_open_zero() {
            let dir = scratch("nocost");
            let k = stub(&dir, "exit 0"); // succeeds but writes no cost file
            let inv = SubprocessInvoker {
                workspace: TempDirProvider,
                commands: commands_using(&k),
                secrets: vec![],
            };
            let job = ci();
            match inv.invoke(&router::route(&job), &job, None) {
                Delegation::Delivered(cost) => assert_eq!(cost, JobCost::ZERO),
                other => panic!("expected Delivered ZERO, got {:?}", DelegDbg(&other)),
            }
            let _ = std::fs::remove_dir_all(&dir);
        }

        #[test]
        fn a_missing_kernel_binary_is_a_clean_failure_not_a_panic() {
            let inv = SubprocessInvoker {
                workspace: TempDirProvider,
                commands: commands_using(Path::new("/nonexistent/fxrun-no-such-kernel")),
                secrets: vec![],
            };
            let job = ci();
            match inv.invoke(&router::route(&job), &job, None) {
                Delegation::Failed(msg) => assert!(msg.contains("spawn"), "spawn failure: {msg}"),
                other => panic!("expected Failed, got {:?}", DelegDbg(&other)),
            }
        }

        /// Small debug shim so panic messages can print a Delegation (which isn't Debug).
        struct DelegDbg<'a>(&'a Delegation);
        impl std::fmt::Debug for DelegDbg<'_> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self.0 {
                    Delegation::Delivered(c) => write!(f, "Delivered({c})"),
                    Delegation::Failed(e) => write!(f, "Failed({e})"),
                    Delegation::TimedOut(d) => write!(f, "TimedOut({d:?})"),
                }
            }
        }
    }
}
