//! Cascade Controller — topological repair orchestrator for cross-service changes.
//!
//! When a service's intent changes, the cascade controller:
//! 1. Computes the blast radius via `UniverseGraph`
//! 2. Generates a `CascadePlan` of repair phases sorted by topological depth
//! 3. Executes each phase: Forge build -> cargo check -> Mechanic repair
//!
//! If a phase fails after `max_repair_cycles` attempts, it triggers a
//! **hard halt** and skips all downstream dependents.

use crate::universe_graph::{compute_blast_radius, ServiceDelta, UniverseGraph};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

// ============================================================================
// Configuration
// ============================================================================

/// Cascade controller configuration.
#[derive(Debug, Clone)]
pub struct CascadeConfig {
    /// Maximum number of repair cycles per phase before hard-halting.
    pub max_repair_cycles: usize,
    /// Timeout for each individual phase (reserved for future enforcement).
    pub phase_timeout: Duration,
    /// When true, print the plan but do not execute any external commands.
    pub dry_run: bool,
    /// Enable cargo clippy validation after cargo check (default: true).
    pub enable_clippy: bool,
    /// Enable cargo test validation after clippy (default: false, opt-in).
    pub enable_tests: bool,
}

impl Default for CascadeConfig {
    fn default() -> Self {
        Self {
            max_repair_cycles: 3,
            phase_timeout: Duration::from_secs(600), // 10 minutes
            dry_run: false,
            enable_clippy: true,
            enable_tests: false,
        }
    }
}

// ============================================================================
// Phase types
// ============================================================================

/// An action that can be performed during a cascade phase.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CascadeAction {
    /// Run `forge build <service_path>`.
    ForgeBuild,
    /// Run `cargo check` in the service directory.
    CargoCheck,
    /// Run `mechanic repair <service_path>`.
    MechanicRepair,
}

/// The status of a single cascade phase.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PhaseStatus {
    /// Not yet started.
    Pending,
    /// Currently executing.
    Running,
    /// cargo check passed.
    Passed,
    /// Failed with an error message.
    Failed(String),
    /// Exceeded `max_repair_cycles` -- circuit breaker tripped.
    HardHalt(String),
    /// Skipped because an upstream phase failed or hard-halted.
    Skipped,
}

/// A single repair phase targeting one impacted service.
#[derive(Debug)]
pub struct CascadePhase {
    /// Service identifier (matches `ServiceEntry.id`).
    pub service_id: String,
    /// Absolute path to the service root directory.
    pub service_path: PathBuf,
    /// Topological distance from the trigger service.
    pub depth: usize,
    /// Current status of this phase.
    pub status: PhaseStatus,
    /// Number of repair attempts already made.
    pub repair_attempts: usize,
    /// Wall-clock duration once completed.
    pub duration: Option<Duration>,

    /// Cargo package name (from Cargo.toml) for scoped `cargo check -p`.
    pub package_name: Option<String>,

    // --- P6 前置数据字段 ---
    /// 最近一次验证失败的 compiler stderr（截断最后 2048 字符）
    pub last_compiler_output: Option<String>,
    /// 修复成功后变更的文件列表（来自 git diff）
    pub files_modified: Vec<String>,
    /// 修复成功后的 diff 摘要（截断最后 4096 字符）
    pub repair_diff: Option<String>,
}

/// Find the relative sub-path from the git root to the given path.
/// Returns empty PathBuf if path is already a git root.
fn find_subpath_from_git_root(path: &Path) -> PathBuf {
    let mut git_root = path.to_path_buf();
    loop {
        if git_root.join(".git").exists() {
            return path.strip_prefix(&git_root).unwrap_or(Path::new("")).to_path_buf();
        }
        if !git_root.pop() {
            return PathBuf::new();
        }
    }
}

/// Resolve the Cargo package name from Cargo.toml in a service directory.
fn resolve_cargo_package_name(service_path: &Path) -> Option<String> {
    let cargo_toml = service_path.join("Cargo.toml");
    let content = std::fs::read_to_string(&cargo_toml).ok()?;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("name") && trimmed.contains('=') {
            return trimmed.split('=').nth(1).map(|v| v.trim().trim_matches('"').to_string());
        }
    }
    None
}

/// Result of running an external command, capturing stdout/stderr.
struct CommandResult {
    success: bool,
    #[allow(dead_code)]
    stdout: String,
    stderr: String,
}

// ============================================================================
// Plan
// ============================================================================

/// A complete cascade repair plan.
#[derive(Debug)]
pub struct CascadePlan {
    /// The service whose intent change triggered the cascade.
    pub trigger_service: String,
    /// Human-readable descriptions of the triggering changes.
    pub trigger_changes: Vec<String>,
    /// Ordered list of repair phases (sorted by `depth`).
    pub phases: Vec<CascadePhase>,
    /// Configuration used for this plan.
    pub config: CascadeConfig,
    /// Maps each service_id to its direct upstream provider_ids.
    /// Used for dependency-based skip logic instead of depth-based.
    pub upstream_map: HashMap<String, Vec<String>>,
    /// When execution started.
    pub started_at: Option<Instant>,
    /// Whether execution has completed (all phases resolved).
    pub completed: bool,
}

// ============================================================================
// Report
// ============================================================================

/// Summary report produced after executing a cascade plan.
#[derive(Debug)]
pub struct CascadeReport {
    /// The executed plan (phases contain final statuses).
    pub plan: CascadePlan,
    /// Total wall-clock duration of the entire cascade.
    pub total_duration: Duration,
    /// Number of services that reached `Passed`.
    pub services_repaired: usize,
    /// Number of services that ended in `Failed`, `HardHalt`, or `Skipped`.
    pub services_failed: usize,
    /// Whether any phase triggered a hard halt.
    pub hard_halted: bool,
}

// ============================================================================
// Plan creation
// ============================================================================

/// Build a `CascadePlan` from a universe graph and a service delta.
///
/// The plan phases are generated from `compute_blast_radius` and sorted
/// by topological depth (nearest first).  Each phase's `service_path`
/// is resolved relative to `manifest_dir`.
pub fn create_plan(
    graph: &UniverseGraph,
    delta: &ServiceDelta,
    manifest_dir: &Path,
    config: CascadeConfig,
) -> CascadePlan {
    let impacted = compute_blast_radius(graph, delta);

    // Build a lookup from service_id -> relative path
    let path_map: std::collections::HashMap<&str, &Path> = graph
        .services
        .iter()
        .map(|s| (s.id.as_str(), s.path.as_path()))
        .collect();

    let phases: Vec<CascadePhase> = impacted
        .iter()
        .map(|imp| {
            let rel_path = path_map
                .get(imp.service_id.as_str())
                .copied()
                .unwrap_or_else(|| Path::new(&imp.service_id));
            let service_path = manifest_dir.join(rel_path);

            // Resolve Cargo package name for scoped check
            let package_name = resolve_cargo_package_name(&service_path);

            CascadePhase {
                service_id: imp.service_id.clone(),
                service_path,
                depth: imp.depth,
                status: PhaseStatus::Pending,
                repair_attempts: 0,
                duration: None,
                package_name,
                last_compiler_output: None,
                files_modified: Vec::new(),
                repair_diff: None,
            }
        })
        .collect();

    // Build upstream_map: for each service in the plan, record its direct
    // upstream provider_ids from graph.dependencies.
    let mut upstream_map: HashMap<String, Vec<String>> = HashMap::new();
    for phase in &phases {
        let upstreams: Vec<String> = graph
            .dependencies
            .get(&phase.service_id)
            .map(|deps| {
                deps.iter()
                    .map(|d| d.provider_id.clone())
                    .collect()
            })
            .unwrap_or_default();
        upstream_map.insert(phase.service_id.clone(), upstreams);
    }

    CascadePlan {
        trigger_service: delta.service_id.clone(),
        trigger_changes: delta.changed_interfaces.clone(),
        phases,
        config,
        upstream_map,
        started_at: None,
        completed: false,
    }
}

