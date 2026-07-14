use anyhow::{anyhow, bail, Context, Result};
use clap::{Args, Subcommand, ValueEnum};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use walkdir::{DirEntry, WalkDir};

#[derive(Subcommand)]
pub enum RunnerStateCommand {
    /// Classify preserved runner-state churn without mutating anything.
    Audit(RunnerStateAuditCli),
    /// Normalize known benign producer churn while preserving runner identity/state.
    Normalize(RunnerStateNormalizeCli),
    /// Run audit + normalize + optional snapshot/publish automation.
    Settle(RunnerStateSettleCli),
}

#[derive(Args, Clone, Debug)]
pub struct RunnerStateAuditCli {
    /// Repository root containing `_work` and optional `.kb`.
    #[arg(long, default_value = ".")]
    root: PathBuf,
    /// Runner slot selector: 01, 02, or all.
    #[arg(long, default_value = "all")]
    slots: String,
    /// Fail if any blocker/unclassified dirty state is found.
    #[arg(long)]
    strict: bool,
    /// Output format.
    #[arg(long, value_enum, default_value = "human")]
    format: RunnerStateOutputFormat,
}

#[derive(Args, Clone, Debug)]
pub struct RunnerStateNormalizeCli {
    /// Repository root containing `_work` and optional `.kb`.
    #[arg(long, default_value = ".")]
    root: PathBuf,
    /// Runner slot selector: 01, 02, or all.
    #[arg(long, default_value = "all")]
    slots: String,
    /// Show actions without mutating files.
    #[arg(long)]
    dry_run: bool,
    /// Output format.
    #[arg(long, value_enum, default_value = "human")]
    format: RunnerStateOutputFormat,
}

#[derive(Args, Clone, Debug)]
pub struct RunnerStateSettleCli {
    /// Repository root containing `_work` and optional `.kb`.
    #[arg(long, default_value = ".")]
    root: PathBuf,
    /// Runner slot selector: 01, 02, or all.
    #[arg(long, default_value = "all")]
    slots: String,
    /// Require no active Runner.Worker/Runner.Listener processes before mutating.
    #[arg(long)]
    require_idle: bool,
    /// Override the idle gate.
    #[arg(long)]
    force: bool,
    /// Commit the settled snapshot locally when mutations exist.
    #[arg(long)]
    commit: bool,
    /// Commit message for --commit.
    #[arg(
        long,
        default_value = "chore(runner-state): settle runner runtime snapshot"
    )]
    message: String,
    /// Push current branch after committing.
    #[arg(long)]
    push_pr: bool,
    /// Arm auto-merge for the created PR when --push-pr is used.
    #[arg(long)]
    automerge: bool,
    /// Show actions without mutating files.
    #[arg(long)]
    dry_run: bool,
    /// Fail if any blocker/unclassified dirty state remains after normalization.
    #[arg(long)]
    strict: bool,
    /// Output format.
    #[arg(long, value_enum, default_value = "human")]
    format: RunnerStateOutputFormat,
}

#[derive(Copy, Clone, Eq, PartialEq, Debug, ValueEnum)]
pub enum RunnerStateOutputFormat {
    Human,
    Json,
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum RunnerStateSlots {
    All,
    Slot(u8),
}

impl RunnerStateSlots {
    fn parse(raw: &str) -> Result<Self> {
        match raw {
            "all" => Ok(Self::All),
            "01" | "1" => Ok(Self::Slot(1)),
            "02" | "2" => Ok(Self::Slot(2)),
            other => bail!("invalid runner-state slot '{other}' (expected 01, 02, or all)"),
        }
    }

