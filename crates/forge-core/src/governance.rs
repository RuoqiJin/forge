//! Governance — lint rule engine for intent granularity enforcement.
//!
//! Validates intent files against their declared granularity level,
//! detecting forbidden tags, structural violations, and suggesting splits.

use crate::intent_graph::{GovernanceMode, GranularityLevel, IntentFileMeta, IntentGraph};
use crate::sexp_parser::SexpNode;
use serde::Serialize;

// ============================================================================
// Violation types
// ============================================================================

#[derive(Debug, Clone, Serialize)]
pub enum ViolationSeverity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone, Serialize)]
pub struct GranularityViolation {
    pub file: String,
    pub line: Option<usize>,
    pub rule: String,
    pub severity: ViolationSeverity,
    pub message: String,
    pub suggestion: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SplitSuggestion {
    pub current_lines: usize,
    pub table_count: usize,
    pub api_count: usize,
    pub flow_count: usize,
    pub suggested_files: Vec<(String, String)>,
}

// ============================================================================
// Content lint — tag & atom checks per granularity
// ============================================================================

/// Forbidden tags per level.
const L1_FORBIDDEN_TAGS: &[&str] = &["col", "op", "function"];
const L2_FORBIDDEN_TAGS: &[&str] = &["col", "op"];

/// Lint the content of a single intent file against its declared granularity.
///
/// - Descriptive governance-mode: skip granularity checks, warn on codegen tags
/// - L1 forbids: col, op, function tags and URL atoms
/// - L2 forbids: col, op tags
/// - L3: no content restrictions
pub fn lint_content(meta: &IntentFileMeta, nodes: &[SexpNode]) -> Vec<GranularityViolation> {
    let mut violations = Vec::new();

    // Descriptive files: skip granularity content checks, but warn on codegen tags
    if meta.governance_mode == Some(GovernanceMode::Descriptive) {
        for node in nodes {
            check_descriptive_codegen_leak(
                node,
                &meta.file_path.to_string_lossy(),
                &mut violations,
            );
        }
        return violations;
    }

    match meta.level {
        GranularityLevel::L3Implementation => {
            // No content restrictions for L3
            return violations;
        }
        GranularityLevel::L1Blueprint => {
            // Check forbidden tags
            for node in nodes {
                check_forbidden_tags(
                    node,
                    L1_FORBIDDEN_TAGS,
                    &meta.file_path.to_string_lossy(),
                    meta.level,
                    &mut violations,
                );
            }
            // Check for URL patterns in atoms
            for node in nodes {
                walk_atoms(node, &mut |val| {
                    if val.contains("http://") || val.contains("https://") {
                        violations.push(GranularityViolation {
                            file: meta.file_path.to_string_lossy().to_string(),
                            line: None,
                            rule: "L1-no-urls".into(),
                            severity: ViolationSeverity::Error,
                            message: format!(
                                "L1-Blueprint must not contain URLs in atoms: '{}'",
                                truncate(val, 60)
                            ),
                            suggestion: Some(
                                "Move URL references to an L2 or L3 file".into(),
                            ),
                        });
                    }
                });
            }
        }
        GranularityLevel::L2Topology => {
            for node in nodes {
                check_forbidden_tags(
                    node,
                    L2_FORBIDDEN_TAGS,
                    &meta.file_path.to_string_lossy(),
                    meta.level,
                    &mut violations,
                );
            }
        }
    }

    violations
}

/// Recursively walk the S-expression tree checking for forbidden tag names.
fn check_forbidden_tags(
    node: &SexpNode,
    forbidden: &[&str],
    file: &str,
    level: GranularityLevel,
    violations: &mut Vec<GranularityViolation>,
) {
    match node {
        SexpNode::Atom(_) => {}
        SexpNode::List(children) => {
            if let Some(tag) = node.tag() {
                if forbidden.contains(&tag) {
                    violations.push(GranularityViolation {
                        file: file.to_string(),
                        line: None,
                        rule: format!("{}-forbidden-tag", level.label()),
                        severity: ViolationSeverity::Error,
                        message: format!(
                            "{} must not contain '{}' tag",
                            level.label(),
                            tag
                        ),
                        suggestion: Some(format!(
                            "Move '({} ...)' to an L3-Implementation file",
                            tag
                        )),
                    });
                }
            }
            for child in children {
                check_forbidden_tags(child, forbidden, file, level, violations);
            }
        }
    }
}

/// Walk all Atom nodes in the tree, calling `f` on each atom value.
fn walk_atoms<F: FnMut(&str)>(node: &SexpNode, f: &mut F) {
    match node {
        SexpNode::Atom(val) => f(val),
        SexpNode::List(children) => {
            for child in children {
                walk_atoms(child, f);
            }
        }
    }
}

/// Tags that indicate codegen intent — forbidden in descriptive files.
/// Covers all three target pipelines: Rust, TypeScript, Python.
const CODEGEN_TAGS: &[&str] = &[
    // Rust patterns
    "crud-gateway", "rpc-gateway", "mcp-module", "bootstrap",
    "state-machine", "worker", "error-enum", "domain-engine",
    "pure-utility", "s3-dispatcher", "s3-response-builder",
    "admin-route-registry", "domain-types",
    // TypeScript patterns
    "vue-component", "pinia-store", "canvas-node", "external-engine",
    // Python patterns
    "comfy-node",
];

/// Check descriptive files for accidental codegen declarations.
fn check_descriptive_codegen_leak(
    node: &SexpNode,
    file: &str,
    violations: &mut Vec<GranularityViolation>,
) {
    match node {
        SexpNode::Atom(_) => {}
        SexpNode::List(children) => {
            if let Some(tag) = node.tag() {
                if CODEGEN_TAGS.contains(&tag) {
                    violations.push(GranularityViolation {
                        file: file.to_string(),
                        line: None,
                        rule: "descriptive-codegen-leak".into(),
                        severity: ViolationSeverity::Warning,
                        message: format!(
                            "descriptive file contains codegen tag '{}' — this will not generate code",
                            tag
                        ),
                        suggestion: Some(
                            "Change governance-mode to strict-codegen or experimental, or remove the codegen declaration".into(),
                        ),
                    });
                }
                // Also check for :pattern attribute (e.g. component with :pattern crud-gateway)
                if tag == "component" {
                    for child in children {
                        if let SexpNode::Atom(val) = child {
                            if val == ":pattern" {
                                violations.push(GranularityViolation {
                                    file: file.to_string(),
                                    line: None,
                                    rule: "descriptive-codegen-leak".into(),
                                    severity: ViolationSeverity::Warning,
                                    message: "descriptive file contains :pattern attribute — this will not generate code".into(),
                                    suggestion: Some(
                                        "Change governance-mode to strict-codegen or experimental".into(),
                                    ),
                                });
                            }
                        }
                    }
                }
            }
            for child in children {
                check_descriptive_codegen_leak(child, file, violations);
            }
        }
    }
}

/// Truncate a string for display in error messages.
fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max])
    }
}

