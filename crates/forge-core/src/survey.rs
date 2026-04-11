//! Survey — scan a project's codebase and generate standardized intent.lisp via LLM.
//!
//! Three-phase pipeline:
//!   Phase 1 (no LLM): AST scan → file listing + symbol extraction + survey hash
//!   Phase 2 (LLM):    structured prompt + claude -p → intent.lisp draft
//!   Phase 3 (validate): sexp_parser validation + structural checks

use crate::scanner;
use crate::sexp_parser::SexpNode;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

// ---------------------------------------------------------------------------
// Embedded prompt templates (compile-time)
// ---------------------------------------------------------------------------

const CARTOGRAPHER_PROMPT: &str = include_str!("../../../prompts/cartographer.md");
const L1_TEMPLATE: &str = include_str!("../../../prompts/templates/l1-blueprint.lisp");
const L2_TEMPLATE: &str = include_str!("../../../prompts/templates/l2-topology.lisp");
const L3_TEMPLATE: &str = include_str!("../../../prompts/templates/l3-implementation.lisp");

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SurveyLevel {
    L1Blueprint,
    L2Topology,
    L3Implementation,
}

impl SurveyLevel {
    pub fn label(&self) -> &'static str {
        match self {
            Self::L1Blueprint => "L1-Blueprint",
            Self::L2Topology => "L2-Topology",
            Self::L3Implementation => "L3-Implementation",
        }
    }

    pub fn template(&self) -> &'static str {
        match self {
            Self::L1Blueprint => L1_TEMPLATE,
            Self::L2Topology => L2_TEMPLATE,
            Self::L3Implementation => L3_TEMPLATE,
        }
    }

    /// Max symbols to include in the prompt context.
    pub fn symbol_budget(&self) -> usize {
        match self {
            Self::L1Blueprint => 80,
            Self::L2Topology => 250,
            Self::L3Implementation => 500,
        }
    }

    /// Max file listing entries.
    pub fn file_budget(&self) -> usize {
        match self {
            Self::L1Blueprint => 200,
            Self::L2Topology => 500,
            Self::L3Implementation => 800,
        }
    }
}

pub struct SurveyConfig {
    pub project_root: PathBuf,
    pub level: SurveyLevel,
    pub check_only: bool,
    pub dry_run: bool,
}

pub struct SurveyResult {
    pub intent_lisp: String,
    pub output_path: PathBuf,
    pub survey_hash: String,
    pub survey_date: String,
    pub file_count: usize,
    pub symbol_count: usize,
    pub validation_ok: bool,
    pub stale: Option<bool>,
}

// Phase 1 intermediate
struct ScanData {
    files: Vec<FileEntry>,
    symbols: Vec<scanner::CodeNode>,
    language: String,
    survey_hash: String,
}

#[derive(Debug)]
struct FileEntry {
    rel_path: String,
    size: u64,
}

// ---------------------------------------------------------------------------
// Main entry point
// ---------------------------------------------------------------------------

