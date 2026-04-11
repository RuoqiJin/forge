//! Forge Audit — three-state implementation status scanner.
//!
//! Scans a Forge-managed project and cross-references:
//!   1. Custom method declarations from `.missiond/*.lisp` IR
//!   2. AST-extracted functions from the project source tree
//!
//! Reports each custom method as:
//!   - 🟢 Implemented: function exists, body has no `todo!()`/`unimplemented!()`
//!   - 🟡 Pending: function exists, body contains `todo!()`/`unimplemented!()`
//!   - 🔴 Missing: declared in Lisp IR but not found in source

use forge_core::scanner::{self, CodeNode};
use nc_parser::SexpNode;
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub enum AuditStatus {
    Implemented { lines: u32 },
    Pending,
    Missing,
}

#[derive(Debug, Clone, Serialize)]
pub struct AuditEntry {
    pub method: String,
    pub status: AuditStatus,
    pub file: Option<String>,
    pub start_line: Option<u32>,
    pub end_line: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PatternAudit {
    pub pattern: String,
    pub source_file: String,
    pub entries: Vec<AuditEntry>,
    pub implemented: usize,
    pub pending: usize,
    pub missing: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectAudit {
    pub project: String,
    pub patterns: Vec<PatternAudit>,
    pub total_implemented: usize,
    pub total_pending: usize,
    pub total_missing: usize,
}

// ---------------------------------------------------------------------------
// Core audit logic
// ---------------------------------------------------------------------------

pub fn audit_project(project_root: &Path) -> anyhow::Result<ProjectAudit> {
    let missiond = project_root.join(".missiond");
    if !missiond.exists() {
        anyhow::bail!("No .missiond/ directory found at {}", project_root.display());
    }

    // 1. Collect custom method declarations from each pattern's Lisp IR
    let mut pattern_specs: Vec<(String, String, Vec<String>)> = Vec::new();

    let mut lisp_files: Vec<std::path::PathBuf> = std::fs::read_dir(&missiond)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("lisp"))
        .collect();
    lisp_files.sort();

    for path in &lisp_files {
        let filename = path.file_name().unwrap().to_string_lossy();
        if filename.as_ref() == "intent.lisp" {
            continue;
        }

        let src = std::fs::read_to_string(path)?;
        let nodes = SexpNode::parse(&src)
            .map_err(|e| anyhow::anyhow!("Failed to parse {}: {}", filename, e))?;

        for node in &nodes {
            let tag = match node.tag() {
                Some(t) => t,
                None => continue,
            };

            let customs = extract_custom_methods(tag, node);
            if customs.is_empty() {
                continue;
            }

            pattern_specs.push((
                tag.to_string(),
                filename.to_string(),
                customs,
            ));
        }
    }

    // 2. Scan project source tree for all code symbols
    let code_nodes = scanner::auto_scan(project_root, "auto")?;

    // 3. Build function name -> CodeNode lookup (functions only)
    let fn_map: HashMap<&str, &CodeNode> = code_nodes
        .iter()
        .filter(|n| n.node_type == "function")
        .map(|n| (n.name.as_str(), n))
        .collect();

    // 4. Build source text cache for todo!() detection
    //    (keyed by relative file path -> file content)
    let mut source_cache: HashMap<String, String> = HashMap::new();

    // 5. Cross-reference: for each custom method, find matching CodeNode
    let mut project = ProjectAudit {
        project: project_root
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string(),
        patterns: Vec::new(),
        total_implemented: 0,
        total_pending: 0,
        total_missing: 0,
    };

    for (pattern_tag, source_file, custom_methods) in &pattern_specs {
        let mut audit = PatternAudit {
            pattern: pattern_tag.clone(),
            source_file: source_file.clone(),
            entries: Vec::new(),
            implemented: 0,
            pending: 0,
            missing: 0,
        };

        for method in custom_methods {
            // Try both `custom_xxx` (scaffold name) and `xxx` (original name)
            let search_names = [format!("custom_{}", method), method.clone()];

            let found = search_names
                .iter()
                .find_map(|name| fn_map.get(name.as_str()).copied());

            match found {
                Some(node) => {
                    // Check if function body contains todo!() or unimplemented!()
                    let has_todo = check_for_todo_macro(
                        project_root,
                        &node.file_path,
                        node.start_line,
                        node.end_line,
                        &mut source_cache,
                    );

                    let lines = node.end_line.saturating_sub(node.start_line) + 1;

                    if has_todo {
                        audit.entries.push(AuditEntry {
                            method: method.clone(),
                            status: AuditStatus::Pending,
                            file: Some(node.file_path.clone()),
                            start_line: Some(node.start_line),
                            end_line: Some(node.end_line),
                        });
                        audit.pending += 1;
                    } else {
                        audit.entries.push(AuditEntry {
                            method: method.clone(),
                            status: AuditStatus::Implemented { lines },
                            file: Some(node.file_path.clone()),
                            start_line: Some(node.start_line),
                            end_line: Some(node.end_line),
                        });
                        audit.implemented += 1;
                    }
                }
                None => {
                    audit.entries.push(AuditEntry {
                        method: method.clone(),
                        status: AuditStatus::Missing,
                        file: None,
                        start_line: None,
                        end_line: None,
                    });
                    audit.missing += 1;
                }
            }
        }

        project.total_implemented += audit.implemented;
        project.total_pending += audit.pending;
        project.total_missing += audit.missing;
        project.patterns.push(audit);
    }

    Ok(project)
}

// ---------------------------------------------------------------------------
// Custom method extraction per IR pattern
// ---------------------------------------------------------------------------

fn extract_custom_methods(tag: &str, node: &SexpNode) -> Vec<String> {
    match tag {
        "s3-dispatcher" => {
            match nc_jarvis_ir::s3_dispatcher::S3DispatcherDecl::from_sexp(node) {
                Ok(decl) => decl.customs.iter().map(|c| c.method.clone()).collect(),
                Err(_) => vec![],
            }
        }
        "s3-response-builder" => {
            match nc_jarvis_ir::s3_response_builder::S3ResponseBuilderDecl::from_sexp(node) {
                Ok(decl) => decl
                    .builders
                    .iter()
                    .filter(|b| {
                        matches!(
                            b.kind,
                            nc_jarvis_ir::s3_response_builder::BuilderKind::Custom
                        )
                    })
                    .map(|b| b.name.clone())
                    .collect(),
                Err(_) => vec![],
            }
        }
        "error-enum" => match nc_jarvis_ir::error_enum::ErrorEnumDecl::from_sexp(node) {
            Ok(decl) => decl.custom_fns.clone(),
            Err(_) => vec![],
        },
        // Patterns without explicit custom declarations return empty
        _ => vec![],
    }
}

// ---------------------------------------------------------------------------
// todo!() / unimplemented!() detection via source text
// ---------------------------------------------------------------------------

/// Check if a function body (identified by file path and line range) contains
/// `todo!()` or `unimplemented!()`. We read the actual source text rather than
/// relying on the scanner's `calls` field, because tree-sitter classifies these
/// as `macro_invocation` nodes, not `call_expression` nodes.
fn check_for_todo_macro(
    project_root: &Path,
    rel_file_path: &str,
    start_line: u32,
    end_line: u32,
    cache: &mut HashMap<String, String>,
) -> bool {
    let source = cache
        .entry(rel_file_path.to_string())
        .or_insert_with(|| {
            let abs_path = project_root.join(rel_file_path);
            std::fs::read_to_string(&abs_path).unwrap_or_default()
        });

    if source.is_empty() {
        return false;
    }

    // Extract lines in the function body range (1-indexed)
    let lines: Vec<&str> = source.lines().collect();
    let start = (start_line as usize).saturating_sub(1);
    let end = (end_line as usize).min(lines.len());

    for line in &lines[start..end] {
        let trimmed = line.trim();
        if trimmed.contains("todo!()") || trimmed.contains("unimplemented!()") {
            return true;
        }
    }

    false
}