    fn matches_path(&self, path: &str) -> bool {
        match self {
            Self::All => true,
            Self::Slot(slot) => {
                let slot = format!("{slot:02}");
                path.contains(&format!("runner-home-{slot}"))
                    || path.contains(&format!("actions-runner-{slot}-work"))
                    || path.contains(&format!("actions-runner-{slot}"))
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct RunnerStateAuditOptions {
    pub root: PathBuf,
    pub strict: bool,
    pub slots: RunnerStateSlots,
}

#[derive(Clone, Debug)]
pub struct RunnerStateNormalizeOptions {
    pub root: PathBuf,
    pub dry_run: bool,
    pub slots: RunnerStateSlots,
}

#[derive(Clone, Debug)]
pub struct RunnerStateSettleOptions {
    pub root: PathBuf,
    pub slots: RunnerStateSlots,
    pub require_idle: bool,
    pub force: bool,
    pub commit: bool,
    pub message: String,
    pub push_pr: bool,
    pub automerge: bool,
    pub dry_run: bool,
    pub strict: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RunnerStateClass {
    LiveRunnerState,
    CacheState,
    ConfigChurn,
    GitKbRuntime,
    DeniedSensitive,
    Unclassified,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RunnerStateEntry {
    pub path: String,
    pub class: RunnerStateClass,
    pub reason: String,
    pub normalized: bool,
    pub blocker: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RunnerStateAuditReport {
    pub root: String,
    pub strict: bool,
    pub dirty_source: String,
    pub entries: Vec<RunnerStateEntry>,
    pub by_class: BTreeMap<RunnerStateClass, usize>,
    pub blockers: Vec<String>,
    pub unclassified_paths: Vec<String>,
    pub duplicate_safe_directories: BTreeMap<String, usize>,
    pub active_processes: Vec<String>,
    pub better_rule: &'static str,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RunnerStateNormalizeReport {
    pub root: String,
    pub dry_run: bool,
    pub normalized_paths: Vec<String>,
    pub removed_runtime_paths: Vec<String>,
    pub preserved_paths: Vec<String>,
    pub audit_before: RunnerStateAuditReport,
    pub audit_after: RunnerStateAuditReport,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RunnerStateSettleReport {
    pub root: String,
    pub dry_run: bool,
    pub strict: bool,
    pub require_idle: bool,
    pub normalized_paths: Vec<String>,
    pub removed_runtime_paths: Vec<String>,
    pub preserved_paths: Vec<String>,
    pub actions: Vec<String>,
    pub audit_after: RunnerStateAuditReport,
}

pub fn execute(cmd: RunnerStateCommand) -> Result<()> {
    match cmd {
        RunnerStateCommand::Audit(args) => {
            let report = audit_runner_state(RunnerStateAuditOptions {
                root: args.root,
                strict: args.strict,
                slots: RunnerStateSlots::parse(&args.slots)?,
            })?;
            emit_audit(&report, args.format)?;
            if args.strict && (!report.blockers.is_empty() || !report.unclassified_paths.is_empty())
            {
                bail!(
                    "runner-state strict audit failed: {} blocker(s), {} unclassified path(s)",
                    report.blockers.len(),
                    report.unclassified_paths.len()
                );
            }
        }
        RunnerStateCommand::Normalize(args) => {
            let report = normalize_runner_state(RunnerStateNormalizeOptions {
                root: args.root,
                dry_run: args.dry_run,
                slots: RunnerStateSlots::parse(&args.slots)?,
            })?;
            emit_normalize(&report, args.format)?;
        }
        RunnerStateCommand::Settle(args) => {
            let report = settle_runner_state(RunnerStateSettleOptions {
                root: args.root,
                slots: RunnerStateSlots::parse(&args.slots)?,
                require_idle: args.require_idle,
                force: args.force,
                commit: args.commit,
                message: args.message,
                push_pr: args.push_pr,
                automerge: args.automerge,
                dry_run: args.dry_run,
                strict: args.strict,
            })?;
            emit_settle(&report, args.format)?;
            if args.strict
                && (!report.audit_after.blockers.is_empty()
                    || !report.audit_after.unclassified_paths.is_empty())
            {
                bail!("runner-state settle strict gate failed after normalization");
            }
        }
    }
    Ok(())
}

pub fn audit_runner_state(opts: RunnerStateAuditOptions) -> Result<RunnerStateAuditReport> {
    let root = normalize_root(&opts.root)?;
    let (paths, dirty_source) = candidate_paths(&root)?;
    let mut entries = Vec::new();
    let mut duplicate_safe_directories = BTreeMap::new();

    for path in paths
        .into_iter()
        .filter(|path| opts.slots.matches_path(path))
    {
        if path == ".gitignore" && gitignore_hides_work(&root)? {
            entries.push(RunnerStateEntry {
                path,
                class: RunnerStateClass::ConfigChurn,
                reason: "broad _work ignore hides preserved runner state".into(),
                normalized: true,
                blocker: true,
            });
            continue;
        }
        let entry = classify_path(&root, path.clone())?;
        if entry.class == RunnerStateClass::ConfigChurn && path.ends_with(".gitconfig") {
            if let Some(duplicates) = safe_directory_duplicates(&root.join(&path))? {
                duplicate_safe_directories.insert(path.clone(), duplicates);
            }
        }
        entries.push(entry);
    }

    entries.sort_by(|a, b| a.path.cmp(&b.path));
    let mut by_class = BTreeMap::new();
    let mut blockers = Vec::new();
    let mut unclassified_paths = Vec::new();
    for entry in &entries {
        *by_class.entry(entry.class).or_insert(0) += 1;
        if entry.blocker {
            blockers.push(format!(
                "{:?}: {} ({})",
                entry.class, entry.path, entry.reason
            ));
        }
        if entry.class == RunnerStateClass::Unclassified {
            unclassified_paths.push(entry.path.clone());
        }
    }
    let active_processes = active_runner_processes();
    if opts.strict && !active_processes.is_empty() {
        blockers.push("active runner process gate is not idle".into());
    }

    Ok(RunnerStateAuditReport {
        root: root.display().to_string(),
        strict: opts.strict,
        dirty_source,
        entries,
        by_class,
        blockers,
        unclassified_paths,
        duplicate_safe_directories,
        active_processes,
        better_rule: "Preserved runner state must be clean, actively owned, compressed with a manifest, normalized by runner-state settle, or committed as an intentional snapshot.",
    })
}

pub fn normalize_runner_state(
    opts: RunnerStateNormalizeOptions,
) -> Result<RunnerStateNormalizeReport> {
    let root = normalize_root(&opts.root)?;
    let before = audit_runner_state(RunnerStateAuditOptions {
        root: root.clone(),
        strict: false,
        slots: opts.slots,
    })?;
    let mut normalized_paths = Vec::new();
    let mut removed_runtime_paths = Vec::new();
    let mut preserved_paths = Vec::new();

    for entry in &before.entries {
        match entry.class {
            RunnerStateClass::ConfigChurn if entry.path.ends_with(".gitconfig") => {
                if !opts.dry_run {
                    dedupe_safe_directories(&root.join(&entry.path))?;
                }
                normalized_paths.push(entry.path.clone());
            }
            RunnerStateClass::ConfigChurn if entry.path == ".gitignore" => {
                if !opts.dry_run {
                    rewrite_gitignore_without_broad_work_ignore(&root)?;
                }
                normalized_paths.push(entry.path.clone());
            }
            RunnerStateClass::GitKbRuntime => {
                if !opts.dry_run {
                    remove_path_if_exists(&root.join(&entry.path))?;
                }
                removed_runtime_paths.push(entry.path.clone());
            }
            RunnerStateClass::LiveRunnerState
            | RunnerStateClass::CacheState
            | RunnerStateClass::DeniedSensitive
            | RunnerStateClass::Unclassified => preserved_paths.push(entry.path.clone()),
            RunnerStateClass::ConfigChurn => preserved_paths.push(entry.path.clone()),
        }
    }

    let after = audit_runner_state(RunnerStateAuditOptions {
        root: root.clone(),
        strict: false,
        slots: opts.slots,
    })?;
    Ok(RunnerStateNormalizeReport {
        root: root.display().to_string(),
        dry_run: opts.dry_run,
        normalized_paths,
        removed_runtime_paths,
        preserved_paths,
        audit_before: before,
        audit_after: after,
    })
}

pub fn settle_runner_state(opts: RunnerStateSettleOptions) -> Result<RunnerStateSettleReport> {
    let root = normalize_root(&opts.root)?;
    let active = active_runner_processes();
    if opts.require_idle && !opts.force && !active.is_empty() {
        bail!(
            "runner-state settle requires idle runners; active processes: {}",
            active.join(" | ")
        );
    }

    let normalized = normalize_runner_state(RunnerStateNormalizeOptions {
        root: root.clone(),
        dry_run: opts.dry_run,
        slots: opts.slots,
    })?;
    let mut actions = Vec::new();
    if opts.commit {
        if opts.dry_run {
            actions.push(format!("dry-run: would commit snapshot: {}", opts.message));
        } else {
            commit_snapshot(&root, &opts.message)?;
            actions.push(format!("committed snapshot: {}", opts.message));
        }
    }
    if opts.push_pr {
        if !opts.commit {
            bail!("--push-pr requires --commit so the PR has a snapshot commit");
        }
        if opts.dry_run {
            actions.push("dry-run: would push branch and create PR".into());
        } else {
            push_pr(&root, opts.automerge)?;
            actions.push("pushed branch and created PR".into());
        }
    }

    let audit_after = audit_runner_state(RunnerStateAuditOptions {
        root: root.clone(),
        strict: opts.strict,
        slots: opts.slots,
    })?;
    Ok(RunnerStateSettleReport {
        root: root.display().to_string(),
        dry_run: opts.dry_run,
        strict: opts.strict,
        require_idle: opts.require_idle,
        normalized_paths: normalized.normalized_paths,
        removed_runtime_paths: normalized.removed_runtime_paths,
        preserved_paths: normalized.preserved_paths,
        actions,
        audit_after,
    })
}

fn candidate_paths(root: &Path) -> Result<(Vec<String>, String)> {
    if root.join(".git").exists() {
        let output = Command::new("git")
            .arg("status")
            .arg("--porcelain=v1")
            .arg("--untracked-files=all")
            .current_dir(root)
            .output();
        if let Ok(output) = output {
            if output.status.success() {
                let mut paths = parse_git_status_paths(&String::from_utf8_lossy(&output.stdout));
                if gitignore_hides_work(root)? {
                    paths.push(".gitignore".into());
                }
                paths.sort();
                paths.dedup();
                return Ok((paths, "git-status".into()));
            }
        }
    }
    let mut paths = Vec::new();
    for start in [
        root.join("_work"),
        root.join(".kb/.cache"),
        root.join(".kb/workspaces"),
    ] {
        if start.exists() {
            for entry in WalkDir::new(&start)
                .follow_links(false)
                .into_iter()
                .filter_entry(not_hidden_git_dir)
                .take(50_000)
            {
                let entry = entry?;
                if entry.path() == start {
                    continue;
                }
                paths.push(relative_path(root, entry.path()));
            }
        }
    }
    if gitignore_hides_work(root)? {
        paths.push(".gitignore".into());
    }
    paths.sort();
    paths.dedup();
    Ok((paths, "filesystem-scan".into()))
}

fn parse_git_status_paths(status: &str) -> Vec<String> {
    status
        .lines()
        .filter_map(|line| line.get(3..))
        .flat_map(|path| path.split(" -> ").last())
        .map(|path| path.trim_matches('"').replace('\\', "/"))
        .filter(|path| {
            path.starts_with("_work/") || path.starts_with(".kb/") || path == ".gitignore"
        })
        .collect()
}

fn classify_path(root: &Path, path: String) -> Result<RunnerStateEntry> {
    let path_lower = path.to_ascii_lowercase();
    let file_name = path.rsplit('/').next().unwrap_or(&path);
    if is_denied_sensitive_path(&path_lower, file_name) {
        return Ok(entry(
            path,
            RunnerStateClass::DeniedSensitive,
            "runner identity/credential path fails closed",
            false,
            true,
        ));
    }
    if path.starts_with(".kb/store") {
        return Ok(entry(
            path,
            RunnerStateClass::GitKbRuntime,
            "GitKB durable store evidence is preserved for the task ledger",
            false,
            false,
        ));
    }
    if path.starts_with(".kb/.cache") || path.starts_with(".kb/workspaces") {
        return Ok(entry(
            path,
            RunnerStateClass::GitKbRuntime,
            "GitKB runtime cache/workspace can be cleaned after git-kb status/fsck",
            true,
            false,
        ));
    }
    if path == ".gitignore" {
        return Ok(entry(
            path,
            RunnerStateClass::ConfigChurn,
            "broad _work ignore must be removed",
            true,
            true,
        ));
    }
    if path.ends_with(".gitconfig") && path.contains("_work/runner-home-") {
        let duplicates = safe_directory_duplicates(&root.join(&path))?.unwrap_or(0);
        if duplicates > 0 {
            return Ok(entry(
                path,
                RunnerStateClass::ConfigChurn,
                "duplicate safe.directory entries",
                true,
                false,
            ));
        }
        return Ok(entry(
            path,
            RunnerStateClass::LiveRunnerState,
            "runner-home gitconfig without duplicate safe.directory churn",
            false,
            false,
        ));
    }
    if path.contains("/_pipelinemapping/")
        || path.ends_with("PipelineFolder.json")
        || file_name == ".runner_migrated"
    {
        return Ok(entry(
            path,
            RunnerStateClass::LiveRunnerState,
            "live runner state must be snapshotted or left to active owner",
            false,
            false,
        ));
    }
    if path.contains("/.cache/kache/")
        || path.contains("/.cache/envctl/")
        || path.ends_with("/.cargo/.global-cache")
        || path.contains("/.cargo/")
    {
        return Ok(entry(
            path,
            RunnerStateClass::CacheState,
            "cache state is compression/snapshot eligible, not hidden",
            false,
            false,
        ));
    }
    if path.starts_with("_work/") {
        return Ok(entry(
            path,
            RunnerStateClass::LiveRunnerState,
            "preserved _work topology/state",
            false,
            false,
        ));
    }
    Ok(entry(
        path,
        RunnerStateClass::Unclassified,
        "no runner-state classification rule matched",
        false,
        true,
    ))
}

fn entry(
    path: String,
    class: RunnerStateClass,
    reason: &str,
    normalized: bool,
    blocker: bool,
) -> RunnerStateEntry {
    RunnerStateEntry {
        path,
        class,
        reason: reason.into(),
        normalized,
        blocker,
    }
}

fn is_denied_sensitive_path(path_lower: &str, file_name: &str) -> bool {
    matches!(
        file_name,
        ".runner" | ".credentials" | ".credentials_rsaparams"
    ) || (matches!(file_name, ".env" | ".service")
        && path_lower.contains("_work/repos/actions-runner-"))
        || path_lower.contains("token")
        || path_lower.contains("secret")
}

fn safe_directory_duplicates(path: &Path) -> Result<Option<usize>> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).with_context(|| format!("read {}", path.display())),
    };
    let mut seen = BTreeSet::new();
    let mut duplicates = 0;
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(value) = trimmed.strip_prefix("directory = ") {
            if !seen.insert(value.trim().to_string()) {
                duplicates += 1;
            }
        }
    }
    Ok(Some(duplicates))
}

fn dedupe_safe_directories(path: &Path) -> Result<()> {
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(value) = trimmed.strip_prefix("directory = ") {
            if !seen.insert(value.trim().to_string()) {
                continue;
            }
        }
        out.push(line.to_string());
    }
    let tmp = path.with_extension("gitconfig.tmp");
    {
        let mut file =
            fs::File::create(&tmp).with_context(|| format!("create {}", tmp.display()))?;
        writeln!(file, "{}", out.join("\n"))?;
        file.sync_all().ok();
    }
    fs::rename(&tmp, path)
        .with_context(|| format!("rename {} -> {}", tmp.display(), path.display()))
}

fn gitignore_hides_work(root: &Path) -> Result<bool> {
    let path = root.join(".gitignore");
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error).with_context(|| format!("read {}", path.display())),
    };
    Ok(text.lines().any(|line| {
        let trimmed = line.trim();
        !trimmed.starts_with('#')
            && !trimmed.starts_with('!')
            && matches!(trimmed, "_work" | "_work/" | "/_work" | "/_work/")
    }))
}

fn rewrite_gitignore_without_broad_work_ignore(root: &Path) -> Result<()> {
    let path = root.join(".gitignore");
    let text = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let kept = text
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            trimmed.starts_with('#')
                || trimmed.starts_with('!')
                || !matches!(trimmed, "_work" | "_work/" | "/_work" | "/_work/")
        })
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(
        &path,
        if kept.is_empty() {
            kept
        } else {
            format!("{kept}\n")
        },
    )
    .with_context(|| format!("write {}", path.display()))
}

