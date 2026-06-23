//! Isolated-workspace teardown guarantee (adapted from `coleam00/Archon`'s "fail → delete the
//! worktree, zero residue").
//!
//! When the real kernel invoker (P3) runs a job, it does so in an **isolated work area** (a tmpfs
//! worktree, per the ADR-0008 §6 rails). Archon's rule is that a *failed* run must leave **zero
//! residue** — the worktree is torn down on every exit path, not just the happy one, so a crashed
//! or breaker-killed job can't accumulate stale trees that leak disk or, worse, get reused.
//!
//! This module is the runner-plane **teardown guarantee**: a RAII guard ([`JobWorkspace`]) whose
//! `Drop` runs the cleanup **exactly once** on *every* exit — normal return, early-return on a
//! kernel error, or a panic. The caller may also tear down **explicitly** ([`JobWorkspace::teardown`])
//! to get a [`TeardownReport`] (did cleanup run, did residue remain); the `Drop` path is the
//! backstop for the paths that can't (panic / `?` early-return). Cleanup that *itself* fails is
//! surfaced as **residue** rather than silently dropped — a leaked tree is an auditable event.
//!
//! `runner-core` stays I/O-free: the actual `remove_dir_all` lives in the caller's injected cleanup
//! closure (the binary's provider passes a real one; tests pass a flag-setter). This defines the
//! contract the P3 invoker fulfils, before P3 exists — the same "seam first" discipline as the cost
//! seam. It is orthogonal to model routing (weave's domain): teardown is pure execution hygiene.

use std::path::{Path, PathBuf};

/// A workspace's cleanup action (typically `remove_dir_all`); runs once on teardown/drop.
type CleanupFn = Box<dyn FnOnce() -> Result<(), String>>;
/// A reporter for residue left when cleanup fails on the `Drop` backstop path.
type ResidueReporter = Box<dyn Fn(&Path, &str)>;

/// The result of tearing a workspace down.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeardownReport {
    /// Whether the cleanup action actually ran (false only if it had already run).
    pub ran: bool,
    /// `Some(reason)` if cleanup failed and a tree may have leaked (residue); `None` if clean.
    pub residue: Option<String>,
}

impl TeardownReport {
    /// Whether teardown left no residue (the success condition).
    pub fn is_clean(&self) -> bool {
        self.residue.is_none()
    }
}

/// A RAII handle to one job's isolated work area. Holds the cleanup action and guarantees it runs
/// once, on whichever exit path the job takes. Construct via a [`WorkspaceProvider`] (the real one
/// creates the directory; a test one just records intent).
pub struct JobWorkspace {
    root: PathBuf,
    /// `None` after teardown has run (so `Drop` never double-runs it).
    cleanup: Option<CleanupFn>,
    /// Optional residue reporter for the `Drop` path (which cannot return a [`TeardownReport`]).
    on_residue: Option<ResidueReporter>,
}

impl JobWorkspace {
    /// Build a workspace rooted at `root` whose teardown runs `cleanup` (e.g. `remove_dir_all`).
    pub fn new<C>(root: impl Into<PathBuf>, cleanup: C) -> Self
    where
        C: FnOnce() -> Result<(), String> + 'static,
    {
        Self {
            root: root.into(),
            cleanup: Some(Box::new(cleanup)),
            on_residue: None,
        }
    }

    /// Attach a reporter invoked (with the root + reason) if cleanup fails on the `Drop` backstop
    /// path — so a leaked tree on an un-torn-down exit is still auditable.
    pub fn with_residue_reporter<F>(mut self, reporter: F) -> Self
    where
        F: Fn(&Path, &str) + 'static,
    {
        self.on_residue = Some(Box::new(reporter));
        self
    }

    /// The isolated work area's root path.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Tear the workspace down **now**, returning whether cleanup ran and whether residue remained.
    /// Preferred over relying on `Drop` when the caller wants the report. After this, `Drop` is a
    /// no-op (cleanup never runs twice).
    pub fn teardown(mut self) -> TeardownReport {
        match self.cleanup.take() {
            Some(c) => match c() {
                Ok(()) => TeardownReport {
                    ran: true,
                    residue: None,
                },
                Err(e) => TeardownReport {
                    ran: true,
                    residue: Some(e),
                },
            },
            None => TeardownReport {
                ran: false,
                residue: None,
            },
        }
    }
}