// ============================================================================
// Structural lint — graph-level checks
// ============================================================================

/// Lint the structure of the entire intent graph.
///
/// Rules:
/// - L2/L3 files must have a parent declaration
/// - module-link targets cannot be L1 (only downward)
/// - File size thresholds: L1 > 200, L2 > 500, L3 > 1500 lines trigger warnings
/// - Orphan detection: governed non-L1 file not referenced by any module-link
pub fn lint_structure(graph: &IntentGraph) -> Vec<GranularityViolation> {
    let mut violations = Vec::new();

    // Build set of files referenced by module-links
    let referenced: std::collections::HashSet<usize> = graph
        .edges
        .iter()
        .map(|&(_, child)| child)
        .collect();

    for (idx, meta) in graph.files.iter().enumerate() {
        let file = meta.file_path.to_string_lossy().to_string();

        // Rule: L2/L3 must have parent
        if meta.level != GranularityLevel::L1Blueprint && meta.parent.is_none() {
            violations.push(GranularityViolation {
                file: file.clone(),
                line: None,
                rule: "parent-required".into(),
                severity: ViolationSeverity::Error,
                message: format!(
                    "{} file must declare (parent \"...\") pointing to its parent",
                    meta.level.label()
                ),
                suggestion: Some("Add (parent \"intent.lisp\") declaration".into()),
            });
        }

        // Rule: module-link targets cannot be L1
        for link in &meta.module_links {
            // Find the target index
            if let Some(target_meta) = graph
                .files
                .iter()
                .find(|m| m.file_path.to_string_lossy() == link.target_path)
            {
                if target_meta.level == GranularityLevel::L1Blueprint {
                    violations.push(GranularityViolation {
                        file: file.clone(),
                        line: None,
                        rule: "no-upward-link".into(),
                        severity: ViolationSeverity::Error,
                        message: format!(
                            "module-link to '{}' targets an L1-Blueprint — links must point downward",
                            link.target_path
                        ),
                        suggestion: Some("module-link should reference L2 or L3 files only".into()),
                    });
                }
            }
        }

        // Rule: file size thresholds
        let threshold = match meta.level {
            GranularityLevel::L1Blueprint => 200,
            GranularityLevel::L2Topology => 500,
            GranularityLevel::L3Implementation => 1500,
        };
        if meta.line_count > threshold {
            violations.push(GranularityViolation {
                file: file.clone(),
                line: None,
                rule: "file-size".into(),
                severity: ViolationSeverity::Warning,
                message: format!(
                    "{} file has {} lines, exceeding {} threshold of {} lines",
                    meta.level.label(),
                    meta.line_count,
                    meta.level.label(),
                    threshold
                ),
                suggestion: Some("Consider splitting into sub-files".into()),
            });
        }

        // Rule: orphan detection — non-L1, governed, not referenced by any edge
        if meta.level != GranularityLevel::L1Blueprint
            && !referenced.contains(&idx)
            && idx != graph.root
        {
            violations.push(GranularityViolation {
                file: file.clone(),
                line: None,
                rule: "orphan-governed".into(),
                severity: ViolationSeverity::Warning,
                message: format!(
                    "{} is not referenced by any module-link",
                    file
                ),
                suggestion: Some(
                    "Add a (module-link ...) in the parent file or remove this file".into(),
                ),
            });
        }

        // Rule: experimental should be L3
        if meta.governance_mode == Some(GovernanceMode::Experimental)
            && meta.level != GranularityLevel::L3Implementation
        {
            violations.push(GranularityViolation {
                file: file.clone(),
                line: None,
                rule: "experimental-level".into(),
                severity: ViolationSeverity::Warning,
                message: format!(
                    "experimental governance-mode on {} file — sandbox experiments should target L3-Implementation",
                    meta.level.label()
                ),
                suggestion: Some(
                    "Move experimental content to an L3-Implementation file".into(),
                ),
            });
        }

        // Info: experimental sandbox notice
        if meta.governance_mode == Some(GovernanceMode::Experimental) {
            violations.push(GranularityViolation {
                file: file.clone(),
                line: None,
                rule: "experimental-sandbox".into(),
                severity: ViolationSeverity::Info,
                message: "AI experimental sandbox enabled. Ensure this component has sandbox tests.".into(),
                suggestion: None,
            });
        }
    }

    violations
}

