//! `fxrun-dispatch` — the meta-native dispatcher (ADR-0008 §2/S7).
//!
//! Two modes:
//! - **`--socket <path>` (P2):** bind a Unix-domain socket, accept signed [`DispatchRequest`]
//!   frames from `flexnetos_github_app`, verify the HMAC, enforce fork-PR isolation, route to a
//!   kernel via [`runner_core::router`], and delegate through the [`KernelInvoker`] seam —
//!   **never reimplementing** loop_lib / atc / handoff / weave.
//! - **stdin (P0):** read one JSON `JobSpec`, print the plan (dry-run smoke aid).
//!
//! Protocol: one request per connection — the client writes the JSON [`DispatchRequest`], shuts
//! down its write half, then reads the JSON [`DispatchResponse`]. The HMAC key comes from
//! envctl's vault in P3 (`FXRUN_DISPATCH_KEY` is the transitional source).

use runner_core::jobspec::JobSpec;
use runner_core::router::{self, KernelPlan};
use runner_core::safety::{self, Placement};
use runner_core::wire::{verify_frame, DispatchRequest, DispatchResponse};
use std::io::{Read, Write};
use std::os::unix::net::UnixListener;
use std::path::Path;

/// The delegation seam: turn a routed [`KernelPlan`] into a real kernel invocation. The dispatcher
/// NEVER reimplements a kernel — it shells out to the existing binary. Injected so the UDS path is
/// testable with a fake (no kernels spawned in CI).
trait KernelInvoker {
    fn invoke(&self, plan: &KernelPlan, job: &JobSpec) -> Result<(), String>;
}

/// The default invoker: logs the delegation it *would* perform (no subprocess). The real
/// kernel-spawn invoker (`loop`/`atc`/`hf`/`weave` + secret injection + provenance) lands in P3.
struct DryRunInvoker;
impl KernelInvoker for DryRunInvoker {
    fn invoke(&self, plan: &KernelPlan, job: &JobSpec) -> Result<(), String> {
        eprintln!(
            "  delegate → `{}` : {} (job {}, corr {}, repo {})",
            plan.kernel.program(),
            plan.intent,
            job.id,
            job.correlation_id,
            plan.repo
        );
        Ok(())
    }
}

/// Handle one received frame end-to-end. Pure over its inputs (no socket), so the accept→verify→
/// isolate→route→delegate decision is unit-tested directly. Fail-closed: every non-happy path is
/// a `DispatchResponse::rejected`.
fn handle_request(key: &[u8], invoker: &dyn KernelInvoker, raw: &[u8]) -> DispatchResponse {
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
fn serve_once(
    listener: &UnixListener,
    key: &[u8],
    invoker: &dyn KernelInvoker,
) -> std::io::Result<()> {
    let (mut stream, _addr) = listener.accept()?;
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw)?;
    let resp = handle_request(key, invoker, &raw);
    let bytes = serde_json::to_vec(&resp)
        .unwrap_or_else(|_| br#"{"accepted":false,"error":"response encode failed"}"#.to_vec());
    stream.write_all(&bytes)?;
    stream.flush()?;
    Ok(())
}

/// Bind `socket_path` and serve forever (one job per connection — the ephemeral-runner model).
/// Removes a stale socket first; a per-connection error is logged and the loop continues.
fn serve(socket_path: &Path, key: &[u8], invoker: &dyn KernelInvoker) -> std::io::Result<()> {
    if socket_path.exists() {
        std::fs::remove_file(socket_path)?;
    }
    let listener = UnixListener::bind(socket_path)?;
    eprintln!("fxrun-dispatch: listening on {}", socket_path.display());
    loop {
        if let Err(e) = serve_once(&listener, key, invoker) {
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
    println!(
        "verified={verified} placement={place:?} kernel={:?} program={} intent='{}'",
        plan.kernel,
        plan.kernel.program(),
        plan.intent
    );
    Ok(())
}

fn main() -> anyhow::Result<()> {
    // P3: fetch from envctl's vault, not the environment.
    let key = std::env::var("FXRUN_DISPATCH_KEY").unwrap_or_default();
    let args: Vec<String> = std::env::args().collect();

    if let Some(i) = args.iter().position(|a| a == "--socket") {
        let path = args
            .get(i + 1)
            .ok_or_else(|| anyhow::anyhow!("--socket requires a path"))?;
        // Fail-closed: a server that can't verify frames must not start.
        if key.is_empty() {
            anyhow::bail!(
                "refusing to serve without FXRUN_DISPATCH_KEY (P3: injected from envctl's vault)"
            );
        }
        serve(Path::new(path), key.as_bytes(), &DryRunInvoker)?;
        return Ok(());
    }

    stdin_dry_run(&key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use runner_core::jobspec::{JobKind, JobSpec};
    use runner_core::wire::sign_frame;
    use std::net::Shutdown;
    use std::os::unix::net::UnixStream;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

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

    fn unique_sock() -> std::path::PathBuf {
        static N: AtomicUsize = AtomicUsize::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("fxrun-dispatch-{}-{n}.sock", std::process::id()))
    }

    #[test]
    fn accepts_signed_non_fork_job_and_delegates() {
        let inv = RecordingInvoker::default();
        let frame = sign_frame(b"k", &ci_spec(false)).unwrap();
        let raw = serde_json::to_vec(&frame).unwrap();
        let resp = handle_request(b"k", &inv, &raw);
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
        let resp = handle_request(b"k", &inv, &raw);
        assert!(!resp.accepted);
        assert!(resp.error.unwrap().contains("fork"));
        assert_eq!(inv.calls(), 0, "fork job must never reach a kernel");
    }

    #[test]
    fn bad_signature_is_rejected() {
        let inv = RecordingInvoker::default();
        let frame = sign_frame(b"k", &ci_spec(false)).unwrap();
        let raw = serde_json::to_vec(&frame).unwrap();
        let resp = handle_request(b"wrong-key", &inv, &raw);
        assert!(!resp.accepted);
        assert_eq!(inv.calls(), 0);
    }

    #[test]
    fn unparseable_frame_is_rejected() {
        let inv = RecordingInvoker::default();
        let resp = handle_request(b"k", &inv, b"this is not json");
        assert!(!resp.accepted);
        assert_eq!(inv.calls(), 0);
    }

    #[test]
    fn real_uds_roundtrip() {
        let key = b"dispatch-key".to_vec();
        let sock = unique_sock();
        let listener = UnixListener::bind(&sock).unwrap();

        let recorder = Arc::new(RecordingInvoker::default());
        let rec_srv = recorder.clone();
        let key_srv = key.clone();
        let handle = std::thread::spawn(move || {
            serve_once(&listener, &key_srv, &*rec_srv).unwrap();
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