pub fn run(config: &SurveyConfig) -> Result<SurveyResult, String> {
    let now = chrono_lite_now();

    // Phase 1: scan
    let scan = phase1_scan(&config.project_root)?;

    // --check mode: just compare hashes
    if config.check_only {
        let stale = check_staleness(&config.project_root, &scan.survey_hash)?;
        return Ok(SurveyResult {
            intent_lisp: String::new(),
            output_path: resolve_output_dir(&config.project_root).join("intent.lisp"),
            survey_hash: scan.survey_hash,
            survey_date: now,
            file_count: scan.files.len(),
            symbol_count: scan.symbols.len(),
            validation_ok: true,
            stale: Some(stale),
        });
    }

    // Phase 2: generate via LLM
    let prompt = build_prompt(&scan, config, &now);

    if config.dry_run {
        return Ok(SurveyResult {
            intent_lisp: prompt,
            output_path: resolve_output_dir(&config.project_root).join("intent.lisp"),
            survey_hash: scan.survey_hash,
            survey_date: now,
            file_count: scan.files.len(),
            symbol_count: scan.symbols.len(),
            validation_ok: true,
            stale: None,
        });
    }

    let raw_output = call_claude(&prompt)?;
    let lisp_text = strip_markdown_fences(&raw_output);

    if lisp_text.trim().is_empty() {
        return Err("claude returned empty S-EXP content after stripping. Raw output may have been non-lisp.".into());
    }

    // Phase 3: validate
    let validation_ok = phase3_validate(&lisp_text);

    // Write output
    let output_dir = resolve_output_dir(&config.project_root);
    let output_path = output_dir.join("intent.lisp");
    std::fs::create_dir_all(&output_dir)
        .map_err(|e| format!("failed to create {}: {}", output_dir.display(), e))?;
    std::fs::write(&output_path, &lisp_text)
        .map_err(|e| format!("failed to write {}: {}", output_path.display(), e))?;

    Ok(SurveyResult {
        intent_lisp: lisp_text,
        output_path,
        survey_hash: scan.survey_hash,
        survey_date: now,
        file_count: scan.files.len(),
        symbol_count: scan.symbols.len(),
        validation_ok,
        stale: None,
    })
}

// ---------------------------------------------------------------------------
// Phase 1: Scan
// ---------------------------------------------------------------------------

fn phase1_scan(project_root: &Path) -> Result<ScanData, String> {
    // Collect file listing
    let files = collect_file_listing(project_root)?;

    // AST symbol extraction via existing scanner
    let language = scanner::detect_language(project_root);
    let symbols = scanner::auto_scan(project_root, &language)
        .map_err(|e| format!("AST scan failed: {}", e))?;

    // Compute deterministic hash from file listing
    let survey_hash = compute_survey_hash(&files);

    Ok(ScanData {
        files,
        symbols,
        language,
        survey_hash,
    })
}

fn collect_file_listing(project_root: &Path) -> Result<Vec<FileEntry>, String> {
    use ignore::WalkBuilder;

    let mut entries = Vec::new();
    let walker = WalkBuilder::new(project_root)
        .hidden(true)        // skip dotfiles
        .git_ignore(true)    // respect .gitignore
        .git_global(false)
        .build();

    for result in walker {
        let entry = match result {
            Ok(e) => e,
            Err(_) => continue,
        };
        if !entry.file_type().map_or(false, |ft| ft.is_file()) {
            continue;
        }
        let path = entry.path();

        // Skip common noise
        let path_str = path.to_string_lossy();
        if path_str.contains("/target/")
            || path_str.contains("/node_modules/")
            || path_str.contains("/.git/")
        {
            continue;
        }

        let rel_path = path
            .strip_prefix(project_root)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();

        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);

        entries.push(FileEntry { rel_path, size });
    }

    entries.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    Ok(entries)
}

fn compute_survey_hash(files: &[FileEntry]) -> String {
    let mut hasher = DefaultHasher::new();
    for f in files {
        f.rel_path.hash(&mut hasher);
        f.size.hash(&mut hasher);
    }
    format!("{:016x}", hasher.finish())
}

// ---------------------------------------------------------------------------
// Phase 2: Prompt construction + LLM call
// ---------------------------------------------------------------------------