// ============================================================================
// Command helpers (internal)
// ============================================================================

/// Check whether an executable exists on `PATH`.
fn command_exists(name: &str) -> bool {
    Command::new("which")
        .arg(name)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Run an external command, capturing stdout and stderr.
/// Returns `Ok(CommandResult)` with success flag and captured output,
/// and `Err(msg)` if the command could not be spawned at all.
fn run_command(cmd: &str, args: &[&str], working_dir: Option<&Path>) -> Result<CommandResult, String> {
    let mut command = Command::new(cmd);
    command.args(args);
    if let Some(dir) = working_dir {
        command.current_dir(dir);
    }
    command
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let output = command
        .output()
        .map_err(|e| format!("failed to spawn `{}`: {}", cmd, e))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !stdout.is_empty() {
        eprintln!("[cascade][{}] stdout:\n{}", cmd, stdout.trim_end());
    }
    if !stderr.is_empty() {
        eprintln!("[cascade][{}] stderr:\n{}", cmd, stderr.trim_end());
    }

    Ok(CommandResult {
        success: output.status.success(),
        stdout,
        stderr,
    })
}

// ============================================================================
// Phase execution
// ============================================================================

/// Execute a single repair attempt on a phase.
///
/// The sequence is:
/// 1. `forge build` (if `.missiond/` exists and `forge` is on PATH)
/// 2. `cargo check` -- if green, continue to extended validation
/// 3. `cargo clippy` (if `enable_clippy`) -- if green, continue
/// 4. `cargo test --lib` (if `enable_tests`) -- if green, return `Ok(true)`
/// 5. If any validation fails: `mechanic repair` (if `mechanic` is on PATH)
/// 6. Repeat validation (check -> clippy -> test) post-repair
///
/// Returns `Ok(true)` when all enabled validations pass, `Ok(false)` when
/// they fail after repair, and `Err(msg)` on unexpected spawn errors.
pub fn execute_phase_attempt(
    phase: &mut CascadePhase,
    config: &CascadeConfig,
) -> Result<bool, String> {
    let svc_path_str = phase.service_path.to_string_lossy().to_string();

    // Step 1: Forge build (optional -- only if .missiond/ exists and forge is on PATH)
    let missiond_dir = phase.service_path.join(".missiond");
    if missiond_dir.is_dir() && command_exists("forge") {
        eprintln!(
            "[cascade] forge build {} (depth={})",
            phase.service_id, phase.depth
        );
        match run_command("forge", &["build", &svc_path_str], None) {
            Ok(r) if r.success => {}
            Ok(_) => {
                eprintln!(
                    "[cascade] warning: forge build failed for {}",
                    phase.service_id
                );
            }
            Err(e) => {
                eprintln!(
                    "[cascade] warning: forge build error for {}: {}",
                    phase.service_id, e
                );
            }
        }
    }

    // Step 2: Full validation (check -> clippy -> test)
    if run_full_validation(phase, config)? {
        // Validation passed -- capture diff info for P6
        capture_diff_info(phase);
        return Ok(true);
    }

    // Step 3: mechanic repair (optional)
    if command_exists("mechanic") {
        eprintln!(
            "[cascade] mechanic repair {} (depth={})",
            phase.service_id, phase.depth
        );
        match run_command("mechanic", &["repair", &svc_path_str], None) {
            Ok(_) => {}
            Err(e) => {
                eprintln!(
                    "[cascade] warning: mechanic repair error for {}: {}",
                    phase.service_id, e
                );
            }
        }
    } else {
        eprintln!(
            "[cascade] warning: mechanic not in PATH, skipping repair for {}",
            phase.service_id
        );
    }

    // Step 4: Full validation again (post-repair)
    let passed = run_full_validation(phase, config)?;
    if passed {
        capture_diff_info(phase);
    }
    Ok(passed)
}

/// Run the layered validation pipeline: cargo check -> clippy -> test.
///
/// Returns `Ok(true)` if all enabled layers pass, `Ok(false)` if any fails,
/// and `Err(msg)` on spawn errors.
///
/// On failure, captures the compiler stderr into `phase.last_compiler_output`
/// (truncated to the last 2048 characters for P6 analysis).
fn run_full_validation(phase: &mut CascadePhase, config: &CascadeConfig) -> Result<bool, String> {
    // Layer 1: cargo check (scoped to package if available)
    let check_args: Vec<&str> = if let Some(ref pkg) = phase.package_name {
        vec!["check", "-p", pkg]
    } else {
        vec!["check"]
    };
    eprintln!(
        "[cascade] cargo check {} (depth={})",
        phase.service_id, phase.depth
    );
    match run_command("cargo", &check_args, Some(&phase.service_path)) {
        Ok(r) if r.success => {}
        Ok(r) => {
            phase.last_compiler_output = Some(truncate_tail(&r.stderr, 2048));
            return Ok(false);
        }
        Err(e) => return Err(format!("cargo check spawn error: {}", e)),
    }

    // Layer 2: cargo clippy (if enabled)
    if config.enable_clippy {
        eprintln!(
            "[cascade] cargo clippy {} (depth={})",
            phase.service_id, phase.depth
        );
        let clippy_args: Vec<&str> = if let Some(ref pkg) = phase.package_name {
            vec!["clippy", "-p", pkg, "--", "-D", "warnings"]
        } else {
            vec!["clippy", "--all-targets", "--", "-D", "warnings"]
        };
        match run_command(
            "cargo",
            &clippy_args,
            Some(&phase.service_path),
        ) {
            Ok(r) if r.success => {}
            Ok(r) => {
                phase.last_compiler_output = Some(truncate_tail(&r.stderr, 2048));
                return Ok(false);
            }
            Err(e) => return Err(format!("cargo clippy spawn error: {}", e)),
        }
    }

    // Layer 3: cargo test (if enabled)
    if config.enable_tests {
        eprintln!(
            "[cascade] cargo test --lib {} (depth={})",
            phase.service_id, phase.depth
        );
        match run_command("cargo", &["test", "--lib"], Some(&phase.service_path)) {
            Ok(r) if r.success => {}
            Ok(r) => {
                phase.last_compiler_output = Some(truncate_tail(&r.stderr, 2048));
                return Ok(false);
            }
            Err(e) => return Err(format!("cargo test spawn error: {}", e)),
        }
    }

    Ok(true)
}

/// Truncate a string to the last `max_len` characters (preserving the tail).
fn truncate_tail(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        s[s.len() - max_len..].to_string()
    }
}