// ============================================================================
// Split suggestion
// ============================================================================

/// Count specific tags in the S-expression tree and suggest how to split.
///
/// Returns `None` if the file is small enough (< 300 lines and <= 3 tables).
pub fn suggest_split(nodes: &[SexpNode], line_count: usize) -> Option<SplitSuggestion> {
    let mut table_count = 0usize;
    let mut api_count = 0usize;
    let mut flow_count = 0usize;

    for node in nodes {
        count_tags(node, &mut table_count, &mut api_count, &mut flow_count);
    }

    // Threshold: if small and few tables, no split needed
    if line_count < 300 && table_count <= 3 {
        return None;
    }

    let mut suggested_files = Vec::new();

    // Always suggest L1 root
    suggested_files.push((
        "intent.lisp".into(),
        "L1-Blueprint: architecture overview, pillars, design constraints".into(),
    ));

    // Suggest L2 for flows
    if flow_count > 0 {
        suggested_files.push((
            "intent-flows.lisp".into(),
            format!(
                "L2-Topology: {} flow/state-machine declarations",
                flow_count
            ),
        ));
    }

    // Suggest L3 for tables if > 3
    if table_count > 3 {
        suggested_files.push((
            "intent-db.lisp".into(),
            format!("L3-Implementation: {} table definitions with columns", table_count),
        ));
    }

    // Suggest L3 for API if > 10
    if api_count > 10 {
        suggested_files.push((
            "intent-api.lisp".into(),
            format!("L3-Implementation: {} endpoint/call definitions", api_count),
        ));
    }

    Some(SplitSuggestion {
        current_lines: line_count,
        table_count,
        api_count,
        flow_count,
        suggested_files,
    })
}

