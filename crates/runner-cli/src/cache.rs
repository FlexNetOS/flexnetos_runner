use anyhow::{anyhow, bail, Context, Result};
use clap::{Args, Subcommand, ValueEnum};
use filetime::{set_file_mtime, FileTime};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use walkdir::{DirEntry, WalkDir};

const DEFAULT_MIN_AGE: &str = "7d";
const MANIFEST_SCHEMA_VERSION: u8 = 1;
const DEFAULT_MANIFEST_DIR: &str = "cache-compression/manifests";

#[derive(Subcommand)]
pub enum CacheCommand {
    /// Read-only cache pressure audit. Emits candidate/skip evidence.
    Audit(CacheAuditCli),
    /// Compress old safe cache files atomically with a restore manifest.
    Compress(CacheCompressCli),
    /// Restore or verify files previously compressed by `fxrun cache compress`.
    Restore(CacheRestoreCli),
}

#[derive(Args, Clone, Debug)]
pub struct CacheAuditCli {
    /// Cache root. Defaults to repo-local `_work`.
    #[arg(long, default_value = "_work")]
    root: PathBuf,
    /// Runner slot selector: 01, 02, or all.
    #[arg(long, default_value = "all")]
    slot: String,
    /// Minimum file age before it can be selected, e.g. 7d, 12h, 30m, 60s.
    #[arg(long, default_value = DEFAULT_MIN_AGE)]
    min_age: String,
    /// Output format.
    #[arg(long, value_enum, default_value = "human")]
    format: OutputFormat,
    /// Optional path to write the audit report JSON.
    #[arg(long)]
    manifest_out: Option<PathBuf>,
    /// Additional substring/glob-ish include filters.
    #[arg(long)]
    include: Vec<String>,
    /// Additional substring/glob-ish exclude filters.
    #[arg(long)]
    exclude: Vec<String>,
}

#[derive(Args, Clone, Debug)]
pub struct CacheCompressCli {
    /// Cache root. Defaults to repo-local `_work`.
    #[arg(long, default_value = "_work")]
    root: PathBuf,
    /// Runner slot selector: 01, 02, or all.
    #[arg(long, default_value = "all")]
    slot: String,
    /// Minimum file age before it can be selected, e.g. 7d, 12h, 30m, 60s.
    #[arg(long, default_value = DEFAULT_MIN_AGE)]
    min_age: String,
    /// Show what would be compressed without mutating files.
    #[arg(long)]
    dry_run: bool,
    /// Compressor to use. zstd is the durable default.
    #[arg(long, value_enum, default_value = "zstd")]
    compressor: Compressor,
    /// Compressor level. Bounded to zstd's normal range by the implementation.
    #[arg(long, default_value_t = 3)]
    level: i32,
    /// Manifest path. Defaults to `_work/cache-compression/manifests/<timestamp>.json`.
    #[arg(long)]
    manifest: Option<PathBuf>,
    /// Lock path. Defaults to `_work/cache-compression/<slot>.lock`.
    #[arg(long)]
    lock: Option<PathBuf>,
    /// Maximum files to compress in one run.
    #[arg(long)]
    max_files: Option<usize>,
    /// Maximum original bytes to compress in one run.
    #[arg(long)]
    max_bytes: Option<u64>,
    /// Output format.
    #[arg(long, value_enum, default_value = "human")]
    format: OutputFormat,
}

#[derive(Args, Clone, Debug)]
pub struct CacheRestoreCli {
    /// Cache root. Defaults to repo-local `_work`.
    #[arg(long, default_value = "_work")]
    root: PathBuf,
    /// File path or `.zst` path to restore/verify.
    target: Option<PathBuf>,
    /// Manifest produced by `fxrun cache compress`.
    #[arg(long)]
    manifest: Option<PathBuf>,
    /// Restore all entries in the manifest.
    #[arg(long)]
    all: bool,
    /// Show restore actions without mutating files.
    #[arg(long)]
    dry_run: bool,
    /// Overwrite behavior.
    #[arg(long, value_enum, default_value = "if-missing")]
    overwrite: OverwritePolicy,
    /// Verify compressed data against manifest without restoring.
    #[arg(long)]
    verify_only: bool,
    /// Output format.
    #[arg(long, value_enum, default_value = "human")]
    format: OutputFormat,
}

#[derive(Copy, Clone, Eq, PartialEq, Debug, ValueEnum)]
pub enum OutputFormat {
    Human,
    Json,
}

#[derive(Copy, Clone, Eq, PartialEq, Debug, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum Compressor {
    Zstd,
}

#[derive(Copy, Clone, Eq, PartialEq, Debug, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum OverwritePolicy {
    Never,
    IfMissing,
    Always,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CacheSlot {
    All,
    Slot(String),
}

impl CacheSlot {
    fn parse(raw: &str) -> Result<Self> {
        match raw {
            "all" => Ok(Self::All),
            "01" | "02" => Ok(Self::Slot(raw.to_string())),
            other => bail!("invalid cache slot '{other}' (expected 01, 02, or all)"),
        }
    }