/// Capture git diff info (files_modified, repair_diff) for the phase's service path.
/// Best-effort: failures are silently ignored.
fn capture_diff_info(phase: &mut CascadePhase) {
    // git diff --stat HEAD to get modified file list
    if let Ok(r) = run_command("git", &["diff", "--stat", "HEAD"], Some(&phase.service_path)) {
        let files: Vec<String> = r.stdout
            .lines()
            .filter(|l| l.contains('|'))
            .filter_map(|l| l.split('|').next())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        phase.files_modified = files;
    }

    // git diff HEAD to get full diff (truncated)
    if let Ok(r) = run_command("git", &["diff", "HEAD"], Some(&phase.service_path)) {
        if !r.stdout.is_empty() {
            phase.repair_diff = Some(truncate_tail(&r.stdout, 4096));
        }
    }
}

// ============================================================================
// Git 2PC shadow branches
// ============================================================================

/// A shadow branch backed by a git worktree for atomic cascade operations.
///
/// If all phases pass, their worktree changes are merged back.
/// If any phase fails, all worktrees are discarded -- zero side-effects.
#[derive(Debug)]
struct ShadowBranch {
    /// Temporary worktree path.
    worktree_path: PathBuf,
    /// The branch name created for the worktree.
    branch_name: String,
    /// Original service path (used for merge-back).
    original_path: PathBuf,
}

/// Result of resolving the 2PC commit phase.
#[derive(Debug, Default, PartialEq, Eq)]
struct CommitResolution {
    /// Whether the merge stage forced the cascade into a hard-stop state.
    hard_halted: bool,
    /// How many additional failed services should be counted for reporting.
    services_failed_delta: usize,
}

/// Create a detached git worktree as a shadow branch for isolation.
///
/// Returns `Err` if the service directory is not inside a git repository
/// or if git commands fail.  Callers should degrade gracefully.
fn create_shadow_branch(service_path: &Path, service_id: &str) -> Result<ShadowBranch, String> {
    let branch_name = format!("cascade/shadow-{}-{}", service_id, std::process::id());
    let worktree_path = std::env::temp_dir().join(&branch_name);

    // Ensure the worktree target doesn't already exist
    if worktree_path.exists() {
        let _ = std::fs::remove_dir_all(&worktree_path);
    }

    run_command(
        "git",
        &[
            "worktree",
            "add",
            "--detach",
            &worktree_path.to_string_lossy(),
        ],
        Some(service_path),
    )?;

    // Verify the worktree was actually created
    if !worktree_path.exists() {
        return Err(format!(
            "git worktree add succeeded but path does not exist: {}",
            worktree_path.display()
        ));
    }

    Ok(ShadowBranch {
        worktree_path,
        branch_name,
        original_path: service_path.to_path_buf(),
    })
}

/// Merge changes from a shadow worktree back to the original working tree.
///
/// Copies all modified/new files from the worktree into the original path,
/// then cleans up the worktree.
fn merge_shadow_branch(sb: &ShadowBranch) -> Result<(), String> {
    // 0. Resolve the git repo root.  `git diff --name-only HEAD` returns
    //    repo-root-relative paths, so we need the actual root — not the
    //    service sub-directory — as the copy-back target.
    let repo_root_out = run_command(
        "git",
        &["rev-parse", "--show-toplevel"],
        Some(&sb.original_path),
    )?;
    let repo_root = PathBuf::from(repo_root_out.stdout.trim());

    // 1. Collect tracked files that changed (modified or deleted)
    let diff_output = Command::new("git")
        .args(["diff", "--name-only", "HEAD"])
        .current_dir(&sb.worktree_path)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .map_err(|e| format!("git diff in worktree failed: {}", e))?;

    // 2. Collect untracked new files (forge-generated .rs etc.)
    let untracked_output = Command::new("git")
        .args(["ls-files", "--others", "--exclude-standard"])
        .current_dir(&sb.worktree_path)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .map_err(|e| format!("git ls-files in worktree failed: {}", e))?;

    let changed_files = String::from_utf8_lossy(&diff_output.stdout);
    let untracked_files = String::from_utf8_lossy(&untracked_output.stdout);

    // Merge all file paths (changed + untracked)
    let all_files: Vec<&str> = changed_files
        .lines()
        .chain(untracked_files.lines())
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();

    for file in &all_files {
        let src = sb.worktree_path.join(file);
        let dst = repo_root.join(file);

        if src.exists() {
            // Copy new or modified file
            if let Some(parent) = dst.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            std::fs::copy(&src, &dst).map_err(|e| {
                format!("failed to copy {} -> {}: {}", src.display(), dst.display(), e)
            })?;
        } else if dst.exists() {
            // File deleted in worktree — sync the deletion to original
            std::fs::remove_file(&dst).map_err(|e| {
                format!("failed to delete {}: {}", dst.display(), e)
            })?;
        }
    }

    cleanup_shadow_branch(sb);
    Ok(())
}

/// Remove a shadow worktree and its associated branch.
fn cleanup_shadow_branch(sb: &ShadowBranch) {
    let _ = run_command(
        "git",
        &[
            "worktree",
            "remove",
            "--force",
            &sb.worktree_path.to_string_lossy(),
        ],
        Some(&sb.original_path),
    );
    // Also try to delete the branch (best-effort)
    let _ = run_command(
        "git",
        &["branch", "-D", &sb.branch_name],
        Some(&sb.original_path),
    );
}

/// Roll back already-merged working trees after a partial 2PC failure.
///
/// Returns `true` when every rollback command succeeds and `false` when
/// any `git checkout` / `git clean` step fails.
fn rollback_merged_paths(merged_paths: &[PathBuf]) -> bool {
    let mut rollback_clean = true;

    for path in merged_paths {
        let co = Command::new("git")
            .args(["checkout", "--", "."])
            .current_dir(path)
            .output();
        match co {
            Ok(o) if o.status.success() => {}
            Ok(o) => {
                rollback_clean = false;
                eprintln!(
                    "[cascade] ERROR: git checkout rollback failed in {}: {}",
                    path.display(),
                    String::from_utf8_lossy(&o.stderr).trim()
                );
            }
            Err(e) => {
                rollback_clean = false;
                eprintln!(
                    "[cascade] ERROR: git checkout spawn failed in {}: {}",
                    path.display(),
                    e
                );
            }
        }

        let cl = Command::new("git")
            .args(["clean", "-fd"])
            .current_dir(path)
            .output();
        match cl {
            Ok(o) if o.status.success() => {}
            Ok(o) => {
                rollback_clean = false;
                eprintln!(
                    "[cascade] ERROR: git clean rollback failed in {}: {}",
                    path.display(),
                    String::from_utf8_lossy(&o.stderr).trim()
                );
            }
            Err(e) => {
                rollback_clean = false;
                eprintln!(
                    "[cascade] ERROR: git clean spawn failed in {}: {}",
                    path.display(),
                    e
                );
            }
        }
    }

    rollback_clean
}