impl Drop for JobWorkspace {
    fn drop(&mut self) {
        // Backstop: if the job exited without an explicit teardown (panic, `?` early-return on a
        // kernel error), still run cleanup exactly once. Residue is reported, never silently lost.
        if let Some(c) = self.cleanup.take() {
            if let Err(e) = c() {
                if let Some(report) = &self.on_residue {
                    report(&self.root, &e);
                }
            }
        }
    }
}

/// Acquires an isolated [`JobWorkspace`] for a job. The real implementation (P3) creates a tmpfs
/// worktree; a no-op implementation ([`NoopWorkspaceProvider`]) is the behaviour-preserving default
/// for today's dry-run (no work area to isolate yet).
pub trait WorkspaceProvider {
    /// Acquire a fresh, isolated workspace labelled by `label` (e.g. the job id / fingerprint).
    fn acquire(&self, label: &str) -> Result<JobWorkspace, String>;
}

/// A provider that hands back a workspace with a no-op cleanup — nothing is created, so nothing
/// leaks. The default until the P3 invoker creates real tmpfs worktrees.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopWorkspaceProvider;

impl WorkspaceProvider for NoopWorkspaceProvider {
    fn acquire(&self, label: &str) -> Result<JobWorkspace, String> {
        Ok(JobWorkspace::new(
            PathBuf::from(format!("<noop:{label}>")),
            || Ok(()),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;

    /// A cleanup that flips a shared flag and optionally fails.
    fn flagging_cleanup(flag: Rc<Cell<bool>>, fail: bool) -> impl FnOnce() -> Result<(), String> {
        move || {
            flag.set(true);
            if fail {
                Err("rmdir failed: tree leaked".into())
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn explicit_teardown_runs_cleanup_once_and_reports_clean() {
        let flag = Rc::new(Cell::new(false));
        let ws = JobWorkspace::new("/tmp/ws-1", flagging_cleanup(flag.clone(), false));
        assert_eq!(ws.root(), Path::new("/tmp/ws-1"));
        let report = ws.teardown();
        assert!(flag.get(), "cleanup ran");
        assert!(report.ran);
        assert!(report.is_clean());
    }

    #[test]
    fn teardown_surfaces_residue_when_cleanup_fails() {
        let flag = Rc::new(Cell::new(false));
        let ws = JobWorkspace::new("/tmp/ws-2", flagging_cleanup(flag.clone(), true));
        let report = ws.teardown();
        assert!(flag.get());
        assert!(report.ran);
        assert!(!report.is_clean());
        assert!(report.residue.unwrap().contains("leaked"));
    }

    #[test]
    fn drop_is_the_backstop_when_teardown_is_not_called() {
        let flag = Rc::new(Cell::new(false));
        {
            // The workspace is dropped at the end of this scope without an explicit teardown —
            // simulating an early-return / panic path. Cleanup must still run.
            let _ws = JobWorkspace::new("/tmp/ws-3", flagging_cleanup(flag.clone(), false));
            assert!(!flag.get(), "cleanup has not run yet");
        }
        assert!(
            flag.get(),
            "Drop guaranteed cleanup on the un-torn-down path"
        );
    }

    #[test]
    fn cleanup_never_runs_twice() {
        let count = Rc::new(Cell::new(0u32));
        let c = count.clone();
        let ws = JobWorkspace::new("/tmp/ws-4", move || {
            c.set(c.get() + 1);
            Ok(())
        });
        let _ = ws.teardown(); // runs once…
                               // …and the value is consumed, so there is no second (Drop) run.
        assert_eq!(count.get(), 1);
    }

    #[test]
    fn drop_residue_is_reported_through_the_reporter() {
        let reported = Rc::new(Cell::new(false));
        let r = reported.clone();
        let flag = Rc::new(Cell::new(false));
        {
            let _ws = JobWorkspace::new("/tmp/ws-5", flagging_cleanup(flag.clone(), true))
                .with_residue_reporter(move |_root, _reason| r.set(true));
        }
        assert!(flag.get(), "cleanup attempted on drop");
        assert!(reported.get(), "residue was reported, not silently dropped");
    }

    #[test]
    fn noop_provider_acquires_a_clean_no_residue_workspace() {
        let ws = NoopWorkspaceProvider.acquire("job-1").unwrap();
        assert!(ws.root().to_string_lossy().contains("job-1"));
        assert!(ws.teardown().is_clean());
    }
}