/// Recursively count tags relevant to split decisions.
fn count_tags(
    node: &SexpNode,
    tables: &mut usize,
    apis: &mut usize,
    flows: &mut usize,
) {
    match node {
        SexpNode::Atom(_) => {}
        SexpNode::List(children) => {
            if let Some(tag) = node.tag() {
                match tag {
                    "table" => *tables += 1,
                    "col" => {} // counted via table
                    "endpoint" | "calls" => *apis += 1,
                    "flow" | "state-machine" => *flows += 1,
                    _ => {}
                }
            }
            for child in children {
                count_tags(child, tables, apis, flows);
            }
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intent_graph::{GovernanceMode, GranularityLevel, IntentFileMeta, IntentGraph, ModuleLink};
    use crate::sexp_parser::SexpNode;
    use std::collections::HashSet;
    use std::path::PathBuf;

    fn meta(name: &str, level: GranularityLevel, path: &str, lines: usize) -> IntentFileMeta {
        IntentFileMeta {
            file_path: PathBuf::from(path),
            intent_name: name.into(),
            level,
            parent: None,
            version: None,
            module_links: Vec::new(),
            line_count: lines,
            governance_mode: None,
        }
    }

    fn meta_with_parent(
        name: &str,
        level: GranularityLevel,
        path: &str,
        parent: &str,
        lines: usize,
    ) -> IntentFileMeta {
        IntentFileMeta {
            file_path: PathBuf::from(path),
            intent_name: name.into(),
            level,
            parent: Some(parent.into()),
            version: None,
            module_links: Vec::new(),
            line_count: lines,
            governance_mode: None,
        }
    }

    // ---- lint_content tests ----

    #[test]
    fn test_l1_forbids_col_tag() {
        let nodes = SexpNode::parse(r#"
            (intent root
              (granularity L1-Blueprint)
              (table users (col id uuid)))
        "#).unwrap();
        let m = meta("root", GranularityLevel::L1Blueprint, "intent.lisp", 4);
        let v = lint_content(&m, &nodes);
        assert!(v.iter().any(|v| v.rule.contains("forbidden-tag") && v.message.contains("col")));
    }

    #[test]
    fn test_l1_forbids_op_tag() {
        let nodes = SexpNode::parse(r#"
            (intent root
              (granularity L1-Blueprint)
              (op insert))
        "#).unwrap();
        let m = meta("root", GranularityLevel::L1Blueprint, "intent.lisp", 4);
        let v = lint_content(&m, &nodes);
        assert!(v.iter().any(|v| v.message.contains("op")));
    }

    #[test]
    fn test_l1_forbids_function_tag() {
        let nodes = SexpNode::parse(r#"
            (intent root
              (granularity L1-Blueprint)
              (function compute))
        "#).unwrap();
        let m = meta("root", GranularityLevel::L1Blueprint, "intent.lisp", 3);
        let v = lint_content(&m, &nodes);
        assert!(v.iter().any(|v| v.message.contains("function")));
    }

    #[test]
    fn test_l1_forbids_url_atoms() {
        let nodes = SexpNode::parse(r#"
            (intent root
              (granularity L1-Blueprint)
              (endpoint "https://api.example.com/v1"))
        "#).unwrap();
        let m = meta("root", GranularityLevel::L1Blueprint, "intent.lisp", 3);
        let v = lint_content(&m, &nodes);
        assert!(v.iter().any(|v| v.rule == "L1-no-urls"));
    }

    #[test]
    fn test_l1_no_false_positive_on_non_url() {
        let nodes = SexpNode::parse(r#"
            (intent root
              (granularity L1-Blueprint)
              (purpose "does stuff"))
        "#).unwrap();
        let m = meta("root", GranularityLevel::L1Blueprint, "intent.lisp", 3);
        let v = lint_content(&m, &nodes);
        assert!(v.is_empty());
    }

    #[test]
    fn test_l2_forbids_col() {
        let nodes = SexpNode::parse(r#"
            (intent db
              (granularity L2-Topology)
              (table users (col id uuid)))
        "#).unwrap();
        let m = meta("db", GranularityLevel::L2Topology, "db.lisp", 4);
        let v = lint_content(&m, &nodes);
        assert!(v.iter().any(|v| v.message.contains("col")));
    }

    #[test]
    fn test_l2_allows_function() {
        // L2 only forbids col and op, not function
        let nodes = SexpNode::parse(r#"
            (intent topo
              (granularity L2-Topology)
              (function compute))
        "#).unwrap();
        let m = meta("topo", GranularityLevel::L2Topology, "topo.lisp", 3);
        let v = lint_content(&m, &nodes);
        assert!(v.is_empty());
    }

    #[test]
    fn test_l3_allows_everything() {
        let nodes = SexpNode::parse(r#"
            (intent tables
              (granularity L3-Implementation)
              (table users
                (col id uuid)
                (col name text)
                (op insert)
                (op delete))
              (function "https://api.example.com"))
        "#).unwrap();
        let m = meta("tables", GranularityLevel::L3Implementation, "tables.lisp", 8);
        let v = lint_content(&m, &nodes);
        assert!(v.is_empty());
    }

    // ---- lint_structure tests ----

    #[test]
    fn test_l2_missing_parent() {
        let graph = IntentGraph {
            project_name: "test".into(),
            root: 0,
            files: vec![
                meta("root", GranularityLevel::L1Blueprint, "intent.lisp", 50),
                meta("db", GranularityLevel::L2Topology, "db.lisp", 50),
            ],
            edges: {
                let mut e = HashSet::new();
                e.insert((0, 1));
                e
            },
            orphans: vec![],
        };
        let v = lint_structure(&graph);
        assert!(v.iter().any(|v| v.rule == "parent-required"));
    }

    #[test]
    fn test_l2_with_parent_ok() {
        let graph = IntentGraph {
            project_name: "test".into(),
            root: 0,
            files: vec![
                meta("root", GranularityLevel::L1Blueprint, "intent.lisp", 50),
                meta_with_parent(
                    "db",
                    GranularityLevel::L2Topology,
                    "db.lisp",
                    "intent.lisp",
                    50,
                ),
            ],
            edges: {
                let mut e = HashSet::new();
                e.insert((0, 1));
                e
            },
            orphans: vec![],
        };
        let v = lint_structure(&graph);
        assert!(!v.iter().any(|v| v.rule == "parent-required"));
    }

    #[test]
    fn test_file_size_warning_l1() {
        let graph = IntentGraph {
            project_name: "test".into(),
            root: 0,
            files: vec![meta(
                "root",
                GranularityLevel::L1Blueprint,
                "intent.lisp",
                250,
            )],
            edges: HashSet::new(),
            orphans: vec![],
        };
        let v = lint_structure(&graph);
        assert!(v.iter().any(|v| v.rule == "file-size"));
    }

    #[test]
    fn test_file_size_ok_l1() {
        let graph = IntentGraph {
            project_name: "test".into(),
            root: 0,
            files: vec![meta(
                "root",
                GranularityLevel::L1Blueprint,
                "intent.lisp",
                100,
            )],
            edges: HashSet::new(),
            orphans: vec![],
        };
        let v = lint_structure(&graph);
        assert!(!v.iter().any(|v| v.rule == "file-size"));
    }

    #[test]
    fn test_file_size_warning_l3() {
        let graph = IntentGraph {
            project_name: "test".into(),
            root: 0,
            files: vec![
                meta("root", GranularityLevel::L1Blueprint, "intent.lisp", 50),
                meta_with_parent(
                    "tables",
                    GranularityLevel::L3Implementation,
                    "tables.lisp",
                    "intent.lisp",
                    1600,
                ),
            ],
            edges: {
                let mut e = HashSet::new();
                e.insert((0, 1));
                e
            },
            orphans: vec![],
        };
        let v = lint_structure(&graph);
        assert!(v.iter().any(|v| v.rule == "file-size" && v.file == "tables.lisp"));
    }

    #[test]
    fn test_orphan_governed_file() {
        let graph = IntentGraph {
            project_name: "test".into(),
            root: 0,
            files: vec![
                meta("root", GranularityLevel::L1Blueprint, "intent.lisp", 50),
                meta_with_parent(
                    "stray",
                    GranularityLevel::L2Topology,
                    "stray.lisp",
                    "intent.lisp",
                    50,
                ),
            ],
            // No edges — stray.lisp is not referenced
            edges: HashSet::new(),
            orphans: vec![],
        };
        let v = lint_structure(&graph);
        assert!(v.iter().any(|v| v.rule == "orphan-governed"));
    }

    #[test]
    fn test_module_link_to_l1_forbidden() {
        let mut root = meta("root", GranularityLevel::L1Blueprint, "intent.lisp", 50);
        root.module_links.push(ModuleLink {
            target_path: "other-root.lisp".into(),
            expected_level: None,
            source_pillar: None,
        });
        let graph = IntentGraph {
            project_name: "test".into(),
            root: 0,
            files: vec![
                root,
                meta(
                    "other",
                    GranularityLevel::L1Blueprint,
                    "other-root.lisp",
                    50,
                ),
            ],
            edges: {
                let mut e = HashSet::new();
                e.insert((0, 1));
                e
            },
            orphans: vec![],
        };
        let v = lint_structure(&graph);
        assert!(v.iter().any(|v| v.rule == "no-upward-link"));
    }

    // ---- suggest_split tests ----

    #[test]
    fn test_suggest_split_small_file() {
        let nodes = SexpNode::parse(r#"
            (intent small
              (table users (col id uuid))
              (table posts (col id uuid)))
        "#).unwrap();
        assert!(suggest_split(&nodes, 100).is_none());
    }

    #[test]
    fn test_suggest_split_large_file() {
        let nodes = SexpNode::parse(r#"
            (intent large
              (table t1) (table t2) (table t3) (table t4) (table t5)
              (flow f1) (flow f2)
              (endpoint e1) (endpoint e2) (endpoint e3)
              (endpoint e4) (endpoint e5) (endpoint e6)
              (endpoint e7) (endpoint e8) (endpoint e9)
              (endpoint e10) (endpoint e11))
        "#).unwrap();
        let suggestion = suggest_split(&nodes, 500).unwrap();
        assert_eq!(suggestion.table_count, 5);
        assert_eq!(suggestion.flow_count, 2);
        assert_eq!(suggestion.api_count, 11);
        // Should suggest L1 root + flows + db + api
        assert!(suggestion.suggested_files.len() >= 3);
        assert!(suggestion
            .suggested_files
            .iter()
            .any(|(f, _)| f == "intent-db.lisp"));
        assert!(suggestion
            .suggested_files
            .iter()
            .any(|(f, _)| f == "intent-api.lisp"));
        assert!(suggestion
            .suggested_files
            .iter()
            .any(|(f, _)| f == "intent-flows.lisp"));
    }

    #[test]
    fn test_suggest_split_threshold_boundary() {
        // Exactly 3 tables and < 300 lines — no split
        let nodes = SexpNode::parse(r#"
            (intent mid (table t1) (table t2) (table t3))
        "#).unwrap();
        assert!(suggest_split(&nodes, 299).is_none());

        // 4 tables but < 300 lines — triggers split because table_count > 3
        // (condition for None is: line_count < 300 AND table_count <= 3)
        let nodes2 = SexpNode::parse(r#"
            (intent mid (table t1) (table t2) (table t3) (table t4))
        "#).unwrap();
        let s = suggest_split(&nodes2, 200).unwrap();
        assert_eq!(s.table_count, 4);

        // 300 lines triggers even with few tables
        let nodes3 = SexpNode::parse(r#"
            (intent big (table t1))
        "#).unwrap();
        assert!(suggest_split(&nodes3, 300).is_some());
    }

    // ---- governance-mode lint tests ----

    #[test]
    fn test_descriptive_warns_on_codegen_tags() {
        let nodes = SexpNode::parse(r#"
            (intent legacy-desc
              (granularity L1-Blueprint)
              (governance-mode "descriptive")
              (crud-gateway users))
        "#).unwrap();
        let mut m = meta("legacy-desc", GranularityLevel::L1Blueprint, "desc.lisp", 5);
        m.governance_mode = Some(GovernanceMode::Descriptive);
        let v = lint_content(&m, &nodes);
        assert!(v.iter().any(|v| v.rule == "descriptive-codegen-leak"));
    }

    #[test]
    fn test_descriptive_skips_granularity_checks() {
        // Descriptive with col tag should NOT trigger L1-forbidden-tag, only descriptive-codegen-leak
        let nodes = SexpNode::parse(r#"
            (intent desc
              (granularity L1-Blueprint)
              (governance-mode "descriptive")
              (table users (col id uuid)))
        "#).unwrap();
        let mut m = meta("desc", GranularityLevel::L1Blueprint, "desc.lisp", 5);
        m.governance_mode = Some(GovernanceMode::Descriptive);
        let v = lint_content(&m, &nodes);
        assert!(!v.iter().any(|v| v.rule.contains("forbidden-tag")));
    }

    #[test]
    fn test_descriptive_clean_file_no_warnings() {
        let nodes = SexpNode::parse(r#"
            (intent desc-clean
              (granularity L1-Blueprint)
              (governance-mode "descriptive")
              (pillar core (purpose "core logic")))
        "#).unwrap();
        let mut m = meta("desc-clean", GranularityLevel::L1Blueprint, "desc-clean.lisp", 5);
        m.governance_mode = Some(GovernanceMode::Descriptive);
        let v = lint_content(&m, &nodes);
        assert!(v.is_empty());
    }

    #[test]
    fn test_experimental_warns_if_not_l3() {
        let mut m = meta("sandbox", GranularityLevel::L2Topology, "sandbox.lisp", 50);
        m.governance_mode = Some(GovernanceMode::Experimental);
        let graph = IntentGraph {
            project_name: "test".into(),
            root: 0,
            files: vec![
                meta("root", GranularityLevel::L1Blueprint, "intent.lisp", 50),
                m,
            ],
            edges: {
                let mut e = HashSet::new();
                e.insert((0, 1));
                e
            },
            orphans: vec![],
        };
        let v = lint_structure(&graph);
        assert!(v.iter().any(|v| v.rule == "experimental-level"));
    }

    #[test]
    fn test_experimental_l3_emits_info() {
        let mut m = meta_with_parent("sandbox", GranularityLevel::L3Implementation, "sandbox.lisp", "intent.lisp", 50);
        m.governance_mode = Some(GovernanceMode::Experimental);
        let graph = IntentGraph {
            project_name: "test".into(),
            root: 0,
            files: vec![
                meta("root", GranularityLevel::L1Blueprint, "intent.lisp", 50),
                m,
            ],
            edges: {
                let mut e = HashSet::new();
                e.insert((0, 1));
                e
            },
            orphans: vec![],
        };
        let v = lint_structure(&graph);
        assert!(!v.iter().any(|v| v.rule == "experimental-level"));
        assert!(v.iter().any(|v| v.rule == "experimental-sandbox"
            && matches!(v.severity, ViolationSeverity::Info)));
    }

    #[test]
    fn test_unknown_governance_mode_parses_as_none() {
        // Invalid governance-mode string should parse as None (graceful degradation)
        let nodes = SexpNode::parse(r#"
            (intent bad-mode
              (granularity L1-Blueprint)
              (governance-mode "unknown-mode"))
        "#).unwrap();
        let m = IntentFileMeta::from_sexp(
            &nodes, std::path::Path::new("bad.lisp"), 4,
        ).unwrap();
        assert_eq!(m.governance_mode, None);
    }

    #[test]
    fn test_descriptive_warns_on_ts_python_codegen_tags() {
        // Cross-language: vue-component and comfy-node should trigger leak warning
        let nodes = SexpNode::parse(r#"
            (intent desc-cross
              (granularity L1-Blueprint)
              (governance-mode "descriptive")
              (vue-component MyWidget)
              (comfy-node UpscaleV2))
        "#).unwrap();
        let mut m = meta("desc-cross", GranularityLevel::L1Blueprint, "desc-cross.lisp", 6);
        m.governance_mode = Some(GovernanceMode::Descriptive);
        let v = lint_content(&m, &nodes);
        let leak_rules: Vec<_> = v.iter()
            .filter(|v| v.rule == "descriptive-codegen-leak")
            .collect();
        assert_eq!(leak_rules.len(), 2, "should warn on both vue-component and comfy-node");
    }
}