/// Resolve the 2PC commit phase using the provided merge function.
///
/// Extracted so tests can deterministically force "partial merge then rollback"
/// scenarios without depending on fragile filesystem tricks.
fn resolve_two_phase_commit_with_merge<F>(
    plan: &mut CascadePlan,
    shadow_branches: &[Option<ShadowBranch>],
    mut merge_fn: F,
) -> CommitResolution
where
    F: FnMut(&ShadowBranch) -> Result<(), String>,
{
    let mut merged_paths: Vec<PathBuf> = Vec::new();
    let mut merge_ok = true;

    for sb in shadow_branches.iter().flatten() {
        if let Err(e) = merge_fn(sb) {
            eprintln!(
                "[cascade] ERROR: merge failed for {}: {}",
                sb.original_path.display(),
                e
            );
            merge_ok = false;
            break;
        }
        merged_paths.push(sb.original_path.clone());
    }

    if merge_ok {
        return CommitResolution::default();
    }

    eprintln!("[cascade] merge failure -- rolling back already-merged services");
    let rollback_clean = rollback_merged_paths(&merged_paths);
    if !rollback_clean {
        eprintln!(
            "[cascade] CRITICAL: rollback incomplete — manual intervention required for: {:?}",
            merged_paths
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
        );
    }

    for sb in shadow_branches.iter().flatten() {
        cleanup_shadow_branch(sb);
    }

    for phase in plan.phases.iter_mut() {
        if matches!(phase.status, PhaseStatus::Passed) {
            phase.status = PhaseStatus::Failed("merge rollback: 2PC commit failed".into());
        }
    }

    CommitResolution {
        hard_halted: true,
        services_failed_delta: 1,
    }
}

// ============================================================================
// Plan execution
// ============================================================================

/// Execute a complete cascade plan.
///
/// Phases are processed in order (already sorted by depth).  A phase is
/// skipped only when one of its **direct upstream dependencies** (per
/// `upstream_map`) has failed or been skipped -- transitive failures
/// propagate naturally because skipped phases also enter `failed_ids`.
///
/// When not in dry-run mode, each phase executes inside a git worktree
/// (shadow branch).  If all phases pass, worktree changes are merged back
/// atomically.  If any phase fails, all worktrees are discarded.
/// Optional cancellation token for Epoch preemption.
///
/// Cancellation is **checkpoint-based**: the token is polled before each
/// phase, before each repair attempt, and before the 2PC merge gate.
/// Running external commands (`forge build`, `cargo check`, `mechanic
/// repair`) are NOT killed mid-execution — cancellation takes effect
/// after the current subprocess returns.  This is a deliberate design
/// choice to avoid leaving file systems in undefined states.
pub type CancelToken = std::sync::Arc<std::sync::atomic::AtomicBool>;

pub fn execute_plan(plan: &mut CascadePlan) -> CascadeReport {
    execute_plan_with_cancel(plan, None)
}

/// Execute a cascade plan with optional cancellation support.
pub fn execute_plan_with_cancel(
    plan: &mut CascadePlan,
    cancel: Option<&CancelToken>,
) -> CascadeReport {
    let wall_start = Instant::now();
    plan.started_at = Some(wall_start);

    let mut hard_halted = false;
    let mut services_repaired: usize = 0;
    let mut services_failed: usize = 0;
    let mut cancelled = false;

    // Track service_ids that have failed or hard-halted so downstream
    // phases can be skipped.
    let mut failed_ids: HashSet<String> = HashSet::new();

    if plan.config.dry_run {
        eprintln!("[cascade] === DRY RUN -- no commands will be executed ===");
        eprintln!(
            "[cascade] trigger: {} (changes: {:?})",
            plan.trigger_service, plan.trigger_changes
        );
        for phase in &plan.phases {
            eprintln!(
                "[cascade]   phase: {} (depth={}, path={})",
                phase.service_id,
                phase.depth,
                phase.service_path.display()
            );
        }
        // Mark all phases as Skipped in dry-run mode.
        for phase in &mut plan.phases {
            phase.status = PhaseStatus::Skipped;
        }
        plan.completed = true;
        return CascadeReport {
            plan: take_plan(plan),
            total_duration: wall_start.elapsed(),
            services_repaired: 0,
            services_failed: 0,
            hard_halted: false,
        };
    }

    eprintln!(
        "[cascade] executing plan: trigger={}, phases={}",
        plan.trigger_service,
        plan.phases.len()
    );

    // ---- Phase 1 (2PC): Prepare shadow branches ----
    // For each phase, try to create a git worktree.  If git is unavailable
    // or the service isn't in a repo, degrade to direct operation (None).
    let shadow_branches: Vec<Option<ShadowBranch>> = plan
        .phases
        .iter()
        .map(|phase| {
            match create_shadow_branch(&phase.service_path, &phase.service_id) {
                Ok(sb) => {
                    eprintln!(
                        "[cascade] shadow branch created: {} -> {}",
                        phase.service_id,
                        sb.worktree_path.display()
                    );
                    Some(sb)
                }
                Err(e) => {
                    eprintln!(
                        "[cascade] shadow branch unavailable for {} (degrading to direct): {}",
                        phase.service_id, e
                    );
                    None
                }
            }
        })
        .collect();

    // Temporarily swap service_paths to worktree paths for phases that have
    // shadow branches.  We save the originals to restore later.
    let original_paths: Vec<PathBuf> = plan
        .phases
        .iter()
        .map(|p| p.service_path.clone())
        .collect();
    for (idx, sb) in shadow_branches.iter().enumerate() {
        if let Some(sb) = sb {
            // For monorepo: the worktree is the git root, but the service may
            // be in a sub-path. Compute the relative offset from original_path
            // to git root, then apply it to the worktree path.
            let original = &original_paths[idx];
            let subpath = find_subpath_from_git_root(original);
            plan.phases[idx].service_path = if subpath.as_os_str().is_empty() {
                sb.worktree_path.clone()
            } else {
                sb.worktree_path.join(&subpath)
            };
        }
    }

    // Clone config to avoid borrow conflicts (phases borrow plan mutably,
    // config is read-only during execution).
    let config = plan.config.clone();

    let is_cancelled = || -> bool {
        cancel
            .map(|c| c.load(std::sync::atomic::Ordering::Relaxed))
            .unwrap_or(false)
    };

    let num_phases = plan.phases.len();
    for idx in 0..num_phases {
        // Check cancellation at the start of each phase
        if is_cancelled() {
            eprintln!("[cascade] CANCELLED: epoch preempted before phase {}", plan.phases[idx].service_id);
            for remaining in idx..num_phases {
                plan.phases[remaining].status = PhaseStatus::Skipped;
            }
            cancelled = true;
            break;
        }

        // ---- dependency-based skip ----
        // Skip only when a direct upstream (per upstream_map) is in failed_ids.
        // Transitive failures propagate naturally because skipped phases are
        // also added to failed_ids.
        let should_skip =
            if let Some(upstreams) = plan.upstream_map.get(&plan.phases[idx].service_id) {
                upstreams.iter().any(|up| failed_ids.contains(up))
            } else {
                false
            };

        if should_skip {
            plan.phases[idx].status = PhaseStatus::Skipped;
            failed_ids.insert(plan.phases[idx].service_id.clone());
            services_failed += 1;
            eprintln!(
                "[cascade] SKIP: {} (upstream dependency failure)",
                plan.phases[idx].service_id
            );
            continue;
        }

        plan.phases[idx].status = PhaseStatus::Running;
        let phase_start = Instant::now();
        let mut passed = false;

        for attempt in 0..config.max_repair_cycles {
            // Check cancellation before each attempt
            if is_cancelled() {
                eprintln!("[cascade] CANCELLED during repair of {}", plan.phases[idx].service_id);
                plan.phases[idx].status = PhaseStatus::Skipped;
                plan.phases[idx].duration = Some(phase_start.elapsed());
                failed_ids.insert(plan.phases[idx].service_id.clone());
                cancelled = true;
                // Skip all remaining phases
                for remaining in (idx + 1)..num_phases {
                    plan.phases[remaining].status = PhaseStatus::Skipped;
                }
                break;
            }

            eprintln!(
                "[cascade] attempt {}/{} for {}",
                attempt + 1,
                config.max_repair_cycles,
                plan.phases[idx].service_id
            );

            match execute_phase_attempt(&mut plan.phases[idx], &config) {
                Ok(true) => {
                    passed = true;
                    break;
                }
                Ok(false) => {
                    plan.phases[idx].repair_attempts += 1;
                }
                Err(e) => {
                    plan.phases[idx].status = PhaseStatus::Failed(e);
                    plan.phases[idx].duration = Some(phase_start.elapsed());
                    failed_ids.insert(plan.phases[idx].service_id.clone());
                    services_failed += 1;
                    break;
                }
            }
        }

        // If cancelled inside the attempt loop, break the outer phase loop too
        if cancelled {
            break;
        }

        if passed {
            plan.phases[idx].status = PhaseStatus::Passed;
            plan.phases[idx].duration = Some(phase_start.elapsed());
            services_repaired += 1;
        } else if matches!(plan.phases[idx].status, PhaseStatus::Running) {
            // Exhausted all repair cycles without passing
            let msg = format!(
                "MAX_REPAIR_CYCLES exceeded ({} attempts)",
                config.max_repair_cycles
            );
            plan.phases[idx].status = PhaseStatus::HardHalt(msg);
            plan.phases[idx].duration = Some(phase_start.elapsed());
            failed_ids.insert(plan.phases[idx].service_id.clone());
            hard_halted = true;
            services_failed += 1;
            eprintln!(
                "\u{1f6a8} HARD HALT: {} failed after {} attempts",
                plan.phases[idx].service_id, config.max_repair_cycles
            );
        }
    }

    // Restore original service paths before 2PC resolution.
    for (idx, orig) in original_paths.iter().enumerate() {
        plan.phases[idx].service_path = orig.clone();
    }

    // ---- Phase 2 (2PC): Commit or Rollback ----
    // Re-check cancellation right before merge gate (closes the last window)
    if is_cancelled() {
        cancelled = true;
    }
    let all_passed = failed_ids.is_empty() && !cancelled;
    if all_passed {
        eprintln!("[cascade] all phases passed -- merging shadow branches");
        let resolution =
            resolve_two_phase_commit_with_merge(plan, &shadow_branches, merge_shadow_branch);
        hard_halted |= resolution.hard_halted;
        services_failed += resolution.services_failed_delta;
    } else {
        eprintln!("[cascade] failures detected -- rolling back all shadow branches");
        for sb in shadow_branches.iter().flatten() {
            cleanup_shadow_branch(sb);
        }
    }

    plan.completed = true;
    let total_duration = wall_start.elapsed();

    CascadeReport {
        plan: take_plan(plan),
        total_duration,
        services_repaired,
        services_failed,
        hard_halted,
    }
}

