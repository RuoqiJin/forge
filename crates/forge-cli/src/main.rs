//! Forge CLI — stateless code stamping compiler.
//!
//! Reads `.missiond/*.lisp` from a target project, detects top-level tags,
//! and dispatches to registered pattern processors for code generation.
//!
//! Usage:
//!   forge build ~/Projects/jarvis
//!   forge build .                    # current directory
//!   forge build . --output-dir /tmp  # write to /tmp instead of project tree
//!   forge check ~/Projects/jarvis    # validate only, no write
//!   forge lint ~/Projects/jarvis     # governance lint: granularity, structure, split hints

mod audit;
mod registry;

use crate::registry::{build_registry, ForgeContext, GeneratedFile, MissiondToolDefsProcessor, PatternProcessor};
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(name = "forge", version, about = "Stateless code stamping compiler")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Read .missiond/*.lisp, validate IR, generate code into target project
    Build {
        /// Project root directory (must contain .missiond/)
        #[arg(default_value = ".")]
        project_root: PathBuf,

        /// Dry run: parse and validate only, don't write files
        #[arg(long)]
        dry_run: bool,

        /// Output directory (overrides :target paths in Lisp declarations)
        #[arg(long)]
        output_dir: Option<PathBuf>,
    },
    /// Validate .missiond/*.lisp without generating code
    Check {
        /// Project root directory
        #[arg(default_value = ".")]
        project_root: PathBuf,
    },
    /// Lint intent governance: granularity levels, structural rules, split suggestions
    Lint {
        /// Project root directory
        #[arg(default_value = ".")]
        project_root: PathBuf,
    },
    /// Audit custom implementation status (three-state: implemented/pending/missing)
    Audit {
        /// Project root directory (must contain .missiond/)
        #[arg(default_value = ".")]
        project_root: PathBuf,

        /// Output as JSON instead of terminal report
        #[arg(long)]
        json: bool,
    },

    /// Display the Universe Graph from a universe.intent.lisp manifest
    Universe {
        /// Path to universe.intent.lisp
        #[arg()]
        manifest: PathBuf,

        /// Output format: text, json
        #[arg(long, default_value = "text")]
        format: String,
    },

    /// Calculate blast radius for a service change
    BlastRadius {
        /// Path to universe.intent.lisp
        #[arg()]
        manifest: PathBuf,

        /// Changed service ID
        #[arg(long)]
        service: String,

        /// Changed interfaces (comma-separated)
        #[arg(long, value_delimiter = ',')]
        changed: Vec<String>,
    },

    /// Validate universe integrity: dependency consistency, orphans, undeclared cross-service refs
    LintUniverse {
        /// Path to universe.intent.lisp
        #[arg()]
        manifest: PathBuf,
    },

    /// Analyze repair history and generate strategy recommendations (P5 flywheel)
    Distill {
        /// Directory containing repair logs (.forge-logs/)
        #[arg()]
        log_dir: PathBuf,

        /// Output briefing update to a file
        #[arg(long)]
        output: Option<PathBuf>,

        /// Output format: text, json
        #[arg(long, default_value = "text")]
        format: String,
    },

    /// Analyze repair patterns and generate Forge mold evolution proposals (P6 flywheel)
    Evolve {
        /// Directory containing repair logs (.forge-logs/)
        #[arg()]
        log_dir: PathBuf,

        /// Output proposals to a file (Markdown)
        #[arg(long)]
        output: Option<PathBuf>,

        /// Minimum utility threshold (0.0-1.0, default 0.3)
        #[arg(long, default_value = "0.3")]
        min_utility: f64,

        /// Output format: text, json
        #[arg(long, default_value = "text")]
        format: String,
    },

    /// Survey a project's codebase and generate standardized intent.lisp via LLM
    Survey {
        /// Project root directory to survey
        #[arg(default_value = ".")]
        project_root: PathBuf,

        /// Granularity level: L1, L2, L3 (default: L3)
        #[arg(long, default_value = "L3")]
        level: String,

        /// Check mode: report whether existing intent.lisp is stale (no LLM call)
        #[arg(long)]
        check: bool,

        /// Dry run: print the prompt that would be sent to claude, don't execute
        #[arg(long)]
        dry_run: bool,
    },

    /// Run cascade repair: detect changes, compute blast radius, orchestrate repair
    Cascade {
        /// Path to universe.intent.lisp
        #[arg()]
        manifest: PathBuf,

        /// Manual trigger: specify the changed service
        #[arg(long)]
        trigger: Option<String>,

        /// Manual trigger: specify changed interfaces (comma-separated)
        #[arg(long, value_delimiter = ',')]
        changed: Vec<String>,

        /// Watch mode: poll for git changes every N seconds (Intent Sentinel)
        #[arg(long)]
        watch: Option<u64>,

        /// Maximum repair cycles before hard halt (default: 3)
        #[arg(long, default_value = "3")]
        max_cycles: usize,

        /// Dry run: print the plan without executing
        #[arg(long)]
        dry_run: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Build {
            project_root,
            dry_run,
            output_dir,
        } => {
            let root = project_root
                .canonicalize()
                .with_context(|| format!("project root not found: {}", project_root.display()))?;
            let ctx = ForgeContext {
                project_root: root,
                dry_run,
                output_dir,
            };
            run_build(&ctx)
        }
        Commands::Check { project_root } => {
            let root = project_root
                .canonicalize()
                .with_context(|| format!("project root not found: {}", project_root.display()))?;
            let ctx = ForgeContext {
                project_root: root.clone(),
                dry_run: true,
                output_dir: None,
            };
            run_build(&ctx)?;

            // Advisory governance lint (doesn't fail check)
            let missiond_dir = root.join(".missiond");
            if missiond_dir.exists() {
                if let Ok(graph) = forge_core::intent_graph::resolve_intent_graph(&missiond_dir) {
                    let violations = forge_core::governance::lint_structure(&graph);
                    let errors: Vec<_> = violations.iter()
                        .filter(|v| matches!(v.severity, forge_core::governance::ViolationSeverity::Error))
                        .collect();
                    if !errors.is_empty() {
                        eprintln!("\nGovernance warnings:");
                        for e in &errors {
                            eprintln!("  [{}] {}: {}", e.rule, e.file, e.message);
                        }
                    }
                }
            }
            Ok(())
        }
        Commands::Lint { project_root } => {
            let root = project_root
                .canonicalize()
                .with_context(|| format!("project root not found: {}", project_root.display()))?;
            run_lint(&root)
        }
        Commands::Audit { project_root, json } => {
            let root = project_root
                .canonicalize()
                .with_context(|| format!("project root not found: {}", project_root.display()))?;
            let result = audit::audit_project(&root)?;

            if json {
                println!("{}", serde_json::to_string_pretty(&result).unwrap());
            } else {
                print_audit_report(&result);
            }
            Ok(())
        }

        Commands::Universe { manifest, format } => {
            let manifest_path = manifest
                .canonicalize()
                .with_context(|| format!("manifest not found: {}", manifest.display()))?;
            let graph = forge_core::universe_graph::resolve_universe_graph(&manifest_path)
                .map_err(|errs| anyhow::anyhow!("failed to resolve universe graph:\n{}", errs.join("\n")))?;

            match format.as_str() {
                "json" => {
                    // Print services and dependencies as JSON
                    let mut output = serde_json::Map::new();
                    let services: Vec<serde_json::Value> = graph.services.iter().map(|s| {
                        serde_json::json!({ "id": s.id, "path": s.path.display().to_string() })
                    }).collect();
                    output.insert("services".into(), serde_json::Value::Array(services));
                    let deps: serde_json::Map<String, serde_json::Value> = graph.dependencies.iter().map(|(k, v)| {
                        let dep_list: Vec<serde_json::Value> = v.iter().map(|d| {
                            serde_json::json!({ "provider": d.provider_id, "consumes": d.consumes, "breaks_if": d.breaks_if })
                        }).collect();
                        (k.clone(), serde_json::Value::Array(dep_list))
                    }).collect();
                    output.insert("dependencies".into(), serde_json::Value::Object(deps));
                    println!("{}", serde_json::to_string_pretty(&output).unwrap());
                }
                _ => {
                    println!("Universe Graph ({} services):", graph.services.len());
                    for svc in &graph.services {
                        println!("  service: {} (path: {})", svc.id, svc.path.display());
                        if let Some(deps) = graph.dependencies.get(&svc.id) {
                            for dep in deps {
                                println!("    -> depends on: {} (consumes: {:?})", dep.provider_id, dep.consumes);
                            }
                        }
                    }
                }
            }
            Ok(())
        }

        Commands::BlastRadius { manifest, service, changed } => {
            let manifest_path = manifest
                .canonicalize()
                .with_context(|| format!("manifest not found: {}", manifest.display()))?;

            let graph = forge_core::universe_graph::resolve_universe_graph(&manifest_path)
                .map_err(|errs| anyhow::anyhow!("failed to resolve universe graph:\n{}", errs.join("\n")))?;

            let delta = forge_core::universe_graph::ServiceDelta {
                service_id: service,
                changed_interfaces: changed,
            };

            let affected = forge_core::universe_graph::compute_blast_radius(&graph, &delta);

            println!("Blast radius ({} services affected):", affected.len());
            for (i, svc) in affected.iter().enumerate() {
                println!("  {}. {} (depth: {}, reason: {})", i + 1, svc.service_id, svc.depth, svc.reason);
            }
            Ok(())
        }

        Commands::LintUniverse { manifest } => {
            let manifest_path = manifest
                .canonicalize()
                .with_context(|| format!("manifest not found: {}", manifest.display()))?;
            let manifest_dir = manifest_path.parent().unwrap_or(std::path::Path::new("."));

            let graph = forge_core::universe_graph::resolve_universe_graph(&manifest_path)
                .map_err(|errs| anyhow::anyhow!("failed to resolve universe graph:\n{}", errs.join("\n")))?;

            let violations = forge_core::universe_graph::validate_universe_integrity(&graph, manifest_dir);

            if violations.is_empty() {
                println!("Universe integrity check PASSED ({} services, {} dependencies)",
                    graph.services.len(),
                    graph.dependencies.values().map(|v| v.len()).sum::<usize>());
            } else {
                let errors = violations.iter().filter(|v| v.severity == "error").count();
                let warnings = violations.iter().filter(|v| v.severity == "warning").count();
                for v in &violations {
                    let icon = if v.severity == "error" { "ERROR" } else { "WARN " };
                    println!("  [{}] {}: {}", icon, v.service_id, v.message);
                }
                println!("\n{} error(s), {} warning(s)", errors, warnings);
                if errors > 0 {
                    anyhow::bail!("universe integrity check FAILED");
                }
            }
            Ok(())
        }

        Commands::Distill { log_dir, output, format } => {
            let log_dir = log_dir
                .canonicalize()
                .with_context(|| format!("log directory not found: {}", log_dir.display()))?;

            let records = forge_core::repair_log::load_repair_history(&log_dir)
                .map_err(|e| anyhow::anyhow!("failed to load repair history: {}", e))?;

            if records.is_empty() {
                println!("No repair records found in {}", log_dir.display());
                return Ok(());
            }

            let report = forge_core::strategy::distill(&records);

            match format.as_str() {
                "json" => {
                    // JSON output: stats + patterns summary
                    let mut json = serde_json::Map::new();
                    json.insert("total_records".into(), serde_json::json!(report.total_records));
                    json.insert("total_cascades".into(), serde_json::json!(report.stats.total_cascades));
                    json.insert("total_phases".into(), serde_json::json!(report.stats.total_phases));
                    json.insert("success_rate".into(), serde_json::json!(report.stats.success_rate));
                    json.insert("avg_attempts_per_phase".into(), serde_json::json!(report.stats.avg_attempts_per_phase));
                    json.insert("hard_halt_count".into(), serde_json::json!(report.stats.hard_halt_count));
                    json.insert("most_failed_service".into(), serde_json::json!(report.stats.most_failed_service));

                    let patterns: Vec<serde_json::Value> = report.patterns.iter().map(|p| {
                        serde_json::json!({
                            "description": p.description,
                            "frequency": p.frequency,
                            "services": p.services,
                            "trigger_changes": p.trigger_changes,
                            "avg_repair_attempts": p.avg_repair_attempts,
                            "suggested_briefing": p.suggested_briefing,
                        })
                    }).collect();
                    json.insert("patterns".into(), serde_json::Value::Array(patterns));

                    println!("{}", serde_json::to_string_pretty(&json).unwrap());
                }
                _ => {
                    // Text output
                    println!("Strategy Distillation Report");
                    println!("============================");
                    println!("Records analyzed: {}", report.total_records);
                    println!("Total cascades:   {}", report.stats.total_cascades);
                    println!("Total phases:     {}", report.stats.total_phases);
                    println!("Success rate:     {:.1}%", report.stats.success_rate * 100.0);
                    println!("Avg attempts:     {:.2}", report.stats.avg_attempts_per_phase);
                    println!("Hard halts:       {}", report.stats.hard_halt_count);
                    if let Some(ref svc) = report.stats.most_failed_service {
                        println!("Most failed:      {}", svc);
                    }
                    println!();

                    if report.patterns.is_empty() {
                        println!("No recurring failure patterns detected.");
                    } else {
                        println!("Detected {} pattern(s):", report.patterns.len());
                        for (i, p) in report.patterns.iter().enumerate() {
                            println!();
                            println!("  {}. {} (frequency: {})", i + 1, p.description, p.frequency);
                            println!("     Services: {}", p.services.join(", "));
                            println!("     Triggers: {}", p.trigger_changes.join(", "));
                            println!("     Avg attempts: {:.1}", p.avg_repair_attempts);
                            println!("     Suggestion: {}", p.suggested_briefing);
                        }
                    }
                }
            }

            // Write briefing update if --output is specified
            if let Some(output_path) = output {
                let briefing = forge_core::strategy::generate_briefing_update(&report.patterns);
                std::fs::write(&output_path, &briefing)
                    .with_context(|| format!("failed to write briefing to {}", output_path.display()))?;
                println!("\nBriefing update written to {}", output_path.display());
            }

            Ok(())
        }

        Commands::Evolve { log_dir, output, min_utility, format } => {
            let log_dir = log_dir
                .canonicalize()
                .with_context(|| format!("log directory not found: {}", log_dir.display()))?;

            let records = forge_core::repair_log::load_repair_history(&log_dir)
                .map_err(|e| anyhow::anyhow!("failed to load repair history: {}", e))?;

            if records.is_empty() {
                println!("No repair records found in {}", log_dir.display());
                return Ok(());
            }

            // Step 1: Distill patterns
            let distill_report = forge_core::strategy::distill(&records);

            if distill_report.patterns.is_empty() {
                println!("No recurring patterns detected. Nothing to evolve.");
                return Ok(());
            }

            // Step 2: Generate evolution proposals
            let evo_report = forge_core::evolution::generate_proposals(
                &distill_report.patterns,
                &records,
            );

            // Step 3: Filter by min_utility
            let filtered: Vec<_> = evo_report
                .proposals
                .iter()
                .filter(|p| p.estimated_utility >= min_utility)
                .collect();

            match format.as_str() {
                "json" => {
                    let proposals_json: Vec<serde_json::Value> = filtered.iter().map(|p| {
                        serde_json::json!({
                            "title": p.title,
                            "priority": p.priority.label(),
                            "utility": p.estimated_utility,
                            "frequency": p.source_frequency,
                            "services": p.affected_services,
                            "problem": p.problem,
                            "solution": p.proposed_solution,
                            "evidence_count": p.evidence_records.len(),
                            "error_signatures": p.error_signatures,
                            "affected_files": p.affected_file_patterns,
                        })
                    }).collect();

                    let output_json = serde_json::json!({
                        "patterns_analyzed": evo_report.patterns_analyzed,
                        "proposals_total": evo_report.proposals.len(),
                        "proposals_filtered": filtered.len(),
                        "min_utility": min_utility,
                        "summary": evo_report.summary,
                        "proposals": proposals_json,
                    });

                    println!("{}", serde_json::to_string_pretty(&output_json).unwrap());
                }
                _ => {
                    println!("Forge Evolution Report (P6 Mold Fission)");
                    println!("=========================================");
                    println!("Patterns analyzed: {}", evo_report.patterns_analyzed);
                    println!("Proposals total:   {}", evo_report.proposals.len());
                    println!("Min utility filter: {:.2}", min_utility);
                    println!("Proposals shown:   {}", filtered.len());
                    println!();
                    println!("{}", evo_report.summary);
                    println!();

                    if filtered.is_empty() {
                        println!("No proposals meet the minimum utility threshold of {:.2}.", min_utility);
                    } else {
                        for (i, p) in filtered.iter().enumerate() {
                            println!("  {}. [{}] {} (utility: {:.2}, freq: {})",
                                i + 1, p.priority.label(), p.title,
                                p.estimated_utility, p.source_frequency);
                            println!("     Services: {}", p.affected_services.join(", "));
                            println!("     Problem: {}", p.problem);
                            println!("     Solution: {}", p.proposed_solution);
                            if !p.error_signatures.is_empty() {
                                println!("     Error sigs: {}", p.error_signatures.join("; "));
                            }
                            if !p.affected_file_patterns.is_empty() {
                                println!("     Files: {}", p.affected_file_patterns.join(", "));
                            }
                            println!();
                        }
                    }
                }
            }

            // Write Markdown output if --output is specified
            if let Some(output_path) = output {
                let md = forge_core::evolution::format_report_markdown(&evo_report);
                std::fs::write(&output_path, &md)
                    .with_context(|| format!("failed to write output to {}", output_path.display()))?;
                println!("Evolution report written to {}", output_path.display());
            }

            Ok(())
        }

        Commands::Survey { project_root, level, check, dry_run } => {
            let root = project_root
                .canonicalize()
                .with_context(|| format!("project root not found: {}", project_root.display()))?;

            let level = match level.to_uppercase().as_str() {
                "L1" => forge_core::survey::SurveyLevel::L1Blueprint,
                "L2" => forge_core::survey::SurveyLevel::L2Topology,
                "L3" => forge_core::survey::SurveyLevel::L3Implementation,
                other => anyhow::bail!("invalid level: '{}'. Use L1, L2, or L3", other),
            };

            let config = forge_core::survey::SurveyConfig {
                project_root: root,
                level,
                check_only: check,
                dry_run,
            };

            let result = forge_core::survey::run(&config)
                .map_err(|e| anyhow::anyhow!("{}", e))?;

            if check {
                match result.stale {
                    Some(true) => {
                        println!("STALE: intent.lisp needs re-survey (hash mismatch)");
                        println!("  stored hash vs current: {}", result.survey_hash);
                        std::process::exit(1);
                    }
                    Some(false) => {
                        println!("FRESH: intent.lisp is up-to-date (hash: {})", result.survey_hash);
                    }
                    None => unreachable!(),
                }
            } else if dry_run {
                println!("--- DRY RUN: prompt that would be sent to claude ---\n");
                println!("{}", result.intent_lisp);
                println!("\n--- END PROMPT ---");
                println!("\nProject: {} files, {} symbols", result.file_count, result.symbol_count);
                println!("Survey hash: {}", result.survey_hash);
            } else {
                println!("Survey complete!");
                println!("  Level:      {}", level.label());
                println!("  Output:     {}", result.output_path.display());
                println!("  Files:      {}", result.file_count);
                println!("  Symbols:    {}", result.symbol_count);
                println!("  Hash:       {}", result.survey_hash);
                println!("  Valid:      {}", if result.validation_ok { "yes" } else { "NO — manual review needed" });
            }

            Ok(())
        }

        Commands::Cascade { manifest, trigger, changed, watch, max_cycles, dry_run } => {
            let manifest_path = manifest
                .canonicalize()
                .with_context(|| format!("manifest not found: {}", manifest.display()))?;

            if let Some(interval_secs) = watch {
                // === WATCH MODE (Intent Sentinel P2) ===
                run_watch_mode(&manifest_path, interval_secs, max_cycles, dry_run)?;
            } else if let Some(service) = trigger {
                // === MANUAL TRIGGER MODE (Cascade P1) ===
                run_manual_cascade(&manifest_path, &service, &changed, max_cycles, dry_run)?;
            } else {
                anyhow::bail!("Either --trigger <service> or --watch <seconds> is required");
            }
            Ok(())
        }
    }
}

fn run_build(ctx: &ForgeContext) -> Result<()> {
    let missiond_dir = ctx.project_root.join(".missiond");
    if !missiond_dir.exists() {
        anyhow::bail!(
            "No .missiond/ directory found at {}. Is this a Forge-managed project?",
            ctx.project_root.display()
        );
    }

    eprintln!("forge: reading {}", missiond_dir.display());

    let registry = build_registry();

    // Discover all .lisp files
    let mut lisp_files: Vec<PathBuf> = std::fs::read_dir(&missiond_dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("lisp"))
        .collect();
    lisp_files.sort();

    if lisp_files.is_empty() {
        anyhow::bail!("No .lisp files found in {}", missiond_dir.display());
    }

    eprintln!("forge: found {} lisp files", lisp_files.len());

    let mut total_files = 0usize;
    let mut total_bytes = 0usize;
    let mut errors = Vec::new();

    // Collect all parsed nodes across all files, grouped by tag.
    // Batch processors need all nodes with the same tag at once.
    let mut batch_nodes: HashMap<String, Vec<nc_parser::SexpNode>> = HashMap::new();

    // ---------------------------------------------------------------------------
    // Backward-compat: intent-mcp-defs.lisp override
    //
    // This file contains (mcp-module ...) forms, but the old CLI generated
    // MissionD tool definitions instead of MCP handler modules. We preserve
    // this by using the MissiondToolDefsProcessor explicitly.
    // ---------------------------------------------------------------------------
    let missiond_defs_processor = MissiondToolDefsProcessor;

    for path in &lisp_files {
        let filename = path.file_name().unwrap().to_string_lossy();
        let src = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;

        // Skip architecture spec file (no codegen)
        if filename.as_ref() == "intent.lisp" {
            eprintln!("  {filename}: architecture spec (validated, no codegen)");
            continue;
        }

        // Parse all top-level S-EXP forms
        let nodes = match nc_parser::SexpNode::parse(&src) {
            Ok(nodes) => nodes,
            Err(e) => {
                errors.push(format!("{filename}: S-EXP parse error: {e}"));
                continue;
            }
        };

        // Governance-mode gate: skip descriptive files (fail-closed)
        if let Ok(fc_nodes) = forge_core::sexp_parser::SexpNode::parse(&src) {
            match forge_core::intent_graph::IntentFileMeta::from_sexp(&fc_nodes, path, 0) {
                Ok(meta) => {
                    if meta.governance_mode == Some(forge_core::intent_graph::GovernanceMode::Descriptive) {
                        eprintln!("  {filename}: descriptive (no codegen)");
                        continue;
                    }
                }
                Err(_) => {
                    // File has governance-level structure (intent tag) but metadata
                    // parse failed — skip codegen for safety (fail-closed).
                    // Non-intent files (e.g. crud-gateway declarations) will fail
                    // from_sexp because they lack (granularity ...), so we only
                    // block files whose root tag is "intent".
                    if fc_nodes.first()
                        .and_then(|n| n.tag())
                        .map_or(false, |t| t == "intent")
                    {
                        eprintln!("  {filename}: intent metadata parse error, skipping codegen for safety");
                        continue;
                    }
                }
            }
        }

        // Backward-compat: intent-mcp-defs.lisp → MissionD tool defs
        let is_mcp_defs = filename.as_ref() == "intent-mcp-defs.lisp";

        for node in &nodes {
            let tag = match node.tag() {
                Some(t) => t,
                None => continue, // skip bare atoms or empty lists
            };

            // Backward-compat override for MissionD tool definitions
            if is_mcp_defs && tag == "mcp-module" {
                match missiond_defs_processor.process(node, ctx) {
                    Ok(files) => {
                        for f in &files {
                            total_bytes += f.content.len();
                            total_files += 1;
                            if let Err(e) = write_output(f, ctx) {
                                errors.push(format!("{filename}: write error: {e}"));
                            }
                        }
                    }
                    Err(e) => errors.push(format!("{filename}/{tag}: {e}")),
                }
                continue;
            }

            // Check batch processors first
            if registry.is_batch_tag(tag) {
                batch_nodes
                    .entry(tag.to_string())
                    .or_default()
                    .push(node.clone());
                continue;
            }

            // Per-node dispatch (uses get_for_node to handle meta-tags like "pattern")
            if let Some(processor) = registry.get_for_node(node) {
                match processor.process(node, ctx) {
                    Ok(files) => {
                        for f in &files {
                            total_bytes += f.content.len();
                            total_files += 1;
                            if let Err(e) = write_output(f, ctx) {
                                errors.push(format!("{filename}: write error: {e}"));
                            }
                        }
                    }
                    Err(e) => errors.push(format!("{filename}/{tag}: {e}")),
                }
            }
            // Unknown tags are silently skipped (may be architecture specs)
        }
    }

    // Process batch nodes
    for (tag, nodes) in &batch_nodes {
        if let Some(processor) = registry.get_batch(tag) {
            let refs: Vec<&nc_parser::SexpNode> = nodes.iter().collect();
            match processor.process_batch(&refs, ctx) {
                Ok(files) => {
                    for f in &files {
                        total_bytes += f.content.len();
                        total_files += 1;
                        if let Err(e) = write_output(f, ctx) {
                            errors.push(format!("batch/{tag}: write error: {e}"));
                        }
                    }
                }
                Err(e) => errors.push(format!("batch/{tag}: {e}")),
            }
        }
    }

    // Report
    eprintln!();
    if errors.is_empty() {
        if ctx.dry_run {
            eprintln!(
                "forge: CHECK PASSED -- {} patterns validated, {} bytes ready",
                total_files, total_bytes
            );
        } else {
            eprintln!(
                "forge: BUILD OK -- {} files generated, {} bytes written",
                total_files, total_bytes
            );
        }
        Ok(())
    } else {
        eprintln!("forge: {} errors:", errors.len());
        for e in &errors {
            eprintln!("  ERROR: {e}");
        }
        anyhow::bail!("{} pattern(s) failed", errors.len())
    }
}

// ---------------------------------------------------------------------------
// File writer with safety header
// ---------------------------------------------------------------------------

fn write_output(file: &GeneratedFile, ctx: &ForgeContext) -> Result<()> {
    if ctx.dry_run {
        return Ok(());
    }

    // Custom (write-once) files should not overwrite existing files
    let target = Path::new(&file.path);
    if !file.overwrite && target.exists() {
        eprintln!("  skip (exists): {}", file.path);
        return Ok(());
    }

    write_generated(target, &file.content)
}

fn write_generated(path: &Path, content: &str) -> Result<()> {
    // Ensure content has the FORGE header
    if !content.contains("GENERATED BY FORGE") {
        anyhow::bail!(
            "Refusing to write {}: missing GENERATED BY FORGE header",
            path.display()
        );
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, content)
        .with_context(|| format!("failed to write {}", path.display()))?;
    eprintln!("  wrote: {}", path.display());
    Ok(())
}

// ---------------------------------------------------------------------------
// Governance: lint subcommand
// ---------------------------------------------------------------------------

fn run_lint(project_root: &Path) -> Result<()> {
    let missiond_dir = project_root.join(".missiond");
    if !missiond_dir.exists() {
        anyhow::bail!(
            "No .missiond/ directory found at {}. Is this a Forge-managed project?",
            project_root.display()
        );
    }

    match forge_core::intent_graph::resolve_intent_graph(&missiond_dir) {
        Ok(graph) => {
            // If ALL files ended up as orphans, this is a legacy project
            if graph.files.is_empty() && !graph.orphans.is_empty() {
                return run_lint_legacy(&missiond_dir);
            }

            println!(
                "Intent Graph: {} governed files, {} orphans",
                graph.files.len(),
                graph.orphans.len()
            );
            println!(
                "Root: {} ({})",
                graph.files[graph.root].intent_name,
                graph.files[graph.root].level.label()
            );
            println!();

            print_intent_tree(&graph);

            // Run structural lint
            let struct_violations = forge_core::governance::lint_structure(&graph);

            // Run content lint per file
            let mut content_violations = Vec::new();
            for meta in &graph.files {
                let full_path = missiond_dir.join(&meta.file_path);
                if let Ok(content) = std::fs::read_to_string(&full_path) {
                    if let Ok(nodes) = forge_core::sexp_parser::SexpNode::parse(&content) {
                        content_violations.extend(forge_core::governance::lint_content(meta, &nodes));
                    }
                }
            }

            let all_violations: Vec<_> = struct_violations.iter().chain(content_violations.iter()).collect();
            let error_count = all_violations.iter()
                .filter(|v| matches!(v.severity, forge_core::governance::ViolationSeverity::Error))
                .count();
            let warning_count = all_violations.iter()
                .filter(|v| matches!(v.severity, forge_core::governance::ViolationSeverity::Warning))
                .count();
            let info_count = all_violations.iter()
                .filter(|v| matches!(v.severity, forge_core::governance::ViolationSeverity::Info))
                .count();

            for v in &all_violations {
                let icon = match v.severity {
                    forge_core::governance::ViolationSeverity::Error => "ERROR",
                    forge_core::governance::ViolationSeverity::Warning => "WARN ",
                    forge_core::governance::ViolationSeverity::Info => "INFO ",
                };
                let line_info = v.line.map_or(String::new(), |l| format!(":{}", l));
                println!("  {} [{}] {}{}: {}", icon, v.rule, v.file, line_info, v.message);
                if let Some(ref sug) = v.suggestion {
                    println!("        -> {}", sug);
                }
            }

            println!();
            println!("Result: {} errors, {} warnings, {} info", error_count, warning_count, info_count);

            if error_count > 0 {
                std::process::exit(1);
            }
        }
        Err(ir_errors) => {
            // Check: if the only real error is "no L1 root" and there are no governed files,
            // treat as legacy project (all files lacked granularity tags).
            let only_root_error = ir_errors.len() == 1 && ir_errors[0].field == "root";
            if only_root_error {
                return run_lint_legacy(&missiond_dir);
            }

            for e in &ir_errors {
                eprintln!("  error: {}", e);
            }
            std::process::exit(1);
        }
    }

    Ok(())
}

/// Legacy fallback: suggest splits for ungoverned projects
fn run_lint_legacy(missiond_dir: &Path) -> Result<()> {
    println!("No granularity tags found. Analyzing for split recommendations...");

    let lisp_files: Vec<_> = std::fs::read_dir(missiond_dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map_or(false, |ext| ext == "lisp"))
        .collect();

    for path in &lisp_files {
        if let Ok(content) = std::fs::read_to_string(path) {
            let line_count = content.lines().count();
            if let Ok(nodes) = forge_core::sexp_parser::SexpNode::parse(&content) {
                if let Some(suggestion) = forge_core::governance::suggest_split(&nodes, line_count) {
                    let name = path.file_name().unwrap().to_string_lossy();
                    println!(
                        "\n  {} ({} lines, {} tables, {} APIs):",
                        name, suggestion.current_lines, suggestion.table_count, suggestion.api_count
                    );
                    for (fname, desc) in &suggestion.suggested_files {
                        println!("    -> {} — {}", fname, desc);
                    }
                }
            }
        }
    }

    Ok(())
}

fn format_gov_mode(meta: &forge_core::intent_graph::IntentFileMeta) -> String {
    match meta.governance_mode {
        Some(m) => format!(" <{}>", m.label()),
        None => String::new(),
    }
}

fn print_intent_tree(graph: &forge_core::intent_graph::IntentGraph) {
    let root = &graph.files[graph.root];
    println!(
        "  {} [{}]{} ({} lines)",
        root.intent_name, root.level.label(), format_gov_mode(root), root.line_count
    );

    for &(parent_idx, child_idx) in &graph.edges {
        if parent_idx == graph.root {
            let child = &graph.files[child_idx];
            println!(
                "    {} [{}]{} ({} lines)",
                child.file_path.display(), child.level.label(), format_gov_mode(child), child.line_count
            );

            for &(p2, c2) in &graph.edges {
                if p2 == child_idx {
                    let grandchild = &graph.files[c2];
                    println!(
                        "      {} [{}]{} ({} lines)",
                        grandchild.file_path.display(), grandchild.level.label(), format_gov_mode(grandchild), grandchild.line_count
                    );
                }
            }
        }
    }

    if !graph.orphans.is_empty() {
        println!("  Ungoverned (legacy):");
        for orphan in &graph.orphans {
            println!("    {}", orphan.display());
        }
    }
    println!();
}

// ---------------------------------------------------------------------------
// Audit report formatter
// ---------------------------------------------------------------------------

fn print_audit_report(audit: &audit::ProjectAudit) {
    eprintln!("forge audit: {}\n", audit.project);

    for pattern in &audit.patterns {
        eprintln!("  {} (from {})", pattern.pattern, pattern.source_file);
        for entry in &pattern.entries {
            let (icon, detail) = match &entry.status {
                audit::AuditStatus::Implemented { lines } => {
                    let loc = entry.file.as_deref().unwrap_or("?");
                    (
                        "\u{1F7E2}",
                        format!("{lines} lines @ {loc}:{}", entry.start_line.unwrap_or(0)),
                    )
                }
                audit::AuditStatus::Pending => {
                    let loc = entry.file.as_deref().unwrap_or("?");
                    (
                        "\u{1F7E1}",
                        format!("todo!() @ {loc}:{}", entry.start_line.unwrap_or(0)),
                    )
                }
                audit::AuditStatus::Missing => {
                    ("\u{1F534}", "not found in source tree".to_string())
                }
            };
            eprintln!("    {icon} {} \u{2014} {detail}", entry.method);
        }
        eprintln!(
            "    [{}/{} implemented, {} pending, {} missing]\n",
            pattern.implemented,
            pattern.entries.len(),
            pattern.pending,
            pattern.missing
        );
    }

    let total = audit.total_implemented + audit.total_pending + audit.total_missing;
    eprintln!(
        "  TOTAL: {}/{} implemented ({}%), {} pending, {} missing",
        audit.total_implemented,
        total,
        if total > 0 {
            audit.total_implemented * 100 / total
        } else {
            0
        },
        audit.total_pending,
        audit.total_missing
    );
}

// ---------------------------------------------------------------------------
// Cascade: manual trigger (P1)
// ---------------------------------------------------------------------------

fn run_manual_cascade(
    manifest_path: &Path,
    service: &str,
    changed: &[String],
    max_cycles: usize,
    dry_run: bool,
) -> Result<()> {
    // 1. Resolve universe graph
    let graph = forge_core::universe_graph::resolve_universe_graph(manifest_path)
        .map_err(|errs| anyhow::anyhow!("failed to resolve universe graph:\n{}", errs.join("\n")))?;

    // 2. Build delta
    let delta = forge_core::universe_graph::ServiceDelta {
        service_id: service.to_string(),
        changed_interfaces: changed.to_vec(),
    };

    // 3. Create cascade plan
    let manifest_dir = manifest_path.parent().unwrap();
    let config = forge_core::cascade::CascadeConfig {
        max_repair_cycles: max_cycles,
        dry_run,
        enable_clippy: false, // Disabled: clippy -D warnings on dependency crates is too noisy for cascade
        ..Default::default()
    };
    let mut plan = forge_core::cascade::create_plan(&graph, &delta, manifest_dir, config);

    // 4. Print plan
    println!("Cascade Plan: {} phases", plan.phases.len());
    for (i, phase) in plan.phases.iter().enumerate() {
        println!("  {}. {} (depth: {})", i + 1, phase.service_id, phase.depth);
    }

    if plan.phases.is_empty() {
        println!("No services affected. Nothing to do.");
        return Ok(());
    }

    // 5. Execute
    let report = forge_core::cascade::execute_plan(&mut plan);

    // 5.5. Save repair log (non-dry-run only)
    if !dry_run {
        let record = forge_core::repair_log::record_from_report(&report);
        let log_dir = manifest_dir.join(".forge-logs");
        if let Err(e) = forge_core::repair_log::save_repair_record(&record, &log_dir) {
            eprintln!("[cascade] warning: failed to save repair log: {}", e);
        }
    }

    // 6. Print report
    print_cascade_report(&report);

    if report.hard_halted {
        anyhow::bail!(
            "Cascade HARD HALTED after {} repairs, {} failures",
            report.services_repaired,
            report.services_failed
        );
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Cascade: watch mode — Intent Sentinel (P2)
// ---------------------------------------------------------------------------

fn run_watch_mode(
    manifest_path: &Path,
    interval_secs: u64,
    max_cycles: usize,
    dry_run: bool,
) -> Result<()> {
    use std::sync::{Arc, atomic::{AtomicBool, AtomicU64, Ordering}};
    use std::collections::HashMap;

    println!("Intent Sentinel active -- watching every {}s", interval_secs);
    println!("Press Ctrl+C to stop\n");

    let manifest_dir = manifest_path.parent().unwrap();
    let manifest_path_buf = manifest_path.to_path_buf();

    // Epoch counter -- monotonically increasing
    let epoch = Arc::new(AtomicU64::new(0));
    // Each epoch gets its own independent cancel token.
    // The previous epoch's token stays signalled even after detach.
    let mut current_cancel: Option<Arc<AtomicBool>> = None;

    // Initial snapshot: record git HEAD hash for each service
    let graph = forge_core::universe_graph::resolve_universe_graph(manifest_path)
        .map_err(|errs| anyhow::anyhow!("failed to resolve universe graph:\n{}", errs.join("\n")))?;

    let mut last_hashes: HashMap<String, String> = HashMap::new();
    for svc in &graph.services {
        let svc_dir = manifest_dir.join(&svc.path);
        if let Ok(hash) = get_intent_hash(&svc_dir) {
            last_hashes.insert(svc.id.clone(), hash);
        }
    }

    let mut current_handle: Option<std::thread::JoinHandle<()>> = None;

    // Polling loop
    loop {
        std::thread::sleep(std::time::Duration::from_secs(interval_secs));

        // Re-resolve graph (might have new services)
        let graph = match forge_core::universe_graph::resolve_universe_graph(&manifest_path_buf) {
            Ok(g) => g,
            Err(errs) => {
                eprintln!("Warning: failed to resolve universe: {}", errs.join("; "));
                continue;
            }
        };

        // Check for changes -- process one at a time
        let mut changed_service = None;
        for svc in &graph.services {
            let svc_dir = manifest_dir.join(&svc.path);
            let current_hash = match get_intent_hash(&svc_dir) {
                Ok(h) => h,
                Err(_) => continue,
            };

            let is_changed = match last_hashes.get(&svc.id) {
                Some(prev) if prev != &current_hash => true,
                None => true,
                _ => false,
            };

            if is_changed {
                last_hashes.insert(svc.id.clone(), current_hash);
                changed_service = Some(svc.id.clone());
                break; // Process one change at a time
            }
        }

        if let Some(service_id) = changed_service {
            // Bump epoch -- this signals any running cascade to abort
            let new_epoch = epoch.fetch_add(1, Ordering::SeqCst) + 1;

            // Signal cancellation to the OLD epoch's token (if any).
            // Each epoch has its own independent token, so resetting
            // for the new epoch does NOT clear the old one.
            if let Some(ref old_cancel) = current_cancel {
                old_cancel.store(true, Ordering::SeqCst);
            }

            // Give the running thread a moment to notice cancellation
            if let Some(handle) = current_handle.take() {
                println!("[Sentinel] Epoch {} -- cancelling previous cascade...", new_epoch);
                let join_timeout = std::time::Duration::from_secs(5);
                let start = std::time::Instant::now();
                loop {
                    if handle.is_finished() {
                        let _ = handle.join();
                        break;
                    }
                    if start.elapsed() > join_timeout {
                        // Old thread keeps its cancel token signalled — it will
                        // abort at the next check point and rollback via 2PC.
                        eprintln!("[Sentinel] previous cascade did not stop in 5s, detaching (cancel token stays active)");
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
            }

            // Create a FRESH cancel token for the new epoch
            let new_cancel = Arc::new(AtomicBool::new(false));
            current_cancel = Some(new_cancel.clone());

            let now = format_system_time_hms();
            println!("\n[Sentinel] Epoch {} -- change detected in '{}' ({})", new_epoch, service_id, now);

            // Spawn new cascade with its own independent cancel token
            let manifest_buf = manifest_path_buf.clone();
            let manifest_dir_buf = manifest_dir.to_path_buf();
            let cancel = new_cancel;
            let epoch_at_start = new_epoch;
            let epoch_ref = epoch.clone();

            current_handle = Some(std::thread::spawn(move || {
                let graph = match forge_core::universe_graph::resolve_universe_graph(&manifest_buf) {
                    Ok(g) => g,
                    Err(_) => return,
                };

                let delta = forge_core::universe_graph::ServiceDelta {
                    service_id: service_id.clone(),
                    changed_interfaces: vec![],  // empty = affect all direct consumers
                };

                let config = forge_core::cascade::CascadeConfig {
                    max_repair_cycles: max_cycles,
                    dry_run,
                    enable_clippy: false,
                    ..Default::default()
                };

                let mut plan = forge_core::cascade::create_plan(
                    &graph, &delta, &manifest_dir_buf, config,
                );

                if plan.phases.is_empty() {
                    println!("[Sentinel] Epoch {} -- no services affected", epoch_at_start);
                    return;
                }

                println!("[Sentinel] Epoch {} -- cascade plan: {} phases", epoch_at_start, plan.phases.len());

                // Check cancellation before executing
                if cancel.load(Ordering::SeqCst) || epoch_ref.load(Ordering::SeqCst) != epoch_at_start {
                    println!("[Sentinel] Epoch {} -- PREEMPTED by newer epoch", epoch_at_start);
                    return;
                }

                let cancel_token: forge_core::cascade::CancelToken = cancel.clone();
                let report = forge_core::cascade::execute_plan_with_cancel(
                    &mut plan, Some(&cancel_token),
                );

                // Check if we were preempted during execution
                if cancel.load(Ordering::SeqCst) || epoch_ref.load(Ordering::SeqCst) != epoch_at_start {
                    println!("[Sentinel] Epoch {} -- PREEMPTED (cascade aborted mid-execution)", epoch_at_start);
                    return;
                }

                // Save repair log (non-dry-run only)
                if !dry_run {
                    let record = forge_core::repair_log::record_from_report(&report);
                    let log_dir = manifest_dir_buf.join(".forge-logs");
                    if let Err(e) = forge_core::repair_log::save_repair_record(&record, &log_dir) {
                        eprintln!("[cascade] warning: failed to save repair log: {}", e);
                    }
                }

                // Print report inline (cannot call print_cascade_report from spawned thread)
                println!("\n--- Cascade Report (Epoch {}) ---", epoch_at_start);
                println!("Duration: {:.1}s", report.total_duration.as_secs_f64());
                println!(
                    "Repaired: {}/{}",
                    report.services_repaired,
                    report.plan.phases.len()
                );
                for phase in &report.plan.phases {
                    let icon = match &phase.status {
                        forge_core::cascade::PhaseStatus::Passed => "[OK]",
                        forge_core::cascade::PhaseStatus::Failed(_) => "[FAIL]",
                        forge_core::cascade::PhaseStatus::HardHalt(_) => "[HALT]",
                        forge_core::cascade::PhaseStatus::Skipped => "[SKIP]",
                        forge_core::cascade::PhaseStatus::Pending => "[....]",
                        forge_core::cascade::PhaseStatus::Running => "[....]",
                    };
                    let dur = phase
                        .duration
                        .map(|d| format!(" ({:.1}s)", d.as_secs_f64()))
                        .unwrap_or_default();
                    println!(
                        "  {} {} [attempts: {}]{}",
                        icon, phase.service_id, phase.repair_attempts, dur
                    );
                }
                if report.hard_halted {
                    println!("\n[HALT] Cascade aborted -- max repair cycles exceeded");
                }
            }));
        }
    }
}

/// Get a hash representing the current state of intent.lisp in a service directory.
///
/// Checks the actual file content (not git history), so unsaved/uncommitted
/// changes are detected immediately.  Falls back to git commit hash if no
/// intent.lisp file is found.
fn get_intent_hash(svc_dir: &Path) -> Result<String> {
    // Look for intent.lisp in known locations
    let intent_path = [".missiond/intent.lisp", ".jarvis/intent.lisp", "intent.lisp"]
        .iter()
        .map(|p| svc_dir.join(p))
        .find(|p| p.exists());

    if let Some(path) = intent_path {
        // Hash the file content directly — detects uncommitted edits
        let content = std::fs::read(&path)
            .with_context(|| format!("cannot read {}", path.display()))?;

        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        content.hash(&mut hasher);
        Ok(format!("{:016x}", hasher.finish()))
    } else {
        // Fallback: use git commit hash for directories without intent.lisp
        let output = std::process::Command::new("git")
            .args(["log", "-1", "--format=%H", "--", "."])
            .current_dir(svc_dir)
            .output()
            .with_context(|| format!("git log failed in {}", svc_dir.display()))?;

        let hash = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if hash.is_empty() {
            anyhow::bail!("no git commits found in {}", svc_dir.display());
        }
        Ok(hash)
    }
}

/// Format current time as HH:MM:SS using std::time::SystemTime (no chrono).
fn format_system_time_hms() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // UTC time components
    let s = secs % 60;
    let m = (secs / 60) % 60;
    let h = (secs / 3600) % 24;
    format!("{:02}:{:02}:{:02} UTC", h, m, s)
}

// ---------------------------------------------------------------------------
// Cascade report printer
// ---------------------------------------------------------------------------

fn print_cascade_report(report: &forge_core::cascade::CascadeReport) {
    println!("\n--- Cascade Report ---");
    println!("Duration: {:.1}s", report.total_duration.as_secs_f64());
    println!(
        "Repaired: {}/{}",
        report.services_repaired,
        report.plan.phases.len()
    );

    for phase in &report.plan.phases {
        let icon = match &phase.status {
            forge_core::cascade::PhaseStatus::Passed => "[OK]",
            forge_core::cascade::PhaseStatus::Failed(_) => "[FAIL]",
            forge_core::cascade::PhaseStatus::HardHalt(_) => "[HALT]",
            forge_core::cascade::PhaseStatus::Skipped => "[SKIP]",
            forge_core::cascade::PhaseStatus::Pending => "[....]",
            forge_core::cascade::PhaseStatus::Running => "[....]",
        };
        let dur = phase
            .duration
            .map(|d| format!(" ({:.1}s)", d.as_secs_f64()))
            .unwrap_or_default();
        println!(
            "  {} {} [attempts: {}]{}",
            icon, phase.service_id, phase.repair_attempts, dur
        );
    }

    if report.hard_halted {
        println!("\n[HALT] Cascade aborted -- max repair cycles exceeded");
    }
}
