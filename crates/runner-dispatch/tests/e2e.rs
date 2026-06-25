//! End-to-end + smoke tests for the P3 dispatcher: spawn the **real** `fxrun-dispatch` binary,
//! drive it over a real Unix-domain socket with signed frames, and assert it spawns a real kernel
//! subprocess, relays the cost, kills a hung kernel at the deadline, and scrubs injected secrets from
//! the audit log — the whole pipeline through the actual transport, not the in-process decision core.
//!
//! The "kernel" is a `/bin/sh` **stub** wired in via `FXRUN_KERNEL_CMD_LOOP`, so the suite needs no
//! real kernels installed (and CI stays hermetic). Unix-only (the UDS server is `#[cfg(unix)]`).
#![cfg(unix)]

use runner_core::jobspec::{JobKind, JobSpec};
use runner_core::wire::{sign_frame, DispatchResponse};
use std::io::{Read, Write};
use std::net::Shutdown;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

const KEY: &[u8] = b"e2e-dispatch-key";

/// A unique scratch directory for one test (socket + audit log + stub live here).
fn scratch(stem: &str) -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let d = std::env::temp_dir().join(format!("fxrun-e2e-{}-{stem}-{n}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o700)).unwrap();
    d
}

/// Write an executable `/bin/sh` stub kernel.
fn write_stub(dir: &Path, body: &str) -> PathBuf {
    let path = dir.join("stub-kernel.sh");
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// A running `fxrun-dispatch` child that is killed when the guard drops (so a panicking test never
/// leaks a server).
struct Dispatcher {
    child: Child,
    sock: PathBuf,
}

impl Dispatcher {
    /// Launch the real binary on a fresh socket with the given extra env, and wait for the socket.
    fn start(dir: &Path, stub: &Path, extra_env: &[(&str, String)]) -> Self {
        let sock = dir.join("d.sock");
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_fxrun-dispatch"));
        cmd.arg("--socket")
            .arg(&sock)
            .env("FXRUN_DISPATCH_KEY", "e2e-dispatch-key")
            .env("FXRUN_KERNEL_EXEC", "1")
            .env("FXRUN_KERNEL_CMD_LOOP", stub) // CI jobs route to `loop`
            .env("FXRUN_KERNEL_CMD_ATC", stub);
        for (k, v) in extra_env {
            cmd.env(k, v);
        }
        let child = cmd.spawn().expect("spawn fxrun-dispatch");
        let start = Instant::now();
        while !sock.exists() {
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "dispatcher never bound its socket"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        Self { child, sock }
    }

    /// Send a signed job, return the parsed reply.
    fn dispatch(&self, spec: &JobSpec) -> DispatchResponse {
        let frame = sign_frame(KEY, spec).unwrap();
        let mut stream = UnixStream::connect(&self.sock).expect("connect");
        stream
            .write_all(&serde_json::to_vec(&frame).unwrap())
            .unwrap();
        stream.shutdown(Shutdown::Write).unwrap();
        let mut buf = Vec::new();
        stream.read_to_end(&mut buf).unwrap();
        serde_json::from_slice(&buf).expect("parse DispatchResponse")
    }
}

impl Drop for Dispatcher {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn ci_job(id: &str) -> JobSpec {
    JobSpec {
        id: id.into(),
        correlation_id: "e2e-corr".into(),
        from_fork: false,
        job: JobKind::Ci {
            repo: "FlexNetOS/meta".into(),
            head_sha: "cafef00d".into(),
        },
    }
}

/// SMOKE: the runner serves, spawns a real kernel subprocess, and relays its cost — end to end.
#[test]
fn smoke_real_kernel_runs_and_reports_cost() {
    let dir = scratch("smoke");
    let log = dir.join("audit.ndjson");
    // The stub kernel writes a cost report (proving the FXRUN_COST_FILE contract) and a side marker
    // (proving it really executed), then exits 0.
    let marker = dir.join("ran.txt");
    let stub = write_stub(
        &dir,
        &format!(
            "echo ran > {m}\necho '{{\"tokens\":2048,\"usd_micros\":12345}}' > \"$FXRUN_COST_FILE\"",
            m = marker.display()
        ),
    );
    let d = Dispatcher::start(
        &dir,
        &stub,
        &[("FXRUN_EVENT_LOG", log.display().to_string())],
    );

    let resp = d.dispatch(&ci_job("smoke-1"));
    assert!(resp.accepted, "valid job accepted: {resp:?}");
    assert_eq!(resp.kernel.as_deref(), Some("loop"));
    assert!(marker.exists(), "the real kernel subprocess executed");

    // The audit trail shows a delegated event carrying the cost the stub reported.
    let audit = std::fs::read_to_string(&log).unwrap();
    assert!(
        audit.contains("\"outcome\":\"delegated\""),
        "audit: {audit}"
    );
    assert!(
        audit.contains("\"tokens\":2048"),
        "cost relayed into the audit log: {audit}"
    );
    drop(d);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A kernel that exits non-zero is a rejected dispatch carrying a recovery directive.
#[test]
fn e2e_kernel_failure_is_rejected_with_recovery() {
    let dir = scratch("fail");
    let stub = write_stub(&dir, "echo 'kernel detonated' >&2\nexit 4");
    let d = Dispatcher::start(&dir, &stub, &[]);

    let resp = d.dispatch(&ci_job("fail-1"));
    assert!(!resp.accepted);
    assert!(
        resp.recovery.is_some(),
        "a failed dispatch carries recovery advice"
    );
    let err = resp.error.unwrap();
    assert!(err.contains("exited 4"), "exit code surfaced: {err}");
    drop(d);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A hung kernel is killed at the operator deadline and routed through recovery — the runner does not
/// wedge. (Real wall-clock: the stub would sleep 30s; the 1s deadline must cut it short.)
#[test]
fn e2e_hung_kernel_is_killed_at_the_deadline() {
    let dir = scratch("hang");
    let stub = write_stub(&dir, "sleep 30");
    let d = Dispatcher::start(
        &dir,
        &stub,
        &[("FXRUN_DEFAULT_DEADLINE_SECS", "1".to_string())],
    );

    let start = Instant::now();
    let resp = d.dispatch(&ci_job("hang-1"));
    let elapsed = start.elapsed();
    assert!(!resp.accepted, "a hung job is not accepted");
    assert!(
        elapsed < Duration::from_secs(8),
        "the deadline cut the 30s sleep short (took {elapsed:?})"
    );
    let err = resp.error.unwrap();
    assert!(err.contains("deadline"), "timeout surfaced: {err}");
    drop(d);
    let _ = std::fs::remove_dir_all(&dir);
}

/// An injected secret reaches the kernel's environment, and if the kernel leaks it into an error it
/// is scrubbed from BOTH the wire reply and the audit log (P3 secret injection × redaction).
#[test]
fn e2e_injected_secret_is_redacted_from_the_reply_and_audit() {
    let dir = scratch("secret");
    let log = dir.join("audit.ndjson");
    const SECRET: &str = "kernel-bearer-tok3n-do-not-leak";
    // The stub reads its injected secret and (mis)behaves by echoing it on the failure path.
    let stub = write_stub(&dir, "echo \"leaked $KERNEL_TOKEN\" >&2\nexit 1");
    let d = Dispatcher::start(
        &dir,
        &stub,
        &[
            ("KERNEL_TOKEN", SECRET.to_string()),
            ("FXRUN_INJECT_SECRETS", "KERNEL_TOKEN".to_string()),
            ("FXRUN_EVENT_LOG", log.display().to_string()),
        ],
    );

    let resp = d.dispatch(&ci_job("secret-1"));
    assert!(!resp.accepted);
    let err = resp.error.unwrap();
    assert!(
        !err.contains(SECRET),
        "secret leaked into the wire reply: {err}"
    );
    assert!(
        err.contains("«redacted»"),
        "reply shows the redaction placeholder: {err}"
    );

    let audit = std::fs::read_to_string(&log).unwrap();
    assert!(
        !audit.contains(SECRET),
        "secret leaked into the audit log:\n{audit}"
    );
    drop(d);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A tampered frame is rejected before any kernel is spawned (auth still holds with execution on).
#[test]
fn e2e_tampered_frame_is_rejected_before_execution() {
    let dir = scratch("tamper");
    let marker = dir.join("should-not-run.txt");
    let stub = write_stub(&dir, &format!("echo ran > {}", marker.display()));
    let d = Dispatcher::start(&dir, &stub, &[]);

    // Hand-build a frame whose signature won't verify under KEY.
    let spec = ci_job("tamper-1");
    let spec_json = serde_json::to_string(&spec).unwrap();
    let frame = serde_json::json!({ "spec_json": spec_json, "signature": "sha256=deadbeef" });
    let mut stream = UnixStream::connect(&d.sock).unwrap();
    stream.write_all(frame.to_string().as_bytes()).unwrap();
    stream.shutdown(Shutdown::Write).unwrap();
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).unwrap();
    let resp: DispatchResponse = serde_json::from_slice(&buf).unwrap();

    assert!(!resp.accepted, "tampered frame rejected");
    std::thread::sleep(Duration::from_millis(100));
    assert!(
        !marker.exists(),
        "no kernel was spawned for an unverified frame"
    );
    drop(d);
    let _ = std::fs::remove_dir_all(&dir);
}