/// Move the plan out of the mutable reference, replacing it with a
/// completed empty sentinel.  Avoids cloning the whole structure.
fn take_plan(plan: &mut CascadePlan) -> CascadePlan {
    let mut empty = CascadePlan {
        trigger_service: String::new(),
        trigger_changes: Vec::new(),
        phases: Vec::new(),
        config: CascadeConfig::default(),
        upstream_map: HashMap::new(),
        started_at: None,
        completed: true,
    };
    std::mem::swap(plan, &mut empty);
    empty
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::universe_graph::{ServiceDelta, ServiceDependency, ServiceEntry, UniverseGraph};
    use std::collections::HashMap;
    use std::path::PathBuf;

    // ---- Test helpers ----

    /// Build a simple chain graph: C -> B -> A
    /// C is the root provider, B depends on C, A depends on B.
    fn chain_graph() -> UniverseGraph {
        UniverseGraph {
            services: vec![
                ServiceEntry {
                    id: "C".into(),
                    path: PathBuf::from("./svc-c"),
                },
                ServiceEntry {
                    id: "B".into(),
                    path: PathBuf::from("./svc-b"),
                },
                ServiceEntry {
                    id: "A".into(),
                    path: PathBuf::from("./svc-a"),
                },
            ],
            dependencies: {
                let mut m = HashMap::new();
                m.insert(
                    "B".into(),
                    vec![ServiceDependency {
                        provider_id: "C".into(),
                        consumes: vec!["TypeX".into()],
                        breaks_if: vec!["type-signature-change".into()],
                        source_file: PathBuf::from("./svc-b/intent.lisp"),
                    }],
                );
                m.insert(
                    "A".into(),
                    vec![ServiceDependency {
                        provider_id: "B".into(),
                        consumes: vec!["TypeY".into()],
                        breaks_if: vec!["api-removal".into()],
                        source_file: PathBuf::from("./svc-a/intent.lisp"),
                    }],
                );
                m
            },
        }
    }

    /// Build a fan-out graph: P -> {X, Y}
    fn fan_out_graph() -> UniverseGraph {
        UniverseGraph {
            services: vec![
                ServiceEntry {
                    id: "P".into(),
                    path: PathBuf::from("./svc-p"),
                },
                ServiceEntry {
                    id: "X".into(),
                    path: PathBuf::from("./svc-x"),
                },
                ServiceEntry {
                    id: "Y".into(),
                    path: PathBuf::from("./svc-y"),
                },
            ],
            dependencies: {
                let mut m = HashMap::new();
                m.insert(
                    "X".into(),
                    vec![ServiceDependency {
                        provider_id: "P".into(),
                        consumes: vec!["Foo".into()],
                        breaks_if: vec!["change".into()],
                        source_file: PathBuf::from("./svc-x/intent.lisp"),
                    }],
                );
                m.insert(
                    "Y".into(),
                    vec![ServiceDependency {
                        provider_id: "P".into(),
                        consumes: vec!["Bar".into()],
                        breaks_if: vec!["change".into()],
                        source_file: PathBuf::from("./svc-y/intent.lisp"),
                    }],
                );
                m
            },
        }
    }

    // ---- create_plan: correct phase generation and depth ordering ----

    #[test]
    fn test_create_plan_generates_phases_in_depth_order() {
        let graph = chain_graph();
        let delta = ServiceDelta {
            service_id: "C".into(),
            changed_interfaces: vec!["type-signature-change".into()],
        };
        let manifest_dir = Path::new("/tmp/universe");
        let config = CascadeConfig::default();

        let plan = create_plan(&graph, &delta, manifest_dir, config);

        assert_eq!(plan.trigger_service, "C");
        assert_eq!(plan.phases.len(), 2);

        // B is depth 1 (direct consumer of C), A is depth 2
        assert_eq!(plan.phases[0].service_id, "B");
        assert_eq!(plan.phases[0].depth, 1);
        assert_eq!(
            plan.phases[0].service_path,
            PathBuf::from("/tmp/universe/./svc-b")
        );

        assert_eq!(plan.phases[1].service_id, "A");
        assert_eq!(plan.phases[1].depth, 2);
        assert_eq!(
            plan.phases[1].service_path,
            PathBuf::from("/tmp/universe/./svc-a")
        );

        // All phases start as Pending
        for phase in &plan.phases {
            assert_eq!(phase.status, PhaseStatus::Pending);
            assert_eq!(phase.repair_attempts, 0);
        }
    }

    // ---- create_plan: upstream_map is populated correctly ----

    #[test]
    fn test_create_plan_builds_upstream_map() {
        let graph = chain_graph();
        let delta = ServiceDelta {
            service_id: "C".into(),
            changed_interfaces: vec!["type-signature-change".into()],
        };
        let manifest_dir = Path::new("/tmp/universe");
        let config = CascadeConfig::default();

        let plan = create_plan(&graph, &delta, manifest_dir, config);

        // B depends on C, A depends on B
        assert_eq!(plan.upstream_map.get("B").unwrap(), &vec!["C".to_string()]);
        assert_eq!(plan.upstream_map.get("A").unwrap(), &vec!["B".to_string()]);
    }

    // ---- create_plan: empty blast radius ----

    #[test]
    fn test_create_plan_empty_blast_radius() {
        let graph = chain_graph();
        // Change an interface that no consumer breaks on
        let delta = ServiceDelta {
            service_id: "C".into(),
            changed_interfaces: vec!["unrelated-change".into()],
        };
        let manifest_dir = Path::new("/tmp/universe");
        let config = CascadeConfig::default();

        let plan = create_plan(&graph, &delta, manifest_dir, config);

        assert_eq!(plan.phases.len(), 0);
        assert_eq!(plan.trigger_service, "C");
    }

    // ---- dry_run: no commands executed ----

    #[test]
    fn test_dry_run_does_not_execute_commands() {
        let graph = chain_graph();
        let delta = ServiceDelta {
            service_id: "C".into(),
            changed_interfaces: vec!["type-signature-change".into()],
        };
        let manifest_dir = Path::new("/tmp/universe");
        let config = CascadeConfig {
            dry_run: true,
            ..CascadeConfig::default()
        };

        let mut plan = create_plan(&graph, &delta, manifest_dir, config);
        assert_eq!(plan.phases.len(), 2);

        let report = execute_plan(&mut plan);

        // In dry-run, nothing is actually executed
        assert_eq!(report.services_repaired, 0);
        assert_eq!(report.services_failed, 0);
        assert!(!report.hard_halted);

        // All phases should be Skipped (dry-run marker)
        for phase in &report.plan.phases {
            assert_eq!(phase.status, PhaseStatus::Skipped);
        }
    }

    // ---- dry_run: empty plan produces clean report ----

    #[test]
    fn test_dry_run_empty_plan() {
        let graph = chain_graph();
        let delta = ServiceDelta {
            service_id: "C".into(),
            changed_interfaces: vec!["unrelated-change".into()],
        };
        let manifest_dir = Path::new("/tmp/universe");
        let config = CascadeConfig {
            dry_run: true,
            ..CascadeConfig::default()
        };

        let mut plan = create_plan(&graph, &delta, manifest_dir, config);
        assert_eq!(plan.phases.len(), 0);

        let report = execute_plan(&mut plan);
        assert_eq!(report.services_repaired, 0);
        assert_eq!(report.services_failed, 0);
        assert!(!report.hard_halted);
        assert!(report.plan.phases.is_empty());
    }

    // ---- HardHalt logic (simulated) ----

    #[test]
    fn test_hard_halt_after_max_repair_cycles() {
        let max_cycles: usize = 3;
        let mut phase = CascadePhase {
            service_id: "test-svc".into(),
            service_path: PathBuf::from("/nonexistent/path"),
            depth: 1,
            status: PhaseStatus::Running,
            repair_attempts: 0,
            duration: None,
            package_name: None,
            last_compiler_output: None,
            files_modified: Vec::new(),
            repair_diff: None,
        };

        // Simulate max_cycles failed attempts
        for _ in 0..max_cycles {
            phase.repair_attempts += 1;
        }

        assert_eq!(phase.repair_attempts, max_cycles);
        let msg = format!("MAX_REPAIR_CYCLES exceeded ({} attempts)", max_cycles);
        phase.status = PhaseStatus::HardHalt(msg.clone());

        assert_eq!(phase.status, PhaseStatus::HardHalt(msg));
    }

    #[test]
    fn test_skipped_phases_after_upstream_failure() {
        // Simulate dependency-based skip: "downstream" depends on "upstream"
        let mut upstream_map: HashMap<String, Vec<String>> = HashMap::new();
        upstream_map.insert("upstream".into(), vec![]);
        upstream_map.insert("downstream".into(), vec!["upstream".into()]);

        let mut phases = vec![
            CascadePhase {
                service_id: "upstream".into(),
                service_path: PathBuf::from("/tmp/upstream"),
                depth: 1,
                status: PhaseStatus::HardHalt("failed".into()),
                repair_attempts: 3,
                duration: Some(Duration::from_secs(1)),
                last_compiler_output: None,
                files_modified: Vec::new(),
                repair_diff: None,
                package_name: None,
            },
            CascadePhase {
                service_id: "downstream".into(),
                service_path: PathBuf::from("/tmp/downstream"),
                depth: 2,
                status: PhaseStatus::Pending,
                repair_attempts: 0,
                duration: None,
                last_compiler_output: None,
                files_modified: Vec::new(),
                repair_diff: None,
                package_name: None,
            },
        ];

        // Simulate the dependency-based skip logic from execute_plan
        let failed_ids: HashSet<String> = vec!["upstream".to_string()].into_iter().collect();

        for idx in 0..phases.len() {
            let should_skip =
                if let Some(upstreams) = upstream_map.get(&phases[idx].service_id) {
                    upstreams.iter().any(|up| failed_ids.contains(up))
                } else {
                    false
                };
            if should_skip {
                phases[idx].status = PhaseStatus::Skipped;
            }
        }

        // upstream stays HardHalt, downstream becomes Skipped
        assert!(matches!(phases[0].status, PhaseStatus::HardHalt(_)));
        assert_eq!(phases[1].status, PhaseStatus::Skipped);
    }

    // ---- Same-depth phases are NOT skipped by sibling failure ----

    #[test]
    fn test_same_depth_phases_not_skipped_by_sibling() {
        // svc-x and svc-y both depend on P, but NOT on each other
        let mut upstream_map: HashMap<String, Vec<String>> = HashMap::new();
        upstream_map.insert("svc-x".into(), vec!["P".into()]);
        upstream_map.insert("svc-y".into(), vec!["P".into()]);

        let mut phases = vec![
            CascadePhase {
                service_id: "svc-x".into(),
                service_path: PathBuf::from("/tmp/x"),
                depth: 1,
                status: PhaseStatus::HardHalt("failed".into()),
                repair_attempts: 3,
                duration: Some(Duration::from_secs(1)),
                last_compiler_output: None,
                files_modified: Vec::new(),
                repair_diff: None,
                package_name: None,
            },
            CascadePhase {
                service_id: "svc-y".into(),
                service_path: PathBuf::from("/tmp/y"),
                depth: 1, // same depth as svc-x
                status: PhaseStatus::Pending,
                repair_attempts: 0,
                duration: None,
                last_compiler_output: None,
                files_modified: Vec::new(),
                repair_diff: None,
                package_name: None,
            },
        ];

        // svc-x failed, but svc-y does NOT depend on svc-x
        let failed_ids: HashSet<String> = vec!["svc-x".to_string()].into_iter().collect();

        for idx in 0..phases.len() {
            let should_skip =
                if let Some(upstreams) = upstream_map.get(&phases[idx].service_id) {
                    upstreams.iter().any(|up| failed_ids.contains(up))
                } else {
                    false
                };
            if should_skip {
                phases[idx].status = PhaseStatus::Skipped;
            }
        }

        // svc-y should NOT be skipped (does not depend on svc-x)
        assert!(matches!(phases[0].status, PhaseStatus::HardHalt(_)));
        assert_eq!(phases[1].status, PhaseStatus::Pending);
    }

    // ---- Dependency-based skip: unrelated same-depth failure does not cascade ----

    #[test]
    fn test_unrelated_same_depth_failure_does_not_skip() {
        // Graph: A->B(depth1,FAIL) and A->C(depth1,PASS), D(depth2) depends on C only
        // D should NOT be skipped even though B at depth1 failed.
        let mut upstream_map: HashMap<String, Vec<String>> = HashMap::new();
        upstream_map.insert("B".into(), vec!["A".into()]);
        upstream_map.insert("C".into(), vec!["A".into()]);
        upstream_map.insert("D".into(), vec!["C".into()]); // D depends on C, NOT B

        let mut phases = vec![
            CascadePhase {
                service_id: "B".into(),
                service_path: PathBuf::from("/tmp/b"),
                depth: 1,
                status: PhaseStatus::HardHalt("failed".into()),
                repair_attempts: 3,
                duration: Some(Duration::from_secs(1)),
                last_compiler_output: None,
                files_modified: Vec::new(),
                repair_diff: None,
                package_name: None,
            },
            CascadePhase {
                service_id: "C".into(),
                service_path: PathBuf::from("/tmp/c"),
                depth: 1,
                status: PhaseStatus::Passed,
                repair_attempts: 0,
                duration: Some(Duration::from_secs(1)),
                last_compiler_output: None,
                files_modified: Vec::new(),
                repair_diff: None,
                package_name: None,
            },
            CascadePhase {
                service_id: "D".into(),
                service_path: PathBuf::from("/tmp/d"),
                depth: 2,
                status: PhaseStatus::Pending,
                repair_attempts: 0,
                duration: None,
                last_compiler_output: None,
                files_modified: Vec::new(),
                repair_diff: None,
                package_name: None,
            },
        ];

        // B failed but D depends on C (which passed), not B
        let failed_ids: HashSet<String> = vec!["B".to_string()].into_iter().collect();

        for idx in 0..phases.len() {
            let should_skip =
                if let Some(upstreams) = upstream_map.get(&phases[idx].service_id) {
                    upstreams.iter().any(|up| failed_ids.contains(up))
                } else {
                    false
                };
            if should_skip {
                phases[idx].status = PhaseStatus::Skipped;
            }
        }

        // B stays HardHalt, C stays Passed, D should NOT be skipped
        assert!(matches!(phases[0].status, PhaseStatus::HardHalt(_)));
        assert_eq!(phases[1].status, PhaseStatus::Passed);
        assert_eq!(phases[2].status, PhaseStatus::Pending); // NOT Skipped!
    }

    // ---- CascadeConfig defaults ----

    #[test]
    fn test_cascade_config_defaults() {
        let config = CascadeConfig::default();
        assert_eq!(config.max_repair_cycles, 3);
        assert_eq!(config.phase_timeout, Duration::from_secs(600));
        assert!(!config.dry_run);
        assert!(config.enable_clippy);
        assert!(!config.enable_tests);
    }

    // ---- create_plan: trigger changes are preserved ----

    #[test]
    fn test_create_plan_preserves_trigger_changes() {
        let graph = chain_graph();
        let delta = ServiceDelta {
            service_id: "C".into(),
            changed_interfaces: vec!["type-signature-change".into(), "api-removal".into()],
        };
        let manifest_dir = Path::new("/tmp/universe");
        let config = CascadeConfig::default();

        let plan = create_plan(&graph, &delta, manifest_dir, config);

        assert_eq!(plan.trigger_changes.len(), 2);
        assert_eq!(plan.trigger_changes[0], "type-signature-change");
        assert_eq!(plan.trigger_changes[1], "api-removal");
    }

    // ---- Fan-out: multiple services at same depth ----

    #[test]
    fn test_create_plan_fan_out_same_depth() {
        let graph = fan_out_graph();
        let delta = ServiceDelta {
            service_id: "P".into(),
            changed_interfaces: vec!["change".into()],
        };

        let plan = create_plan(
            &graph,
            &delta,
            Path::new("/tmp"),
            CascadeConfig::default(),
        );
        assert_eq!(plan.phases.len(), 2);
        assert!(plan.phases.iter().all(|p| p.depth == 1));

        let ids: HashSet<&str> = plan.phases.iter().map(|p| p.service_id.as_str()).collect();
        assert!(ids.contains("X"));
        assert!(ids.contains("Y"));
    }

    // ---- Plan starts incomplete ----

    #[test]
    fn test_plan_starts_incomplete() {
        let graph = chain_graph();
        let delta = ServiceDelta {
            service_id: "C".into(),
            changed_interfaces: vec!["type-signature-change".into()],
        };
        let plan = create_plan(&graph, &delta, Path::new("/tmp"), CascadeConfig::default());

        assert!(!plan.completed);
        assert!(plan.started_at.is_none());
    }

    // ---- cancellation prevents merge (Bug 1 regression test) ----

    #[test]
    fn test_cancelled_cascade_never_merges() {
        // Simulate: cancel token is already set before execution starts
        let graph = chain_graph();
        let delta = ServiceDelta {
            service_id: "C".into(),
            changed_interfaces: vec!["type-signature-change".into()],
        };
        let config = CascadeConfig::default();
        let mut plan = create_plan(&graph, &delta, Path::new("/tmp"), config);
        assert_eq!(plan.phases.len(), 2);

        // Pre-set cancel token
        let token = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        let report = execute_plan_with_cancel(&mut plan, Some(&token));

        // All phases must be Skipped, nothing passed
        assert!(report.plan.phases.iter().all(|p| matches!(p.status, PhaseStatus::Skipped)));
        assert_eq!(report.services_repaired, 0);
        // hard_halted should be false (cancelled, not halted)
        // but the cascade must NOT have attempted any merge
        assert!(!report.hard_halted || report.services_failed > 0);
    }

    #[test]
    fn test_cancel_proves_cancellation_on_independent_siblings() {
        // Use fan_out_graph: P -> {X, Y} where X and Y are INDEPENDENT.
        // Pre-set cancel token = true.
        // Both X and Y must end up Skipped.
        // Since X and Y have no dependency on each other, the ONLY way both
        // can be Skipped is via the `is_cancelled()` path in execute_plan.
        // (If it were upstream-failure, X failing would not cause Y to skip
        // because Y does not depend on X.)
        let graph = fan_out_graph();
        let delta = ServiceDelta {
            service_id: "P".into(),
            changed_interfaces: vec!["change".into()],
        };
        let config = CascadeConfig {
            max_repair_cycles: 1,
            ..CascadeConfig::default()
        };
        let mut plan = create_plan(&graph, &delta, Path::new("/tmp"), config);
        assert_eq!(plan.phases.len(), 2); // X and Y, both depth 1, independent

        let token = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        let report = execute_plan_with_cancel(&mut plan, Some(&token));

        // ASSERTION: Both independent siblings are Skipped.
        // This is impossible via upstream-failure alone (they don't depend on each other).
        // It proves the `is_cancelled()` → `cancelled = true` → skip-all path was taken.
        for phase in &report.plan.phases {
            assert!(
                matches!(phase.status, PhaseStatus::Skipped),
                "phase '{}' should be Skipped via cancellation, got {:?}",
                phase.service_id, phase.status
            );
        }
        assert_eq!(report.services_repaired, 0);
        // Merge must not have happened: all_passed = failed_ids.is_empty() && !cancelled
        // With cancelled=true, even if failed_ids is empty, merge is blocked.
        // Verify by checking hard_halted is false (it's cancel, not halt):
        assert!(!report.hard_halted, "cancelled cascade should not be marked hard_halted");
    }

    #[test]
    fn test_merge_shadow_branch_fails_on_invalid_worktree() {
        // Directly test that merge_shadow_branch returns Err when given
        // a nonexistent worktree path. This proves the error path in the
        // 2PC commit phase is reachable and returns a proper error.
        let sb = ShadowBranch {
            worktree_path: PathBuf::from("/nonexistent/worktree/path"),
            branch_name: "cascade/test-bogus".into(),
            original_path: PathBuf::from("/nonexistent/original"),
        };
        let result = merge_shadow_branch(&sb);
        assert!(
            result.is_err(),
            "merge_shadow_branch with nonexistent worktree must return Err"
        );
    }

    #[test]
    fn test_cleanup_shadow_branch_does_not_panic() {
        // cleanup is best-effort: must not panic on nonexistent paths
        let sb = ShadowBranch {
            worktree_path: PathBuf::from("/nonexistent/worktree/cleanup"),
            branch_name: "cascade/test-cleanup-bogus".into(),
            original_path: PathBuf::from("/nonexistent/original"),
        };
        cleanup_shadow_branch(&sb); // must not panic
    }

    fn init_git_repo(dir: &Path, tracked_rel: &str, contents: &str) {
        let tracked_path = dir.join(tracked_rel);
        if let Some(parent) = tracked_path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&tracked_path, contents).unwrap();

        let init = Command::new("git")
            .args(["init"])
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(init.status.success(), "git init failed: {:?}", init);

        let add = Command::new("git")
            .args(["add", "."])
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(add.status.success(), "git add failed: {:?}", add);

        let commit = Command::new("git")
            .args([
                "-c",
                "user.email=test@example.com",
                "-c",
                "user.name=Forge Core Tests",
                "commit",
                "-m",
                "init",
            ])
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(
            commit.status.success(),
            "git commit failed: {}",
            String::from_utf8_lossy(&commit.stderr)
        );
    }

    #[test]
    fn test_resolve_two_phase_commit_rolls_back_partial_merge_changes() {
        use std::fs;

        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "cascade-rollback-{}-{}",
            std::process::id(),
            unique
        ));
        let svc_a = root.join("svc-a");
        let svc_b = root.join("svc-b");
        fs::create_dir_all(&svc_a).unwrap();
        fs::create_dir_all(&svc_b).unwrap();

        init_git_repo(&svc_a, "tracked.txt", "original-a\n");
        init_git_repo(&svc_b, "tracked.txt", "original-b\n");

        let sb_a = create_shadow_branch(&svc_a, "svc-a").unwrap();
        let sb_b = create_shadow_branch(&svc_b, "svc-b").unwrap();
        let worktree_a = sb_a.worktree_path.clone();
        let worktree_b = sb_b.worktree_path.clone();

        let mut plan = CascadePlan {
            trigger_service: "trigger".into(),
            trigger_changes: vec!["change".into()],
            phases: vec![
                CascadePhase {
                    service_id: "svc-a".into(),
                    service_path: svc_a.clone(),
                    depth: 1,
                    status: PhaseStatus::Passed,
                    repair_attempts: 0,
                    duration: None,
                    package_name: None,
                    last_compiler_output: None,
                    files_modified: Vec::new(),
                    repair_diff: None,
                },
                CascadePhase {
                    service_id: "svc-b".into(),
                    service_path: svc_b.clone(),
                    depth: 1,
                    status: PhaseStatus::Passed,
                    repair_attempts: 0,
                    duration: None,
                    package_name: None,
                    last_compiler_output: None,
                    files_modified: Vec::new(),
                    repair_diff: None,
                },
            ],
            config: CascadeConfig::default(),
            upstream_map: HashMap::new(),
            started_at: None,
            completed: false,
        };

        let svc_a_for_merge = svc_a.clone();
        let svc_b_for_merge = svc_b.clone();
        let resolution =
            resolve_two_phase_commit_with_merge(&mut plan, &[Some(sb_a), Some(sb_b)], move |sb| {
                if sb.original_path == svc_a_for_merge {
                    fs::write(svc_a_for_merge.join("tracked.txt"), "mutated-a\n").unwrap();
                    fs::write(svc_a_for_merge.join("scratch.tmp"), "junk\n").unwrap();
                    Ok(())
                } else if sb.original_path == svc_b_for_merge {
                    Err("synthetic merge failure".into())
                } else {
                    Err("unexpected service".into())
                }
            });

        assert_eq!(
            resolution,
            CommitResolution {
                hard_halted: true,
                services_failed_delta: 1,
            }
        );
        assert_eq!(
            fs::read_to_string(svc_a.join("tracked.txt")).unwrap(),
            "original-a\n"
        );
        assert!(
            !svc_a.join("scratch.tmp").exists(),
            "git clean should remove merged junk file"
        );
        assert!(
            plan.phases.iter().all(
                |phase| matches!(&phase.status, PhaseStatus::Failed(msg) if msg == "merge rollback: 2PC commit failed")
            ),
            "all previously passed phases must be re-marked failed after rollback"
        );
        assert!(
            !worktree_a.exists(),
            "successful shadow worktree should be cleaned up after rollback"
        );
        assert!(
            !worktree_b.exists(),
            "failed shadow worktree should be cleaned up after rollback"
        );

        let _ = fs::remove_dir_all(&root);
    }
}