    fn matches_path(&self, path: &Path) -> bool {
        match self {
            Self::All => true,
            Self::Slot(slot) => path_components(path).iter().any(|component| {
                component == &format!("runner-home-{slot}")
                    || component == &format!("actions-runner-{slot}-work")
                    || component == &format!("actions-runner-{slot}")
            }),
        }
    }

    fn lock_label(&self) -> &str {
        match self {
            Self::All => "all",
            Self::Slot(slot) => slot.as_str(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct CacheAuditOptions {
    pub root: PathBuf,
    pub slot: CacheSlot,
    pub min_age: Duration,
    pub includes: Vec<String>,
    pub excludes: Vec<String>,
    pub active_paths: Vec<PathBuf>,
}

#[derive(Clone, Debug)]
pub struct CacheCompressOptions {
    pub root: PathBuf,
    pub slot: CacheSlot,
    pub min_age: Duration,
    pub dry_run: bool,
    pub compressor: Compressor,
    pub level: i32,
    pub manifest: PathBuf,
    pub max_files: Option<usize>,
    pub max_bytes: Option<u64>,
    pub active_paths: Vec<PathBuf>,
}

#[derive(Clone, Debug)]
pub struct CacheRestoreOptions {
    pub root: PathBuf,
    pub target: Option<PathBuf>,
    pub manifest: Option<PathBuf>,
    pub all: bool,
    pub dry_run: bool,
    pub overwrite: OverwritePolicy,
    pub verify_only: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CacheAuditReport {
    pub root: String,
    pub min_age_seconds: u64,
    pub candidates: Vec<CacheCandidate>,
    pub skipped: Vec<CacheSkip>,
    pub bytes_candidate: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CacheCandidate {
    pub path: String,
    pub size: u64,
    pub mtime_unix: u64,
    pub reason: String,
    pub compressor: Compressor,
    pub tracked_state: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CacheSkip {
    pub path: String,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CacheCompressReport {
    pub root: String,
    pub dry_run: bool,
    pub candidates: Vec<CacheCandidate>,
    pub compressed: Vec<CompressedFile>,
    pub skipped: Vec<CacheSkip>,
    pub failed: Vec<CacheFailure>,
    pub bytes_before: u64,
    pub bytes_after: u64,
    pub bytes_saved: i64,
    pub manifest: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompressedFile {
    pub original_path: String,
    pub compressed_path: String,
    pub original_size: u64,
    pub compressed_size: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CacheFailure {
    pub path: String,
    pub error: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CacheRestoreReport {
    pub root: String,
    pub dry_run: bool,
    pub verify_only: bool,
    pub restored: Vec<RestoredFile>,
    pub verified: Vec<RestoredFile>,
    pub skipped: Vec<CacheSkip>,
    pub failed: Vec<CacheFailure>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RestoredFile {
    pub original_path: String,
    pub compressed_path: String,
    pub sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CacheManifest {
    pub schema_version: u8,
    pub root: String,
    pub created_at_unix: u64,
    pub entries: Vec<ManifestEntry>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ManifestEntry {
    pub original_path: String,
    pub compressed_path: String,
    pub original_size: u64,
    pub compressed_size: u64,
    pub mtime_unix: u64,
    pub mode: Option<u32>,
    pub sha256: String,
    pub compressor: Compressor,
}

pub fn execute(cmd: CacheCommand) -> Result<()> {
    match cmd {
        CacheCommand::Audit(args) => {
            let opts = CacheAuditOptions {
                root: args.root.clone(),
                slot: CacheSlot::parse(&args.slot)?,
                min_age: parse_duration(&args.min_age)?,
                includes: args.include.clone(),
                excludes: args.exclude.clone(),
                active_paths: env_active_paths(),
            };
            let report = audit_cache(opts)?;
            if let Some(path) = args.manifest_out {
                write_json_atomic(&path, &report)?;
            }
            emit(&report, args.format, |report| {
                format!(
                    "cache audit\n  root      : {}\n  candidates: {}\n  skipped   : {}\n  bytes     : {}",
                    report.root,
                    report.candidates.len(),
                    report.skipped.len(),
                    report.bytes_candidate
                )
            })
        }
        CacheCommand::Compress(args) => {
            let root = args.root.clone();
            let slot = CacheSlot::parse(&args.slot)?;
            let manifest = args
                .manifest
                .unwrap_or_else(|| default_manifest_path(&root));
            let lock = args.lock.unwrap_or_else(|| {
                root.join("cache-compression")
                    .join(format!("{}.lock", slot.lock_label()))
            });
            let _guard = CacheLock::acquire(&lock)?;
            let opts = CacheCompressOptions {
                root,
                slot,
                min_age: parse_duration(&args.min_age)?,
                dry_run: args.dry_run,
                compressor: args.compressor,
                level: args.level.clamp(1, 22),
                manifest: manifest.clone(),
                max_files: args.max_files,
                max_bytes: args.max_bytes,
                active_paths: env_active_paths(),
            };
            let report = compress_cache(opts)?;
            emit(&report, args.format, |report| {
                format!(
                    "cache compress\n  root      : {}\n  dry_run   : {}\n  candidates: {}\n  compressed: {}\n  skipped   : {}\n  failed    : {}\n  saved     : {}\n  manifest  : {}",
                    report.root,
                    report.dry_run,
                    report.candidates.len(),
                    report.compressed.len(),
                    report.skipped.len(),
                    report.failed.len(),
                    report.bytes_saved,
                    report.manifest
                )
            })
        }
        CacheCommand::Restore(args) => {
            let opts = CacheRestoreOptions {
                root: args.root,
                target: args.target,
                manifest: args.manifest,
                all: args.all,
                dry_run: args.dry_run,
                overwrite: args.overwrite,
                verify_only: args.verify_only,
            };
            let report = restore_cache(opts)?;
            emit(&report, args.format, |report| {
                format!(
                    "cache restore\n  root       : {}\n  dry_run    : {}\n  verify_only: {}\n  restored   : {}\n  verified   : {}\n  skipped    : {}\n  failed     : {}",
                    report.root,
                    report.dry_run,
                    report.verify_only,
                    report.restored.len(),
                    report.verified.len(),
                    report.skipped.len(),
                    report.failed.len()
                )
            })
        }
    }
}

pub fn audit_cache(opts: CacheAuditOptions) -> Result<CacheAuditReport> {
    let root = normalize_path(&opts.root)?;
    let now = SystemTime::now();
    let mut candidates = Vec::new();
    let mut skipped = Vec::new();
    let active = normalize_active_paths(&opts.active_paths);

    if !root.exists() {
        bail!("cache root does not exist: {}", root.display());
    }

    for entry in WalkDir::new(&root)
        .follow_links(false)
        .into_iter()
        .filter_entry(should_descend)
    {
        let entry = match entry {
            Ok(entry) => entry,
            Err(err) => {
                skipped.push(CacheSkip {
                    path: err
                        .path()
                        .map(|path| display_path(&root, path))
                        .unwrap_or_default(),
                    reason: format!("scan_error:{err}"),
                });
                continue;
            }
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path().to_path_buf();
        let display = display_path(&root, &path);
        if path.extension().is_some_and(|ext| ext == "zst") {
            skipped.push(CacheSkip {
                path: display,
                reason: "already_compressed".into(),
            });
            continue;
        }
        if !opts.slot.matches_path(&path) {
            skipped.push(CacheSkip {
                path: display,
                reason: "slot_mismatch".into(),
            });
            continue;
        }
        if let Some(reason) = deny_reason(&root, &path) {
            skipped.push(CacheSkip {
                path: display,
                reason,
            });
            continue;
        }
        if !is_cache_like(&root, &path, &opts.includes) {
            continue;
        }
        if matches_filters(&display, &opts.excludes) {
            skipped.push(CacheSkip {
                path: display,
                reason: "excluded".into(),
            });
            continue;
        }
        let metadata = fs::metadata(&path).with_context(|| format!("stat {}", path.display()))?;
        let modified = metadata.modified().unwrap_or(UNIX_EPOCH);
        let age = now.duration_since(modified).unwrap_or_default();
        if age < opts.min_age {
            skipped.push(CacheSkip {
                path: display,
                reason: "too_young".into(),
            });
            continue;
        }
        if is_active_path(&path, &active) || is_open_by_process(&path) {
            skipped.push(CacheSkip {
                path: display,
                reason: "active_or_open".into(),
            });
            continue;
        }
        candidates.push(CacheCandidate {
            path: display,
            size: metadata.len(),
            mtime_unix: unix_time(modified),
            reason: cache_reason(&root, &path),
            compressor: Compressor::Zstd,
            tracked_state: git_tracked_state(&root, &path),
        });
    }

    let bytes_candidate = candidates.iter().map(|candidate| candidate.size).sum();
    Ok(CacheAuditReport {
        root: root.display().to_string(),
        min_age_seconds: opts.min_age.as_secs(),
        candidates,
        skipped,
        bytes_candidate,
    })
}

pub fn compress_cache(opts: CacheCompressOptions) -> Result<CacheCompressReport> {
    let root = normalize_path(&opts.root)?;
    let audit = audit_cache(CacheAuditOptions {
        root: root.clone(),
        slot: opts.slot.clone(),
        min_age: opts.min_age,
        includes: Vec::new(),
        excludes: Vec::new(),
        active_paths: opts.active_paths.clone(),
    })?;
    let mut candidates = audit.candidates.clone();
    if let Some(max_files) = opts.max_files {
        candidates.truncate(max_files);
    }
    if let Some(max_bytes) = opts.max_bytes {
        let mut total = 0_u64;
        candidates.retain(|candidate| {
            if total.saturating_add(candidate.size) > max_bytes {
                false
            } else {
                total += candidate.size;
                true
            }
        });
    }

    let mut compressed = Vec::new();
    let mut failed = Vec::new();
    let mut entries = Vec::new();
    let mut bytes_after = 0_u64;
    let bytes_before = candidates
        .iter()
        .map(|candidate| candidate.size)
        .sum::<u64>();

    if !opts.dry_run {
        if let Some(parent) = opts.manifest.parent() {
            fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
        }
    }

    for candidate in &candidates {
        let source = root.join(&candidate.path);
        let dest = zst_path(&source);
        if dest.exists() {
            continue;
        }
        if opts.dry_run {
            continue;
        }
        match compress_one(&root, &source, &dest, opts.compressor, opts.level) {
            Ok(entry) => {
                bytes_after += entry.compressed_size;
                compressed.push(CompressedFile {
                    original_path: entry.original_path.clone(),
                    compressed_path: entry.compressed_path.clone(),
                    original_size: entry.original_size,
                    compressed_size: entry.compressed_size,
                    sha256: entry.sha256.clone(),
                });
                entries.push(entry);
            }
            Err(err) => failed.push(CacheFailure {
                path: candidate.path.clone(),
                error: err.to_string(),
            }),
        }
    }

    if !opts.dry_run && (!entries.is_empty() || !opts.manifest.exists()) {
        let manifest = CacheManifest {
            schema_version: MANIFEST_SCHEMA_VERSION,
            root: root.display().to_string(),
            created_at_unix: unix_time(SystemTime::now()),
            entries,
        };
        write_json_atomic(&opts.manifest, &manifest)?;
    }

    let manifest = opts.manifest.display().to_string();
    Ok(CacheCompressReport {
        root: root.display().to_string(),
        dry_run: opts.dry_run,
        candidates,
        compressed,
        skipped: audit.skipped,
        failed,
        bytes_before,
        bytes_after,
        bytes_saved: bytes_before as i64 - bytes_after as i64,
        manifest,
    })
}

pub fn restore_cache(opts: CacheRestoreOptions) -> Result<CacheRestoreReport> {
    let root = normalize_path(&opts.root)?;
    let mut entries = Vec::new();
    if let Some(manifest_path) = &opts.manifest {
        let manifest: CacheManifest = serde_json::from_reader(
            File::open(manifest_path)
                .with_context(|| format!("open manifest {}", manifest_path.display()))?,
        )?;
        entries.extend(manifest.entries);
    }
    if let Some(target) = &opts.target {
        let target = if target.is_absolute() {
            target.clone()
        } else {
            root.join(target)
        };
        if target.extension().is_some_and(|ext| ext == "zst") {
            let original = strip_zst_suffix(&target)?;
            entries.push(ManifestEntry {
                original_path: display_path(&root, &original),
                compressed_path: display_path(&root, &target),
                original_size: 0,
                compressed_size: fs::metadata(&target).map(|m| m.len()).unwrap_or(0),
                mtime_unix: 0,
                mode: None,
                sha256: String::new(),
                compressor: Compressor::Zstd,
            });
        } else if let Some(found) = entries
            .iter()
            .find(|entry| root.join(&entry.original_path) == target)
            .cloned()
        {
            entries = vec![found];
        } else {
            bail!(
                "target is not a .zst file and was not found in manifest: {}",
                target.display()
            );
        }
    } else if !opts.all {
        bail!("restore requires a target or --all with --manifest");
    }

    if !opts.all && opts.target.is_none() && entries.len() > 1 {
        bail!("manifest has multiple entries; use --all or provide a target");
    }

    let mut restored = Vec::new();
    let mut verified = Vec::new();
    let mut skipped = Vec::new();
    let mut failed = Vec::new();

    for entry in entries {
        let original = root.join(&entry.original_path);
        let compressed = root.join(&entry.compressed_path);
        match restore_one(
            &entry,
            &original,
            &compressed,
            opts.overwrite,
            opts.dry_run,
            opts.verify_only,
        ) {
            Ok(RestoreAction::Restored(file)) => restored.push(file),
            Ok(RestoreAction::Verified(file)) => verified.push(file),
            Ok(RestoreAction::Skipped(reason)) => skipped.push(CacheSkip {
                path: entry.original_path,
                reason,
            }),
            Err(err) => failed.push(CacheFailure {
                path: entry.original_path,
                error: err.to_string(),
            }),
        }
    }

    Ok(CacheRestoreReport {
        root: root.display().to_string(),
        dry_run: opts.dry_run,
        verify_only: opts.verify_only,
        restored,
        verified,
        skipped,
        failed,
    })
}

fn compress_one(
    root: &Path,
    source: &Path,
    dest: &Path,
    compressor: Compressor,
    level: i32,
) -> Result<ManifestEntry> {
    let metadata = fs::metadata(source).with_context(|| format!("stat {}", source.display()))?;
    let sha = sha256_file(source)?;
    let mtime = metadata.modified().unwrap_or(UNIX_EPOCH);
    #[cfg(unix)]
    let mode = Some(metadata.permissions().mode());
    #[cfg(not(unix))]
    let mode = None;
    let tmp = PathBuf::from(format!("{}.tmp", dest.display()));
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    {
        let input = File::open(source).with_context(|| format!("open {}", source.display()))?;
        let output = File::create(&tmp).with_context(|| format!("create {}", tmp.display()))?;
        match compressor {
            Compressor::Zstd => {}
        }
        let mut encoder = zstd::stream::write::Encoder::new(output, level.clamp(1, 22))?;
        let mut reader = io::BufReader::new(input);
        io::copy(&mut reader, &mut encoder)?;
        let output = encoder.finish()?;
        output.sync_all()?;
    }
    let verified_sha = sha256_zstd(&tmp)?;
    if verified_sha != sha {
        let _ = fs::remove_file(&tmp);
        bail!("compressed verification failed for {}", source.display());
    }
    fs::rename(&tmp, dest)
        .with_context(|| format!("rename {} to {}", tmp.display(), dest.display()))?;
    let compressed_size = fs::metadata(dest)?.len();
    fs::remove_file(source).with_context(|| format!("remove original {}", source.display()))?;

    Ok(ManifestEntry {
        original_path: display_path(root, source),
        compressed_path: display_path(root, dest),
        original_size: metadata.len(),
        compressed_size,
        mtime_unix: unix_time(mtime),
        mode,
        sha256: sha,
        compressor,
    })
}

enum RestoreAction {
    Restored(RestoredFile),
    Verified(RestoredFile),
    Skipped(String),
}

fn restore_one(
    entry: &ManifestEntry,
    original: &Path,
    compressed: &Path,
    overwrite: OverwritePolicy,
    dry_run: bool,
    verify_only: bool,
) -> Result<RestoreAction> {
    if !compressed.exists() {
        bail!("compressed file missing: {}", compressed.display());
    }
    let actual_sha = sha256_zstd(compressed)?;
    if !entry.sha256.is_empty() && actual_sha != entry.sha256 {
        bail!("checksum mismatch for {}", compressed.display());
    }
    let restored = RestoredFile {
        original_path: entry.original_path.clone(),
        compressed_path: entry.compressed_path.clone(),
        sha256: actual_sha,
    };
    if verify_only {
        return Ok(RestoreAction::Verified(restored));
    }
    if original.exists() {
        match overwrite {
            OverwritePolicy::Never | OverwritePolicy::IfMissing => {
                return Ok(RestoreAction::Skipped("target_exists".into()))
            }
            OverwritePolicy::Always => {}
        }
    }
    if dry_run {
        return Ok(RestoreAction::Verified(restored));
    }
    if let Some(parent) = original.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = PathBuf::from(format!("{}.restore.tmp", original.display()));
    {
        let input = File::open(compressed)?;
        let mut decoder = zstd::stream::read::Decoder::new(input)?;
        let mut output = File::create(&tmp)?;
        io::copy(&mut decoder, &mut output)?;
        output.sync_all()?;
    }
    let restored_sha = sha256_file(&tmp)?;
    if !entry.sha256.is_empty() && restored_sha != entry.sha256 {
        let _ = fs::remove_file(&tmp);
        bail!("restored checksum mismatch for {}", original.display());
    }
    fs::rename(&tmp, original)?;
    if let Some(mode) = entry.mode {
        #[cfg(unix)]
        fs::set_permissions(original, fs::Permissions::from_mode(mode))?;
    }
    if entry.mtime_unix > 0 {
        set_file_mtime(
            original,
            FileTime::from_unix_time(entry.mtime_unix as i64, 0),
        )?;
    }
    Ok(RestoreAction::Restored(restored))
}

struct CacheLock {
    path: PathBuf,
}

impl CacheLock {
    fn acquire(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        match OpenOptions::new().write(true).create_new(true).open(path) {
            Ok(mut file) => {
                writeln!(file, "pid={}", std::process::id())?;
                Ok(Self {
                    path: path.to_path_buf(),
                })
            }
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {
                bail!("cache compression lock already exists: {}", path.display())
            }
            Err(err) => Err(err).with_context(|| format!("create lock {}", path.display())),
        }
    }
}

impl Drop for CacheLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn should_descend(entry: &DirEntry) -> bool {
    if !entry.file_type().is_dir() {
        return true;
    }
    let name = entry.file_name().to_string_lossy();
    !matches!(
        name.as_ref(),
        ".git"
            | "target"
            | "store"
            | "toolchains"
            | "downloads"
            | "tmp"
            | "_actions"
            | "_temp"
            | "_tool"
            | "externals"
    )
}

fn deny_reason(root: &Path, path: &Path) -> Option<String> {
    let rel = display_path(root, path);
    let components = path_components(Path::new(&rel));
    if components.iter().any(|part| part == ".git") {
        return Some("denylist_git".into());
    }
    if components.iter().any(|part| part == "archives") {
        return Some("denylist_archives".into());
    }
    let file_name = path.file_name()?.to_string_lossy();
    if matches!(
        file_name.as_ref(),
        ".runner" | ".credentials" | ".credentials_rsaparams" | ".env"
    ) {
        return Some("denylist_runner_identity_or_credentials".into());
    }
    let lower = rel.to_ascii_lowercase();
    for token in [
        "credential",
        "secret",
        "token",
        "service",
        "locationservicedata.config",
    ] {
        if lower.contains(token) {
            return Some(format!("denylist_{token}"));
        }
    }
    None
}

fn is_cache_like(root: &Path, path: &Path, includes: &[String]) -> bool {
    let rel = display_path(root, path);
    if matches_filters(&rel, includes) {
        return true;
    }
    rel.contains("/.cache/kache/")
        || rel.contains("/.cache/envctl/")
        || rel.ends_with("/.cargo/.global-cache")
        || rel.ends_with("/.local/share/rtk/history.db")
        || rel.ends_with(".log")
        || rel.contains("/_diag/")
}

fn cache_reason(root: &Path, path: &Path) -> String {
    let rel = display_path(root, path);
    if rel.contains("/.cache/kache/") {
        "kache_cache".into()
    } else if rel.contains("/.cache/envctl/") {
        "envctl_cache".into()
    } else if rel.contains("/.cargo/") {
        "cargo_cache".into()
    } else if rel.contains("/.local/share/rtk/") {
        "rtk_cache".into()
    } else {
        "cache_allowlist".into()
    }
}

fn matches_filters(rel: &str, filters: &[String]) -> bool {
    filters.iter().any(|filter| {
        rel.contains(filter)
            || glob_match(filter, rel)
            || Path::new(rel)
                .file_name()
                .is_some_and(|name| name.to_string_lossy().contains(filter))
    })
}

fn glob_match(pattern: &str, rel: &str) -> bool {
    if !pattern.contains('*') {
        return false;
    }
    let mut remaining = rel;
    for part in pattern.split('*').filter(|part| !part.is_empty()) {
        if let Some(index) = remaining.find(part) {
            remaining = &remaining[index + part.len()..];
        } else {
            return false;
        }
    }
    true
}

fn normalize_active_paths(paths: &[PathBuf]) -> BTreeSet<PathBuf> {
    paths
        .iter()
        .filter_map(|path| normalize_path(path).ok())
        .collect()
}

fn is_active_path(path: &Path, active: &BTreeSet<PathBuf>) -> bool {
    let normalized = normalize_path(path).unwrap_or_else(|_| path.to_path_buf());
    active.iter().any(|active_path| {
        normalized.starts_with(active_path) || active_path.starts_with(&normalized)
    })
}

fn is_open_by_process(path: &Path) -> bool {
    if cfg!(windows) || std::env::var_os("FXRUN_CACHE_SKIP_OPEN_FILE_CHECK").is_some() {
        return false;
    }
    let Some(lsof) = command_path("lsof") else {
        return false;
    };
    Command::new(lsof)
        .arg("--")
        .arg(path)
        .output()
        .map(|output| output.status.success() && !output.stdout.is_empty())
        .unwrap_or(false)
}

fn env_active_paths() -> Vec<PathBuf> {
    std::env::var_os("FXRUN_CACHE_ACTIVE_PATHS")
        .map(|value| std::env::split_paths(&value).collect())
        .unwrap_or_default()
}

fn command_path(name: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&paths) {
        let candidate = dir.join(name);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

fn git_tracked_state(root: &Path, path: &Path) -> String {
    let repo = root.parent().unwrap_or(root);
    let rel = path.strip_prefix(repo).unwrap_or(path);
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .arg("ls-files")
        .arg("--error-unmatch")
        .arg(rel)
        .output();
    match output {
        Ok(output) if output.status.success() => "tracked".into(),
        _ => "untracked_or_ignored".into(),
    }
}

fn parse_duration(raw: &str) -> Result<Duration> {
    if raw.is_empty() {
        bail!("duration cannot be empty");
    }
    let (number, multiplier) = match raw.chars().last().unwrap() {
        'd' => (&raw[..raw.len() - 1], 24 * 60 * 60),
        'h' => (&raw[..raw.len() - 1], 60 * 60),
        'm' => (&raw[..raw.len() - 1], 60),
        's' => (&raw[..raw.len() - 1], 1),
        c if c.is_ascii_digit() => (raw, 1),
        other => bail!("unsupported duration suffix '{other}' in {raw}"),
    };
    let value: u64 = number
        .parse()
        .with_context(|| format!("parse duration {raw}"))?;
    Ok(Duration::from_secs(value.saturating_mul(multiplier)))
}

fn default_manifest_path(root: &Path) -> PathBuf {
    root.join(DEFAULT_MANIFEST_DIR).join(format!(
        "cache-compression-{}.json",
        unix_time(SystemTime::now())
    ))
}

fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = PathBuf::from(format!("{}.tmp", path.display()));
    {
        let mut file = File::create(&tmp)?;
        serde_json::to_writer_pretty(&mut file, value)?;
        writeln!(file)?;
        file.sync_all()?;
    }
    fs::rename(tmp, path)?;
    Ok(())
}

fn emit<T, F>(value: &T, format: OutputFormat, human: F) -> Result<()>
where
    T: Serialize,
    F: FnOnce(&T) -> String,
{
    match format {
        OutputFormat::Human => println!("{}", human(value)),
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(value)?),
    }
    Ok(())
}

fn normalize_path(path: &Path) -> Result<PathBuf> {
    if path.exists() {
        fs::canonicalize(path).with_context(|| format!("canonicalize {}", path.display()))
    } else if let Some(parent) = path.parent().filter(|parent| parent.exists()) {
        Ok(fs::canonicalize(parent)?.join(path.file_name().ok_or_else(|| anyhow!("bad path"))?))
    } else {
        Ok(path.to_path_buf())
    }
}

fn display_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn path_components(path: &Path) -> Vec<String> {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy().to_string())
        .collect()
}

fn unix_time(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn sha256_zstd(path: &Path) -> Result<String> {
    let file = File::open(path)?;
    let mut decoder = zstd::stream::read::Decoder::new(file)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = decoder.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn zst_path(path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.zst", path.display()))
}

fn strip_zst_suffix(path: &Path) -> Result<PathBuf> {
    let raw = path.to_string_lossy();
    raw.strip_suffix(".zst")
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("path does not end with .zst: {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{Duration, SystemTime};

    #[test]
    fn cache_compression_audit_never_selects_denied_or_live_runner_files() {
        let root = test_root("cache-deny");
        let runner = root.join("_work/runner-home-01");
        fs::create_dir_all(runner.join(".cache/kache")).unwrap();
        fs::write(runner.join(".runner"), b"runner identity").unwrap();
        fs::write(runner.join(".credentials"), b"token").unwrap();
        fs::write(runner.join(".cache/kache/events.jsonl"), b"old cache").unwrap();
        make_old(&runner.join(".cache/kache/events.jsonl"));

        let report = audit_cache(CacheAuditOptions {
            root: root.join("_work"),
            slot: CacheSlot::All,
            min_age: Duration::from_secs(60),
            includes: Vec::new(),
            excludes: Vec::new(),
            active_paths: vec![runner.join(".cache/kache/events.jsonl")],
        })
        .unwrap();

        assert!(
            report.candidates.is_empty(),
            "active cache file must be skipped, not selected"
        );
        assert!(report
            .skipped
            .iter()
            .any(|skip| skip.path.ends_with("runner-home-01/.runner")));
        assert!(report
            .skipped
            .iter()
            .any(|skip| skip.path.ends_with("runner-home-01/.credentials")));
        assert!(report.skipped.iter().any(|skip| {
            skip.path
                .ends_with("runner-home-01/.cache/kache/events.jsonl")
                && skip.reason.contains("active")
        }));
    }

    #[test]
    fn cache_compression_round_trips_old_cache_files_with_manifest_metadata() {
        let root = test_root("cache-roundtrip");
        let cache = root.join("_work/runner-home-01/.cache/kache/events.jsonl");
        fs::create_dir_all(cache.parent().unwrap()).unwrap();
        fs::write(&cache, b"cold-cache-line\ncold-cache-line\n").unwrap();
        make_old(&cache);
        let before = fs::metadata(&cache).unwrap();

        let manifest = root.join("_work/cache-compression/manifests/test.json");
        let report = compress_cache(CacheCompressOptions {
            root: root.join("_work"),
            slot: CacheSlot::All,
            min_age: Duration::from_secs(60),
            dry_run: false,
            compressor: Compressor::Zstd,
            level: 3,
            manifest: manifest.clone(),
            max_files: Some(10),
            max_bytes: None,
            active_paths: Vec::new(),
        })
        .unwrap();

        assert_eq!(report.compressed.len(), 1);
        assert!(
            !cache.exists(),
            "original must be removed after verified compression"
        );
        assert!(cache.with_extension("jsonl.zst").exists());
        assert!(manifest.exists());

        let second = compress_cache(CacheCompressOptions {
            root: root.join("_work"),
            slot: CacheSlot::All,
            min_age: Duration::from_secs(60),
            dry_run: false,
            compressor: Compressor::Zstd,
            level: 3,
            manifest: manifest.clone(),
            max_files: Some(10),
            max_bytes: None,
            active_paths: Vec::new(),
        })
        .unwrap();
        assert!(
            second.compressed.is_empty(),
            "compression must be idempotent"
        );

        let restore = restore_cache(CacheRestoreOptions {
            root: root.join("_work"),
            target: None,
            manifest: Some(manifest),
            all: true,
            dry_run: false,
            overwrite: OverwritePolicy::IfMissing,
            verify_only: false,
        })
        .unwrap();
        assert_eq!(restore.restored.len(), 1);
        assert_eq!(
            fs::read(&cache).unwrap(),
            b"cold-cache-line\ncold-cache-line\n"
        );
        let after = fs::metadata(&cache).unwrap();
        assert_eq!(after.len(), before.len());
    }

    #[test]
    fn cache_compression_dry_run_reports_savings_without_mutating() {
        let root = test_root("cache-dry-run");
        let cache = root.join("_work/runner-home-02/.cache/envctl/state.json");
        fs::create_dir_all(cache.parent().unwrap()).unwrap();
        fs::write(&cache, br#"{"state":"cold"}"#).unwrap();
        make_old(&cache);

        let report = compress_cache(CacheCompressOptions {
            root: root.join("_work"),
            slot: CacheSlot::Slot("02".to_string()),
            min_age: Duration::from_secs(60),
            dry_run: true,
            compressor: Compressor::Zstd,
            level: 3,
            manifest: root.join("_work/cache-compression/manifests/dry.json"),
            max_files: None,
            max_bytes: None,
            active_paths: Vec::new(),
        })
        .unwrap();

        assert_eq!(report.candidates.len(), 1);
        assert!(cache.exists());
        assert!(!cache.with_extension("json.zst").exists());
    }

    #[test]
    fn cache_restore_never_overwrites_when_policy_blocks_it() {
        let root = test_root("cache-overwrite");
        let cache = root.join("_work/runner-home-01/.cache/kache/index.db");
        fs::create_dir_all(cache.parent().unwrap()).unwrap();
        fs::write(&cache, b"old-index").unwrap();
        make_old(&cache);
        let manifest = root.join("_work/cache-compression/manifests/test.json");
        compress_cache(CacheCompressOptions {
            root: root.join("_work"),
            slot: CacheSlot::All,
            min_age: Duration::from_secs(60),
            dry_run: false,
            compressor: Compressor::Zstd,
            level: 3,
            manifest: manifest.clone(),
            max_files: None,
            max_bytes: None,
            active_paths: Vec::new(),
        })
        .unwrap();
        fs::write(&cache, b"newer-live-cache").unwrap();
        let restore = restore_cache(CacheRestoreOptions {
            root: root.join("_work"),
            target: None,
            manifest: Some(manifest),
            all: true,
            dry_run: false,
            overwrite: OverwritePolicy::Never,
            verify_only: false,
        })
        .unwrap();
        assert_eq!(restore.skipped.len(), 1);
        assert_eq!(fs::read(&cache).unwrap(), b"newer-live-cache");
    }

    #[test]
    fn cache_command_automation_seams_are_checked_in() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root");
        let docs = fs::read_to_string(root.join("docs/cache-compression.md")).unwrap();
        let script = fs::read_to_string(root.join("scripts/cache-maintenance.sh")).unwrap();
        let workflow =
            fs::read_to_string(root.join(".github/workflows/cache-maintenance.yml")).unwrap();
        let systemd_service =
            fs::read_to_string(root.join("systemd/user/flexnetos-cache-maintenance.service"))
                .unwrap();
        let systemd_timer =
            fs::read_to_string(root.join("systemd/user/flexnetos-cache-maintenance.timer"))
                .unwrap();

        for required in [
            "fxrun cache audit",
            "fxrun cache compress",
            "fxrun cache restore",
            "FXRUN_CACHE_ACTIVE_PATHS",
            "not a gitignore feature",
            "Archives under `_work/archives` are out of scope by default",
            "systemd/user/flexnetos-cache-maintenance.{service,timer}",
        ] {
            assert!(docs.contains(required), "cache docs missing {required}");
        }
        for required in [
            "FXRUN_CACHE_MAINTENANCE_MODE",
            "audit|compress|restore",
            "--dry-run",
            "FXRUN_CACHE_RESTORE_MANIFEST",
        ] {
            assert!(
                script.contains(required),
                "cache wrapper missing {required}"
            );
        }
        for required in [
            "workflow_dispatch:",
            "mode:",
            "slot:",
            "min_age:",
            "dry_run:",
            "scripts/cache-maintenance.sh",
            "cache-maintenance-report.json",
        ] {
            assert!(
                workflow.contains(required),
                "cache workflow missing {required}"
            );
        }
        for required in [
            "@FXRUN_PREFIX@",
            "ExecCondition",
            "FXRUN_CACHE_MAINTENANCE_MODE=audit",
            "FXRUN_CACHE_DRY_RUN=1",
            "cache audit",
        ] {
            assert!(
                systemd_service.contains(required),
                "cache service missing {required}"
            );
        }
        for required in ["OnCalendar=daily", "RandomizedDelaySec", "Persistent=true"] {
            assert!(
                systemd_timer.contains(required),
                "cache timer missing {required}"
            );
        }
    }

    fn test_root(name: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("fxrun-{name}-{nanos}"));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn make_old(path: &std::path::Path) {
        let old =
            filetime::FileTime::from_system_time(SystemTime::now() - Duration::from_secs(3600));
        filetime::set_file_mtime(path, old).unwrap();
    }
}
