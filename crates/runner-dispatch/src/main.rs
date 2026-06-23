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
    fn invoke(&self, plan: &KernelPlan, job: &JobSpec) -> Result<(), String>;
}

/// The default invoker: logs the delegation it *would* perform (no subprocess). The real
/// kernel-spawn invoker (`loop`/`atc`/`hf`/`weave` + secret injection + provenance) lands in P3.
/// Only wired into the Unix `serve` path; the decision core is exercised cross-platform via tests.
#[cfg(unix)]
struct DryRunInvoker;
#[cfg(unix)]
impl KernelInvoker for DryRunInvoker {
    fn invoke(&self, plan: &KernelPlan, job: &JobSpec) -> Result<(), String> {
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
        Ok(())
    }
}

/// Handle one received frame end-to-end. Pure over its inputs (no socket), so the accept→verify→
/// isolate→breaker→route→delegate decision is unit-tested directly. Fail-closed: every non-happy
/// path is a `DispatchResponse::rejected`. The `guard` persists across connections (the runaway-loop
/// breaker is stateful by design).
fn handle_request(
    key: &[u8],
    invoker: &dyn KernelInvoker,
    guard: &mut LoopGuard,
    raw: &[u8],
) -> DispatchResponse {
    let req: DispatchRequest = match serde_json::from_slice(raw) {
        Ok(r) => r,
        Err(e) => return DispatchResponse::rejected(format!("unparseable dispatch frame: {e}")),
    };
    let job = match verify_frame(key, &req) {
        Ok(j) => j,
        Err(e) => return DispatchResponse::rejected(format!("frame rejected: {e}")),
    };

    // Fork-PR isolation (ADR-0008 §6): untrusted fork code must NEVER run on self-hosted hardware.
    // Enforced HERE, before any kernel is touched, so a forged-but-signed fork job still can't run.
    let placement = safety::placement(&job);
    if placement == Placement::HostedOnly {
        return DispatchResponse::rejected(
            "fork-triggered job must run on GitHub-hosted infra, not the self-hosted dispatcher",
        );
    }

    // Runaway-loop circuit breaker: a self-hosted autonomous loop dispatching the SAME work over
    // and over is the #1 unattended-loop failure mode (cost blowups). Trip fail-closed before the
    // kernel is touched. Distinct work and normal retries pass; only a tight identical loop trips.
    if let Verdict::Trip { count } = guard.observe(&job) {
        return DispatchResponse::rejected(format!(
            "loop breaker tripped: identical job dispatched {count}x within the recent window \
             (runaway-loop guard); back off or vary the work"
        ));
    }

    let plan = router::route(&job);
    match invoker.invoke(&plan, &job) {
        Ok(()) => DispatchResponse {
            accepted: true,
            kernel: Some(plan.kernel.program().to_string()),
            placement: Some(format!("{placement:?}")),
            intent: Some(plan.intent.clone()),
            error: None,
        },
        Err(e) => DispatchResponse::rejected(format!(
            "kernel `{}` invocation failed: {e}",
            plan.kernel.program()
        )),
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
) -> std::io::Result<()> {
    use std::io::Write;
    let (mut stream, _addr) = listener.accept()?;
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw)?;
    let resp = handle_request(key, invoker, guard, &raw);
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
) -> std::io::Result<()> {
    use std::os::unix::net::UnixListener;
    if socket_path.exists() {
        std::fs::remove_file(socket_path)?;
    }
    let listener = UnixListener::bind(socket_path)?;
    eprintln!(
        "fxrun-dispatch: listening on {} (loop breaker: {} identical / window {})",
        socket_path.display(),
        guard.trip_threshold(),
        guard.window()
    );
    loop {
        if let Err(e) = serve_once(&listener, key, invoker, guard) {
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
            serve(
                std::path::Path::new(path),
                key.as_bytes(),
                &DryRunInvoker,
                &mut guard,
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
    }
    impl RecordingInvoker {
        fn calls(&self) -> usize {
            self.calls.load(Ordering::Relaxed)
        }
    }
    impl KernelInvoker for RecordingInvoker {
        fn invoke(&self, _plan: &KernelPlan, _job: &JobSpec) -> Result<(), String> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            Ok(())
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
        let resp = handle_request(b"k", &inv, &mut LoopGuard::default(), &raw);
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
        let resp = handle_request(b"k", &inv, &mut LoopGuard::default(), &raw);
        assert!(!resp.accepted);
        assert!(resp.error.unwrap().contains("fork"));
        assert_eq!(inv.calls(), 0, "fork job must never reach a kernel");
    }

    #[test]
    fn bad_signature_is_rejected() {
        let inv = RecordingInvoker::default();
        let frame = sign_frame(b"k", &ci_spec(false)).unwrap();
        let raw = serde_json::to_vec(&frame).unwrap();
        let resp = handle_request(b"wrong-key", &inv, &mut LoopGuard::default(), &raw);
        assert!(!resp.accepted);
        assert_eq!(inv.calls(), 0);
    }

    #[test]
    fn unparseable_frame_is_rejected() {
        let inv = RecordingInvoker::default();
        let resp = handle_request(b"k", &inv, &mut LoopGuard::default(), b"this is not json");
        assert!(!resp.accepted);
        assert_eq!(inv.calls(), 0);
    }

    #[test]
    fn loop_breaker_trips_on_repeated_identical_dispatch_and_spares_the_kernel() {
        let inv = RecordingInvoker::default();
        // Tight breaker: trip on the 2nd identical dispatch within a window of 4.
        let mut guard = LoopGuard::new(4, 2);
        let frame = sign_frame(b"k", &ci_spec(false)).unwrap();
        let raw = serde_json::to_vec(&frame).unwrap();

        // First identical dispatch is delegated…
        let first = handle_request(b"k", &inv, &mut guard, &raw);
        assert!(first.accepted);
        assert_eq!(inv.calls(), 1);

        // …the second identical dispatch trips the breaker and never reaches the kernel.
        let second = handle_request(b"k", &inv, &mut guard, &raw);
        assert!(!second.accepted);
        assert!(second.error.unwrap().contains("loop breaker"));
        assert_eq!(inv.calls(), 1, "tripped job must not be delegated");
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
            assert!(handle_request(b"k", &inv, &mut guard, &raw).accepted);
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
            serve_once(&listener, &key_srv, &*rec_srv, &mut LoopGuard::default()).unwrap();
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