fn remove_path_if_exists(path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    if path.is_dir() {
        fs::remove_dir_all(path).with_context(|| format!("remove {}", path.display()))
    } else {
        fs::remove_file(path).with_context(|| format!("remove {}", path.display()))
    }
}

fn commit_snapshot(root: &Path, message: &str) -> Result<()> {
    run_git(root, &["add", "--", "_work", ".kb", ".gitignore"])?;
    let status = Command::new("git")
        .arg("diff")
        .arg("--cached")
        .arg("--quiet")
        .current_dir(root)
        .status()
        .context("git diff --cached --quiet")?;
    if status.success() {
        return Ok(());
    }
    run_git(root, &["commit", "-m", message])
}

fn push_pr(root: &Path, automerge: bool) -> Result<()> {
    run_git(root, &["push", "-u", "origin", "HEAD"])?;
    let output = Command::new("gh")
        .args(["pr", "create", "--fill"])
        .current_dir(root)
        .output()
        .context("gh pr create --fill")?;
    if !output.status.success() {
        return Err(anyhow!(
            "gh pr create failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    if automerge {
        let pr = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !pr.is_empty() {
            let _ = Command::new("gh")
                .args(["pr", "merge", &pr, "--auto", "--squash", "--delete-branch"])
                .current_dir(root)
                .status();
        }
    }
    Ok(())
}

fn run_git(root: &Path, args: &[&str]) -> Result<()> {
    let status = Command::new("git")
        .args(args)
        .current_dir(root)
        .status()
        .with_context(|| format!("git {}", args.join(" ")))?;
    if !status.success() {
        bail!("git {} failed with {status}", args.join(" "));
    }
    Ok(())
}

fn active_runner_processes() -> Vec<String> {
    if std::env::var_os("FXRUN_RUNNER_STATE_SKIP_PROCESS_CHECK").is_some() {
        return Vec::new();
    }
    let Ok(output) = Command::new("pgrep")
        .args(["-af", "Runner.Worker|Runner.Listener"])
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.contains("pgrep -af"))
        .map(str::to_string)
        .collect()
}

fn normalize_root(root: &Path) -> Result<PathBuf> {
    if root.is_absolute() {
        Ok(root.to_path_buf())
    } else {
        Ok(std::env::current_dir()?.join(root))
    }
}

fn not_hidden_git_dir(entry: &DirEntry) -> bool {
    entry.file_name() != ".git"
}

fn relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn emit_audit(report: &RunnerStateAuditReport, format: RunnerStateOutputFormat) -> Result<()> {
    if matches!(format, RunnerStateOutputFormat::Json) {
        println!("{}", serde_json::to_string_pretty(report)?);
    } else {
        println!("runner-state audit");
        println!("  root        : {}", report.root);
        println!("  source      : {}", report.dirty_source);
        println!("  entries     : {}", report.entries.len());
        println!("  blockers    : {}", report.blockers.len());
        println!("  unclassified: {}", report.unclassified_paths.len());
        println!("  rule        : {}", report.better_rule);
    }
    Ok(())
}

fn emit_normalize(
    report: &RunnerStateNormalizeReport,
    format: RunnerStateOutputFormat,
) -> Result<()> {
    if matches!(format, RunnerStateOutputFormat::Json) {
        println!("{}", serde_json::to_string_pretty(report)?);
    } else {
        println!("runner-state normalize");
        println!("  root       : {}", report.root);
        println!("  dry-run    : {}", report.dry_run);
        println!("  normalized : {}", report.normalized_paths.len());
        println!("  removed rt : {}", report.removed_runtime_paths.len());
        println!("  preserved  : {}", report.preserved_paths.len());
    }
    Ok(())
}

fn emit_settle(report: &RunnerStateSettleReport, format: RunnerStateOutputFormat) -> Result<()> {
    if matches!(format, RunnerStateOutputFormat::Json) {
        println!("{}", serde_json::to_string_pretty(report)?);
    } else {
        println!("runner-state settle");
        println!("  root       : {}", report.root);
        println!("  dry-run    : {}", report.dry_run);
        println!("  normalized : {}", report.normalized_paths.len());
        println!("  removed rt : {}", report.removed_runtime_paths.len());
        println!("  preserved  : {}", report.preserved_paths.len());
        println!("  blockers   : {}", report.audit_after.blockers.len());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(name: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "fxrun-{name}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        fs::remove_dir_all(&root).ok();
        fs::create_dir_all(&root).expect("root");
        root
    }

    #[test]
    fn runner_state_audit_classifies_every_preserved_state_seam() {
        let root = temp_root("runner-state-audit");
        fs::create_dir_all(root.join("_work/runner-home-01/.cache/kache")).unwrap();
        fs::create_dir_all(root.join("_work/runner-home-01/.cargo")).unwrap();
        fs::create_dir_all(root.join("_work/runner-home-01/.cache/envctl/abc")).unwrap();
        fs::create_dir_all(
            root.join("_work/actions-runner-01-work/_PipelineMapping/FlexNetOS/flexnetos_runner"),
        )
        .unwrap();
        fs::create_dir_all(root.join("_work/repos/actions-runner-01")).unwrap();
        fs::create_dir_all(root.join(".kb/.cache")).unwrap();
        fs::create_dir_all(root.join(".kb/workspaces/task")).unwrap();
        fs::write(
            root.join("_work/runner-home-01/.cache/kache/events.jsonl"),
            "{}",
        )
        .unwrap();
        fs::write(
            root.join("_work/runner-home-01/.cargo/.global-cache"),
            "cache",
        )
        .unwrap();
        fs::write(
            root.join("_work/runner-home-01/.cache/envctl/abc/state.json"),
            "{}",
        )
        .unwrap();
        fs::write(root.join("_work/actions-runner-01-work/_PipelineMapping/FlexNetOS/flexnetos_runner/PipelineFolder.json"), "{}").unwrap();
        fs::write(
            root.join("_work/runner-home-01/.gitconfig"),
            "[safe]\n\tdirectory = /x\n\tdirectory = /x\n",
        )
        .unwrap();
        fs::write(root.join("_work/repos/actions-runner-01/.runner"), "token").unwrap();
        fs::write(root.join(".kb/.cache/gitkb.db"), "cache").unwrap();
        fs::write(root.join(".kb/workspaces/task/doc.md"), "workspace").unwrap();

        let report = audit_runner_state(RunnerStateAuditOptions {
            root: root.clone(),
            strict: false,
            slots: RunnerStateSlots::All,
        })
        .expect("audit");

        assert!(report
            .by_class
            .contains_key(&RunnerStateClass::LiveRunnerState));
        assert!(report.by_class.contains_key(&RunnerStateClass::CacheState));
        assert!(report.by_class.contains_key(&RunnerStateClass::ConfigChurn));
        assert!(report
            .by_class
            .contains_key(&RunnerStateClass::GitKbRuntime));
        assert!(report
            .by_class
            .contains_key(&RunnerStateClass::DeniedSensitive));
        assert!(report
            .blockers
            .iter()
            .any(|b| b.contains("denied-sensitive") || b.contains("DeniedSensitive")));
        assert!(!report
            .unclassified_paths
            .iter()
            .any(|p| p.starts_with("_work/")));
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn runner_state_normalize_dedupes_gitconfig_without_removing_runner_identity() {
        let root = temp_root("runner-state-normalize");
        fs::create_dir_all(root.join("_work/runner-home-01")).unwrap();
        fs::create_dir_all(root.join("_work/repos/actions-runner-01")).unwrap();
        fs::write(
            root.join("_work/repos/actions-runner-01/.runner_migrated"),
            "preserve\n",
        )
        .unwrap();
        fs::write(
            root.join("_work/runner-home-01/.gitconfig"),
            "[safe]\n\tdirectory = /x\n\tdirectory = /x\n\tdirectory = /y\n",
        )
        .unwrap();

        let report = normalize_runner_state(RunnerStateNormalizeOptions {
            root: root.clone(),
            dry_run: false,
            slots: RunnerStateSlots::All,
        })
        .expect("normalize");

        let gitconfig = fs::read_to_string(root.join("_work/runner-home-01/.gitconfig")).unwrap();
        assert_eq!(gitconfig.matches("directory = /x").count(), 1);
        assert!(root
            .join("_work/repos/actions-runner-01/.runner_migrated")
            .exists());
        assert!(report
            .normalized_paths
            .iter()
            .any(|p| p.ends_with(".gitconfig")));
        fs::remove_dir_all(root).ok();
    }
}