fn build_prompt(scan: &ScanData, config: &SurveyConfig, now: &str) -> String {
    let level = config.level;
    let project_name = config
        .project_root
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();

    // File tree (capped)
    let file_budget = level.file_budget();
    let file_tree: String = scan
        .files
        .iter()
        .take(file_budget)
        .map(|f| format!("  {} ({}B)", f.rel_path, f.size))
        .collect::<Vec<_>>()
        .join("\n");
    let file_overflow = if scan.files.len() > file_budget {
        format!("\n  ... and {} more files", scan.files.len() - file_budget)
    } else {
        String::new()
    };

    // Symbol table (capped, exported first)
    let symbol_budget = level.symbol_budget();
    let mut syms: Vec<&scanner::CodeNode> = scan.symbols.iter().collect();
    syms.sort_by(|a, b| {
        b.is_exported
            .cmp(&a.is_exported)
            .then(a.file_path.cmp(&b.file_path))
            .then(a.start_line.cmp(&b.start_line))
    });
    let symbol_table: String = syms
        .iter()
        .take(symbol_budget)
        .map(|s| {
            let export = if s.is_exported { "pub" } else { "   " };
            format!(
                "  [{export}] {typ:8} {name:40} {file}:{start}",
                export = export,
                typ = s.node_type,
                name = s.name,
                file = s.file_path,
                start = s.start_line,
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let symbol_overflow = if scan.symbols.len() > symbol_budget {
        format!(
            "\n  ... and {} more symbols",
            scan.symbols.len() - symbol_budget
        )
    } else {
        String::new()
    };

    // Check for existing intent.lisp
    let existing_intent = read_existing_intent(&config.project_root);
    let existing_section = if let Some(ref content) = existing_intent {
        format!(
            "\n## Existing intent.lisp (may be outdated, use as reference)\n\n```lisp\n{}\n```\n",
            content
        )
    } else {
        String::new()
    };

    format!(
        r#"{cartographer}

---

## Project Survey Data

**Project**: {project_name}
**Language**: {language}
**Files**: {file_count}
**Symbols**: {symbol_count}

### File Tree
{file_tree}{file_overflow}

### Code Symbols (top {symbol_budget}, sorted by export status)
{symbol_table}{symbol_overflow}
{existing_section}
### Template ({level_label})
```lisp
{template}
```

## Task

Generate a SINGLE intent.lisp file at **{level_label}** granularity for the project **{project_name}**.

Requirements:
1. Include `(survey-hash "{hash}")` and `(survey-date "{date}")` as top-level children of `(intent ...)`.
2. Include `(upstream-dependency ...)` blocks if this project depends on other services.
3. Use `(pillar ...)` to group related modules. Each pillar should have `(purpose ...)` and `(component ...)` children.
4. Each `(component ...)` should have `:target` pointing to the real file path (relative to project root).
5. Include `(flows ...)` section describing the main data flows.
6. If an existing intent.lisp is provided above, preserve accurate design-constraints and upstream-dependency declarations — but UPDATE component/file mappings to match the ACTUAL code scan results.
7. Output ONLY the raw S-EXP content. No markdown fences, no explanation, no commentary.
"#,
        cartographer = CARTOGRAPHER_PROMPT,
        project_name = project_name,
        language = scan.language,
        file_count = scan.files.len(),
        symbol_count = scan.symbols.len(),
        file_tree = file_tree,
        file_overflow = file_overflow,
        symbol_budget = symbol_budget,
        symbol_table = symbol_table,
        symbol_overflow = symbol_overflow,
        existing_section = existing_section,
        level_label = level.label(),
        template = level.template(),
        hash = scan.survey_hash,
        date = now,
    )
}

fn call_claude(prompt: &str) -> Result<String, String> {
    // Check claude is available
    let which = Command::new("which")
        .arg("claude")
        .output()
        .map_err(|e| format!("cannot check for claude: {}", e))?;
    if !which.status.success() {
        return Err("claude CLI not found in PATH. Install: https://docs.anthropic.com/en/docs/claude-code".into());
    }

    // Write prompt to a temp file to avoid stdin pipe deadlock on large prompts
    let temp_dir = std::env::temp_dir();
    let prompt_file = temp_dir.join("forge-survey-prompt.txt");
    std::fs::write(&prompt_file, prompt)
        .map_err(|e| format!("failed to write temp prompt file: {}", e))?;

    let prompt_content = std::fs::read_to_string(&prompt_file)
        .map_err(|e| format!("failed to read prompt file: {}", e))?;

    let mut child = Command::new("claude")
        .args(["-p", "--output-format", "text"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to spawn claude: {}", e))?;

    // Write prompt to stdin in a separate thread to avoid deadlock
    let stdin = child.stdin.take().ok_or("failed to get claude stdin")?;
    let prompt_clone = prompt_content;
    let write_thread = std::thread::spawn(move || {
        use std::io::Write;
        let mut stdin = stdin;
        let _ = stdin.write_all(prompt_clone.as_bytes());
        let _ = stdin.flush();
        drop(stdin); // close stdin to signal EOF
    });

    let output = child
        .wait_with_output()
        .map_err(|e| format!("failed to wait for claude: {}", e))?;

    let _ = write_thread.join();
    let _ = std::fs::remove_file(&prompt_file);

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "claude exited with code {}: {}",
            output.status.code().unwrap_or(-1),
            stderr.trim()
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    if stdout.trim().is_empty() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("claude returned empty output. stderr: {}", stderr.trim()));
    }

    Ok(stdout)
}

fn strip_markdown_fences(text: &str) -> String {
    let trimmed = text.trim();

    // Strip ```lisp ... ``` or ```sexp ... ``` or just ``` ... ```
    if let Some(rest) = trimmed.strip_prefix("```") {
        let after_tag = if let Some(pos) = rest.find('\n') {
            &rest[pos + 1..]
        } else {
            rest
        };
        if let Some(content) = after_tag.strip_suffix("```") {
            return strip_preamble(content.trim());
        }
        return strip_preamble(after_tag.trim());
    }

    strip_preamble(trimmed)
}

/// LLMs sometimes emit natural language before the S-EXP.
/// Skip to the first `;;` comment or `(` paren.
fn strip_preamble(text: &str) -> String {
    // Already starts with sexp content
    if text.starts_with('(') || text.starts_with(";;") {
        return text.to_string();
    }

    // Find the first line that starts with ;; or (
    for (i, line) in text.lines().enumerate() {
        let lt = line.trim_start();
        if lt.starts_with('(') || lt.starts_with(";;") {
            let remaining: String = text.lines().skip(i).collect::<Vec<_>>().join("\n");
            return remaining;
        }
    }

    // Fallback: return as-is
    text.to_string()
}

// ---------------------------------------------------------------------------
// Phase 3: Validate
// ---------------------------------------------------------------------------

fn phase3_validate(lisp_text: &str) -> bool {
    let parsed = match SexpNode::parse_one(lisp_text) {
        Ok(node) => node,
        Err(_) => return false,
    };

    // Check top-level is (intent ...)
    if parsed.tag() != Some("intent") {
        return false;
    }

    // Check (granularity ...) exists
    let has_granularity = parsed
        .children()
        .iter()
        .any(|c| c.tag() == Some("granularity"));

    // Check (survey-hash ...) exists
    let has_hash = parsed
        .children()
        .iter()
        .any(|c| c.tag() == Some("survey-hash"));

    has_granularity && has_hash
}

// ---------------------------------------------------------------------------
// --check mode: staleness detection
// ---------------------------------------------------------------------------

fn check_staleness(project_root: &Path, current_hash: &str) -> Result<bool, String> {
    let output_dir = resolve_output_dir(project_root);
    let intent_path = output_dir.join("intent.lisp");

    if !intent_path.exists() {
        // No intent.lisp at all — definitely stale
        return Ok(true);
    }

    let content = std::fs::read_to_string(&intent_path)
        .map_err(|e| format!("failed to read {}: {}", intent_path.display(), e))?;

    // Parse and extract survey-hash
    let parsed = SexpNode::parse_one(&content)
        .map_err(|e| format!("failed to parse existing intent.lisp: {}", e))?;

    let stored_hash = parsed
        .children()
        .iter()
        .find(|c| c.tag() == Some("survey-hash"))
        .and_then(|c| c.children().get(1))
        .and_then(|c| c.as_atom())
        .map(|s| s.to_string());

    match stored_hash {
        Some(h) => Ok(h != current_hash),
        None => Ok(true), // No survey-hash in file = stale
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn resolve_output_dir(project_root: &Path) -> PathBuf {
    // Prefer .missiond/ if it exists, otherwise create .missiond/
    let missiond = project_root.join(".missiond");
    let jarvis = project_root.join(".jarvis");
    if jarvis.exists() && !missiond.exists() {
        jarvis
    } else {
        missiond
    }
}

fn read_existing_intent(project_root: &Path) -> Option<String> {
    let candidates = [
        project_root.join(".missiond").join("intent.lisp"),
        project_root.join(".jarvis").join("intent.lisp"),
    ];
    for p in &candidates {
        if let Ok(content) = std::fs::read_to_string(p) {
            return Some(content);
        }
    }
    None
}

fn chrono_lite_now() -> String {
    // Minimal UTC timestamp without pulling in chrono crate
    let output = Command::new("date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .output()
        .ok();
    output
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "2026-01-01T00:00:00Z".to_string())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_markdown_fences_lisp() {
        let input = "```lisp\n(intent foo)\n```";
        assert_eq!(strip_markdown_fences(input), "(intent foo)");
    }

    #[test]
    fn test_strip_markdown_fences_plain() {
        let input = "```\n(intent foo)\n```";
        assert_eq!(strip_markdown_fences(input), "(intent foo)");
    }

    #[test]
    fn test_strip_markdown_fences_none() {
        let input = "(intent foo)";
        assert_eq!(strip_markdown_fences(input), "(intent foo)");
    }

    #[test]
    fn test_compute_survey_hash_deterministic() {
        let files = vec![
            FileEntry { rel_path: "src/main.rs".into(), size: 100 },
            FileEntry { rel_path: "src/lib.rs".into(), size: 200 },
        ];
        let h1 = compute_survey_hash(&files);
        let h2 = compute_survey_hash(&files);
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 16); // 16 hex chars
    }

    #[test]
    fn test_compute_survey_hash_changes_on_mutation() {
        let files1 = vec![
            FileEntry { rel_path: "src/main.rs".into(), size: 100 },
        ];
        let files2 = vec![
            FileEntry { rel_path: "src/main.rs".into(), size: 101 },
        ];
        assert_ne!(compute_survey_hash(&files1), compute_survey_hash(&files2));
    }

    #[test]
    fn test_phase3_validate_valid() {
        let lisp = r#"(intent test-project
  (granularity L2-Topology)
  (survey-hash "abc123")
  (survey-date "2026-04-06"))"#;
        assert!(phase3_validate(lisp));
    }

    #[test]
    fn test_phase3_validate_missing_granularity() {
        let lisp = r#"(intent test-project
  (survey-hash "abc123"))"#;
        assert!(!phase3_validate(lisp));
    }

    #[test]
    fn test_phase3_validate_missing_hash() {
        let lisp = r#"(intent test-project
  (granularity L2-Topology))"#;
        assert!(!phase3_validate(lisp));
    }

    #[test]
    fn test_phase3_validate_not_intent() {
        let lisp = r#"(something-else foo)"#;
        assert!(!phase3_validate(lisp));
    }

    #[test]
    fn test_strip_preamble_with_commentary() {
        let input = "Here is the generated intent.lisp:\n\n;; header\n(intent foo)";
        assert_eq!(strip_markdown_fences(input), ";; header\n(intent foo)");
    }

    #[test]
    fn test_strip_preamble_with_paren() {
        let input = "Sure, here you go:\n(intent foo)";
        assert_eq!(strip_markdown_fences(input), "(intent foo)");
    }

    #[test]
    fn test_strip_preamble_clean() {
        let input = "(intent foo\n  (granularity L3))";
        assert_eq!(strip_markdown_fences(input), input);
    }

    #[test]
    fn test_level_labels() {
        assert_eq!(SurveyLevel::L1Blueprint.label(), "L1-Blueprint");
        assert_eq!(SurveyLevel::L2Topology.label(), "L2-Topology");
        assert_eq!(SurveyLevel::L3Implementation.label(), "L3-Implementation");
    }
}
