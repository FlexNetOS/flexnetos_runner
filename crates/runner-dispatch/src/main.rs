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

use runner_core::cost::JobCost;
use runner_core::events::{DispatchEvent, EventSink, NullSink, Outcome};
use runner_core::governor::{Admission, Governor};
use runner_core::jobspec::JobSpec;
use runner_core::loopguard::{LoopGuard, Verdict};
use runner_core::router::{self, KernelPlan};
use runner_core::safety::{self, Placement};
use runner_core::wire::{verify_frame, DispatchRequest, DispatchResponse};
use std::io::Read;

/// The delegation seam: turn a routed [`KernelPlan`] into a real kernel invocation. The dispatcher
/// NEVER reimplements a kernel — it shells out to the existing binary. Injected so the UDS path is
/// testable with a fake (no kernels spawned in CI).
trait KernelInvoker {
    /// Invoke the kernel and return the job's measured [`JobCost`] (the `atc → runner` cost seam).
    /// Kernels that don't measure cost (the dry-run, or non-agent kernels) return [`JobCost::ZERO`].
    fn invoke(&self, plan: &KernelPlan, job: &JobSpec) -> Result<JobCost, String>;
}

/// The default invoker: logs the delegation it *would* perform (no subprocess). The real
/// kernel-spawn invoker (`loop`/`atc`/`hf`/`weave` + secret injection + provenance) lands in P3.
/// Only wired into the Unix `serve` path; the decision core is exercised cross-platform via tests.
#[cfg(unix)]
struct DryRunInvoker;
#[cfg(unix)]
impl KernelInvoker for DryRunInvoker {
    fn invoke(&self, plan: &KernelPlan, job: &JobSpec) -> Result<JobCost, String> {
        let agent = match plan.agent {
            Some(a) => format!(", agent {a}"),
            None => String::new(),
        };
        eprintln!(
            "  delegate → `{}` : {} (job {}, corr {}, repo {}{})",
            plan.kernel.program(),
            plan.intent,
            job.id,
            job.correlation_id,
            plan.repo,
            agent
        );
        // P3: the real atc invoker reports the job's measured cost here. The dry-run measures none.
        Ok(JobCost::ZERO)
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

/// Handle one received frame end-to-end. Pure over its inputs (no socket), so the accept→verify→
/// isolate→breaker→budget→route→delegate decision is unit-tested directly. Fail-closed: every
/// non-happy path is a `DispatchResponse::rejected`. `guard` and `governor` persist across
/// connections (the runaway-loop breaker and the dispatch budget are stateful by design); every
/// terminal decision is also written to the audit `sink` (kclaw0 `event-system.js` lineage).
fn handle_request(
    key: &[u8],
    invoker: &dyn KernelInvoker,
    guard: &mut LoopGuard,
    governor: &mut Governor,
    sink: &dyn EventSink,
    raw: &[u8],
) -> DispatchResponse {
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

    // Fork-PR isolation (ADR-0008 §6): untrusted fork code must NEVER run on self-hosted hardware.
    // Enforced HERE, before any kernel is touched, so a forged-but-signed fork job still can't run.
    let placement = safety::placement(&job);
    if placement == Placement::HostedOnly {
        let detail = "fork-triggered job must run on GitHub-hosted infra, not the self-hosted \
                      dispatcher";
        sink.emit(&DispatchEvent::for_job(Outcome::ForkRejected, &job).with_detail(detail));
        return DispatchResponse::rejected(detail);
    }

    // Runaway-loop circuit breaker: a self-hosted autonomous loop dispatching the SAME work over
    // and over is the #1 unattended-loop failure mode (cost blowups). Trip fail-closed before the
    // kernel is touched. Distinct work and normal retries pass; only a tight identical loop trips.
    if let Verdict::Trip { count } = guard.observe(&job) {
        let detail = format!(
            "loop breaker tripped: identical job dispatched {count}x within the recent window \
             (runaway-loop guard); back off or vary the work"
        );
        sink.emit(&DispatchEvent::for_job(Outcome::LoopTripped, &job).with_detail(&detail));
        return DispatchResponse::rejected(detail);
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

    let plan = router::route(&job);
    let program = plan.kernel.program();
    match invoker.invoke(&plan, &job) {
        Ok(cost) => {
            // Charge the cost atc reported so the NEXT admit sees it (fail-open: ZERO is a no-op).
            governor.charge(cost);
            let mut event = DispatchEvent::for_job(Outcome::Delegated, &job).with_kernel(program);
            if cost.is_measured() {
                event = event.with_cost(cost);
            }
            sink.emit(&event);
            DispatchResponse {
                accepted: true,
                kernel: Some(program.to_string()),
                placement: Some(format!("{placement:?}")),
                intent: Some(plan.intent.clone()),
                error: None,
            }
        }
        Err(e) => {
            let detail = format!("kernel `{program}` invocation failed: {e}");
            sink.emit(
                &DispatchEvent::for_job(Outcome::KernelFailed, &job)
                    .with_kernel(program)
                    .with_detail(&detail),
            );
            DispatchResponse::rejected(detail)
        }
    }
}

/// Accept exactly one connection, handle its frame, and write the reply. Factored out so the loop
/// (and tests) can drive a single round-trip.
#[cfg(unix)]
fn serve_once(
    listener: &std::os::unix::net::UnixListener,
    key: &[u8],
    invoker: &dyn KernelInvoker,
    guard: &mut LoopGuard,
    governor: &mut Governor,
    sink: &dyn EventSink,
) -> std::io::Result<()> {
    use std::io::Write;
    let (mut stream, _addr) = listener.accept()?;
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw)?;
    let resp = handle_request(key, invoker, guard, governor, sink, &raw);
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
fn serve(
    socket_path: &std::path::Path,
    key: &[u8],
    invoker: &dyn KernelInvoker,
    guard: &mut LoopGuard,
    governor: &mut Governor,
    sink: &dyn EventSink,
) -> std::io::Result<()> {
    use std::os::unix::net::UnixListener;
    if socket_path.exists() {
        std::fs::remove_file(socket_path)?;
    }
    let listener = UnixListener::bind(socket_path)?;
    eprintln!(
        "fxrun-dispatch: listening on {} (loop breaker: {} identical / window {}; dispatch budget: {})",
        socket_path.display(),
        guard.trip_threshold(),
        guard.window(),
        render_budget(&governor.budget())
    );
    loop {
        if let Err(e) = serve_once(&listener, key, invoker, guard, governor, sink) {
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
        "unlimited".to_string()
    } else {
        parts.join(" / ")
    }
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
            // Bounded-autonomy ceiling across jobs/tokens/USD: 0/unset → uncapped per dimension
            // (behaviour-preserving default). Token/USD caps only bite once atc reports cost.
            let mut governor = Governor::from_env(
                env_usize("FXRUN_DISPATCH_BUDGET", 0),
                env_u64("FXRUN_TOKEN_BUDGET"),
                env_u64("FXRUN_USD_MICROS_BUDGET"),
            );
            // Audit trail: NDJSON to FXRUN_EVENT_LOG when set, else a no-op sink (default).
            let event_log = std::env::var("FXRUN_EVENT_LOG").unwrap_or_default();
            let sink: Box<dyn EventSink> = if event_log.trim().is_empty() {
                Box::new(NullSink)
            } else {
                eprintln!("fxrun-dispatch: audit log → {event_log}");
                Box::new(FileSink {
                    path: std::path::PathBuf::from(event_log),
                })
            };
            serve(
                std::path::Path::new(path),
                key.as_bytes(),
                &DryRunInvoker,
                &mut guard,
                &mut governor,
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
        fn invoke(&self, _plan: &KernelPlan, _job: &JobSpec) -> Result<JobCost, String> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            Ok(self.cost)
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
            &mut LoopGuard::default(),
            &mut Governor::unlimited(),
            &NullSink,
            &raw,
        );
        assert!(resp.accepted);
        assert_eq!(resp.kernel.as_deref(), Some("loop"));
        assert_eq!(resp.placement.as_deref(), Some("SelfHosted"));
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
            &mut LoopGuard::default(),
            &mut Governor::unlimited(),
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
            &mut LoopGuard::default(),
            &mut Governor::unlimited(),
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
            &mut LoopGuard::default(),
            &mut Governor::unlimited(),
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
            &mut guard,
            &mut gov,
            &sink,
            &serde_json::to_vec(&ok).unwrap(),
        );
        let forked = sign_frame(b"k", &ci_spec(true)).unwrap();
        handle_request(
            b"k",
            &inv,
            &mut guard,
            &mut gov,
            &sink,
            &serde_json::to_vec(&forked).unwrap(),
        );
        // An unparseable frame is audited too — with no job fields.
        handle_request(b"k", &inv, &mut guard, &mut gov, &sink, b"garbage");

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
            &mut guard,
            &mut Governor::unlimited(),
            &NullSink,
            &raw,
        );
        assert!(first.accepted);
        assert_eq!(inv.calls(), 1);

        // …the second identical dispatch trips the breaker and never reaches the kernel.
        let second = handle_request(
            b"k",
            &inv,
            &mut guard,
            &mut Governor::unlimited(),
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
            handle_request(b"k", &inv, &mut guard, &mut governor, &NullSink, &raw)
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
                &mut guard,
                &mut gov,
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
                    &mut guard,
                    &mut Governor::unlimited(),
                    &NullSink,
                    &raw
                )
                .accepted
            );
        }
        assert_eq!(inv.calls(), 6);
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
                &mut LoopGuard::default(),
                &mut Governor::unlimited(),
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
}
