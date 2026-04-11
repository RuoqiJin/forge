//! Intent Graph — IR for multi-file intent governance.
//!
//! Parses `.missiond/*.lisp` files into a typed graph structure,
//! enforcing granularity levels (L1-Blueprint, L2-Topology, L3-Implementation)
//! and validating topology constraints (no cycles, correct hierarchy, etc.).

use crate::sexp_parser::SexpNode;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use serde::Serialize;

// ============================================================================
// Error type (local to governance layer)
// ============================================================================

#[derive(Debug, Clone)]
pub struct IrError {
    pub field: String,
    pub message: String,
    pub expected: Option<String>,
    pub got: Option<String>,
}

impl std::fmt::Display for IrError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.field, self.message)?;
        if let Some(ref exp) = self.expected {
            write!(f, " (expected: {exp})")?;
        }
        if let Some(ref got) = self.got {
            write!(f, " (got: {got})")?;
        }
        Ok(())
    }
}

impl std::error::Error for IrError {}

// ============================================================================
// Granularity Level
// ============================================================================

/// Granularity level for intent files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum GranularityLevel {
    L1Blueprint,
    L2Topology,
    L3Implementation,
}

impl GranularityLevel {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "L1-Blueprint" | "L1" => Some(Self::L1Blueprint),
            "L2-Topology" | "L2" => Some(Self::L2Topology),
            "L3-Implementation" | "L3" => Some(Self::L3Implementation),
            _ => None,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::L1Blueprint => "L1-Blueprint",
            Self::L2Topology => "L2-Topology",
            Self::L3Implementation => "L3-Implementation",
        }
    }

    /// Numeric rank for comparison (L1=0, L2=1, L3=2).
    pub fn rank(&self) -> u8 {
        match self {
            Self::L1Blueprint => 0,
            Self::L2Topology => 1,
            Self::L3Implementation => 2,
        }
    }
}

// ============================================================================
// Governance Mode
// ============================================================================

/// AI behavior control mode for intent files.
///
/// - `Descriptive`: read-only context; no codegen, no mutation experiments.
/// - `StrictCodegen`: controlled generation; modifications require proof.
/// - `Experimental`: AI sandbox; allows high-temperature mutation experiments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum GovernanceMode {
    Descriptive,
    StrictCodegen,
    Experimental,
    Cartography,
    Polisher,
    SelfDistillation,
}

impl GovernanceMode {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "descriptive" => Some(Self::Descriptive),
            "strict-codegen" => Some(Self::StrictCodegen),
            "experimental" => Some(Self::Experimental),
            "cartography" => Some(Self::Cartography),
            "polisher" => Some(Self::Polisher),
            "self-distillation" => Some(Self::SelfDistillation),
            _ => None,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Descriptive => "descriptive",
            Self::StrictCodegen => "strict-codegen",
            Self::Experimental => "experimental",
            Self::Cartography => "cartography",
            Self::Polisher => "polisher",
            Self::SelfDistillation => "self-distillation",
        }
    }
}

// ============================================================================
// Module Link
// ============================================================================

/// Module-link reference from parent to child file.
#[derive(Debug, Clone)]
pub struct ModuleLink {
    pub target_path: String,
    pub expected_level: Option<GranularityLevel>,
    pub source_pillar: Option<String>,
}

// ============================================================================
// Intent File Metadata
// ============================================================================

/// Metadata for a single governed intent file.
#[derive(Debug, Clone)]
pub struct IntentFileMeta {
    pub file_path: PathBuf,
    pub intent_name: String,
    pub level: GranularityLevel,
    pub parent: Option<String>,
    pub version: Option<String>,
    pub module_links: Vec<ModuleLink>,
    pub line_count: usize,
    pub governance_mode: Option<GovernanceMode>,
}

impl IntentFileMeta {
    /// Parse a governed intent file from its S-expression AST.
    ///
    /// Expects the first node to be `(intent NAME ...)` with at least a
    /// `(granularity ...)` child whose id is a valid level string.
    pub fn from_sexp(
        nodes: &[SexpNode],
        file_path: &Path,
        line_count: usize,
    ) -> Result<Self, IrError> {
        let root = nodes.first().ok_or_else(|| IrError {
            field: "root".into(),
            message: "empty S-expression".into(),
            expected: Some("(intent NAME ...)".into()),
            got: None,
        })?;

        // Extract intent name from root.id()
        let intent_name = root.id().ok_or_else(|| IrError {
            field: "intent".into(),
            message: "missing intent name".into(),
            expected: Some("(intent NAME ...)".into()),
            got: None,
        })?.to_string();

        // Extract granularity level
        let gran_node = root.find("granularity").ok_or_else(|| IrError {
            field: "granularity".into(),
            message: "missing (granularity ...) declaration".into(),
            expected: Some("(granularity L1-Blueprint|L2-Topology|L3-Implementation)".into()),
            got: None,
        })?;
        let level_str = gran_node.id().ok_or_else(|| IrError {
            field: "granularity".into(),
            message: "granularity has no level value".into(),
            expected: Some("L1-Blueprint, L2-Topology, or L3-Implementation".into()),
            got: None,
        })?;
        let level = GranularityLevel::from_str(level_str).ok_or_else(|| IrError {
            field: "granularity".into(),
            message: format!("unknown granularity level: {level_str}"),
            expected: Some("L1-Blueprint, L2-Topology, or L3-Implementation".into()),
            got: Some(level_str.to_string()),
        })?;

        // Extract parent
        let parent = root.str_val("parent").map(|s| s.to_string());

        // Extract version
        let version = root.str_val("version").map(|s| s.to_string());

        // Extract governance-mode (optional)
        let governance_mode = root
            .str_val("governance-mode")
            .and_then(GovernanceMode::from_str);

        // Extract module-links by walking tree
        let module_links = extract_module_links(root, None);

        Ok(IntentFileMeta {
            file_path: file_path.to_path_buf(),
            intent_name,
            level,
            parent,
            version,
            module_links,
            line_count,
            governance_mode,
        })
    }
}

/// Recursively walk S-expression tree to find `(module-link ...)` nodes.
/// Tracks the current pillar name for `source_pillar` context.
fn extract_module_links(node: &SexpNode, current_pillar: Option<&str>) -> Vec<ModuleLink> {
    let mut links = Vec::new();
    match node {
        SexpNode::Atom(_) => {}
        SexpNode::List(children) => {
            let tag = node.tag();
            if tag == Some("module-link") {
                let target_path = node.id().unwrap_or_default().to_string();
                let expected_level = node
                    .str_val("level")
                    .and_then(GranularityLevel::from_str);
                links.push(ModuleLink {
                    target_path,
                    expected_level,
                    source_pillar: current_pillar.map(|s| s.to_string()),
                });
            } else {
                // Track pillar context
                let pillar = if tag == Some("pillar") {
                    node.id()
                } else {
                    current_pillar
                };
                for child in children {
                    links.extend(extract_module_links(child, pillar));
                }
            }
        }
    }
    links
}

// ============================================================================
// Intent Graph
// ============================================================================

/// Complete intent topology graph for a project.
#[derive(Debug)]
pub struct IntentGraph {
    pub project_name: String,
    pub root: usize,
    pub files: Vec<IntentFileMeta>,
    pub edges: HashSet<(usize, usize)>,
    pub orphans: Vec<PathBuf>,
}

// ============================================================================
// Graph Resolution
// ============================================================================

/// Discover, parse, and validate all `.lisp` files under `missiond_dir`
/// into a connected intent graph.
pub fn resolve_intent_graph(missiond_dir: &Path) -> Result<IntentGraph, Vec<IrError>> {
    let mut errors: Vec<IrError> = Vec::new();
    let mut metas: Vec<IntentFileMeta> = Vec::new();
    let mut orphans: Vec<PathBuf> = Vec::new();

    // Step 1: discover all .lisp files
    let lisp_files = discover_lisp_files(missiond_dir);
    if lisp_files.is_empty() {
        errors.push(IrError {
            field: "discovery".into(),
            message: format!("no .lisp files found in {}", missiond_dir.display()),
            expected: None,
            got: None,
        });
        return Err(errors);
    }

    // Step 2: parse each file
    for file_path in &lisp_files {
        let content = match std::fs::read_to_string(file_path) {
            Ok(c) => c,
            Err(e) => {
                errors.push(IrError {
                    field: "io".into(),
                    message: format!("cannot read {}: {e}", file_path.display()),
                    expected: None,
                    got: None,
                });
                continue;
            }
        };
        let line_count = content.lines().count();
        let nodes = match SexpNode::parse(&content) {
            Ok(n) => n,
            Err(e) => {
                errors.push(IrError {
                    field: "parse".into(),
                    message: format!("parse error in {}: {e}", file_path.display()),
                    expected: None,
                    got: None,
                });
                continue;
            }
        };
        // Compute relative path from missiond_dir
        let rel_path = file_path
            .strip_prefix(missiond_dir)
            .unwrap_or(file_path)
            .to_path_buf();

        match IntentFileMeta::from_sexp(&nodes, &rel_path, line_count) {
            Ok(meta) => metas.push(meta),
            Err(_) => {
                // No granularity tag — legacy/orphan file
                orphans.push(rel_path);
            }
        }
    }

    if metas.is_empty() && !errors.is_empty() {
        return Err(errors);
    }

    // Step 3: find unique L1 root
    let l1_indices: Vec<usize> = metas
        .iter()
        .enumerate()
        .filter(|(_, m)| m.level == GranularityLevel::L1Blueprint)
        .map(|(i, _)| i)
        .collect();

    if l1_indices.is_empty() {
        errors.push(IrError {
            field: "root".into(),
            message: "no L1-Blueprint root found".into(),
            expected: Some("exactly one L1-Blueprint file".into()),
            got: Some("0".into()),
        });
        return Err(errors);
    }
    if l1_indices.len() > 1 {
        errors.push(IrError {
            field: "root".into(),
            message: format!("multiple L1-Blueprint files: {}", l1_indices.len()),
            expected: Some("exactly one L1-Blueprint file".into()),
            got: Some(l1_indices.len().to_string()),
        });
        return Err(errors);
    }
    let root_idx = l1_indices[0];

    // Build path -> index map (supports both "intent-db.lisp" and ".missiond/intent-db.lisp" references)
    let path_map: HashMap<String, usize> = metas
        .iter()
        .enumerate()
        .flat_map(|(i, m)| {
            let name = m.file_path.to_string_lossy().to_string();
            let with_prefix = format!(".missiond/{}", name);
            vec![(name, i), (with_prefix, i)]
        })
        .collect();

    let mut edges: HashSet<(usize, usize)> = HashSet::new();

    // Step 4: build edges from module-link (forward) and parent (backward)
    for (parent_idx, meta) in metas.iter().enumerate() {
        for link in &meta.module_links {
            if let Some(&child_idx) = path_map.get(&link.target_path) {
                edges.insert((parent_idx, child_idx));
            } else {
                // Step 5a: validate module-link targets exist
                errors.push(IrError {
                    field: "module-link".into(),
                    message: format!(
                        "module-link target '{}' not found (from {})",
                        link.target_path,
                        meta.file_path.display()
                    ),
                    expected: Some("existing governed file".into()),
                    got: Some(link.target_path.clone()),
                });
            }
        }
    }

    // Parent backlinks
    for (child_idx, meta) in metas.iter().enumerate() {
        if let Some(ref parent_path) = meta.parent {
            if let Some(&parent_idx) = path_map.get(parent_path) {
                edges.insert((parent_idx, child_idx));
            }
            // Don't error on missing parent — lint_structure will catch it
        }
    }

    // Step 5b: validate :level assertions match
    for (_parent_idx, meta) in metas.iter().enumerate() {
        for link in &meta.module_links {
            if let (Some(expected), Some(&child_idx)) =
                (link.expected_level, path_map.get(&link.target_path))
            {
                let actual = metas[child_idx].level;
                if actual != expected {
                    errors.push(IrError {
                        field: "module-link.level".into(),
                        message: format!(
                            "module-link to '{}' declares level {} but target is {}",
                            link.target_path,
                            expected.label(),
                            actual.label()
                        ),
                        expected: Some(expected.label().into()),
                        got: Some(actual.label().into()),
                    });
                }
            }
        }
    }

    // Step 6: cycle detection
    if has_cycle(metas.len(), &edges) {
        errors.push(IrError {
            field: "topology".into(),
            message: "cycle detected in intent graph".into(),
            expected: Some("acyclic DAG".into()),
            got: None,
        });
    }

    // Step 7: level hierarchy validation — child must be strictly finer than parent
    for &(parent_idx, child_idx) in &edges {
        let parent_level = metas[parent_idx].level;
        let child_level = metas[child_idx].level;
        if child_level.rank() <= parent_level.rank() {
            errors.push(IrError {
                field: "hierarchy".into(),
                message: format!(
                    "child '{}' ({}) must be strictly finer than parent '{}' ({})",
                    metas[child_idx].file_path.display(),
                    child_level.label(),
                    metas[parent_idx].file_path.display(),
                    parent_level.label(),
                ),
                expected: Some(format!("rank > {}", parent_level.rank())),
                got: Some(format!("rank = {}", child_level.rank())),
            });
        }
    }

    if !errors.is_empty() {
        return Err(errors);
    }

    let project_name = metas[root_idx].intent_name.clone();
    Ok(IntentGraph {
        project_name,
        root: root_idx,
        files: metas,
        edges,
        orphans,
    })
}

/// Discover all `.lisp` files under `dir` (non-recursive single level + recursive).
fn discover_lisp_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && path.extension().map(|e| e == "lisp").unwrap_or(false) {
                files.push(path);
            } else if path.is_dir() {
                // Recurse into subdirectories
                files.extend(discover_lisp_files(&path));
            }
        }
    }
    files.sort();
    files
}

/// Cycle detection using Kahn's algorithm (topological sort).
/// Returns `true` if a cycle exists.
pub fn has_cycle(file_count: usize, edges: &HashSet<(usize, usize)>) -> bool {
    let mut in_degree = vec![0usize; file_count];
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); file_count];

    for &(from, to) in edges {
        adj[from].push(to);
        in_degree[to] += 1;
    }

    let mut queue: VecDeque<usize> = VecDeque::new();
    for (i, &deg) in in_degree.iter().enumerate() {
        if deg == 0 {
            queue.push_back(i);
        }
    }

    let mut visited = 0usize;
    while let Some(node) = queue.pop_front() {
        visited += 1;
        for &neighbor in &adj[node] {
            in_degree[neighbor] -= 1;
            if in_degree[neighbor] == 0 {
                queue.push_back(neighbor);
            }
        }
    }

    visited < file_count
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp_parser::SexpNode;
    use std::path::Path;

    fn make_l1_sexp() -> Vec<SexpNode> {
        SexpNode::parse(r#"
            (intent my-project
              (granularity L1-Blueprint)
              (version "1.0")
              (pillar core
                (module-link "db.lisp" (level "L2-Topology"))
                (module-link "api.lisp" (level "L3-Implementation"))))
        "#).unwrap()
    }

    fn make_l2_sexp() -> Vec<SexpNode> {
        SexpNode::parse(r#"
            (intent my-project-db
              (granularity L2-Topology)
              (parent "intent.lisp")
              (version "1.0")
              (module-link "tables.lisp" (level "L3-Implementation")))
        "#).unwrap()
    }

    fn make_l3_sexp() -> Vec<SexpNode> {
        SexpNode::parse(r#"
            (intent my-project-tables
              (granularity L3-Implementation)
              (parent "db.lisp")
              (version "1.0")
              (table users
                (col id uuid)
                (col name text)
                (op insert)))
        "#).unwrap()
    }

    #[test]
    fn test_parse_l1() {
        let nodes = make_l1_sexp();
        let meta = IntentFileMeta::from_sexp(&nodes, Path::new("intent.lisp"), 10).unwrap();
        assert_eq!(meta.intent_name, "my-project");
        assert_eq!(meta.level, GranularityLevel::L1Blueprint);
        assert!(meta.parent.is_none());
        assert_eq!(meta.version.as_deref(), Some("1.0"));
        assert_eq!(meta.module_links.len(), 2);
        assert_eq!(meta.module_links[0].target_path, "db.lisp");
        assert_eq!(
            meta.module_links[0].expected_level,
            Some(GranularityLevel::L2Topology)
        );
        assert_eq!(meta.module_links[0].source_pillar.as_deref(), Some("core"));
    }

    #[test]
    fn test_parse_l2() {
        let nodes = make_l2_sexp();
        let meta = IntentFileMeta::from_sexp(&nodes, Path::new("db.lisp"), 8).unwrap();
        assert_eq!(meta.intent_name, "my-project-db");
        assert_eq!(meta.level, GranularityLevel::L2Topology);
        assert_eq!(meta.parent.as_deref(), Some("intent.lisp"));
        assert_eq!(meta.module_links.len(), 1);
    }

    #[test]
    fn test_parse_l3() {
        let nodes = make_l3_sexp();
        let meta = IntentFileMeta::from_sexp(&nodes, Path::new("tables.lisp"), 8).unwrap();
        assert_eq!(meta.intent_name, "my-project-tables");
        assert_eq!(meta.level, GranularityLevel::L3Implementation);
        assert_eq!(meta.parent.as_deref(), Some("db.lisp"));
    }

    #[test]
    fn test_missing_granularity() {
        let nodes = SexpNode::parse("(intent foo (version \"1\"))").unwrap();
        let err = IntentFileMeta::from_sexp(&nodes, Path::new("foo.lisp"), 1).unwrap_err();
        assert_eq!(err.field, "granularity");
    }

    #[test]
    fn test_invalid_granularity() {
        let nodes = SexpNode::parse("(intent foo (granularity L99))").unwrap();
        let err = IntentFileMeta::from_sexp(&nodes, Path::new("foo.lisp"), 1).unwrap_err();
        assert_eq!(err.field, "granularity");
        assert_eq!(err.got.as_deref(), Some("L99"));
    }

    #[test]
    fn test_granularity_level_from_str() {
        assert_eq!(
            GranularityLevel::from_str("L1-Blueprint"),
            Some(GranularityLevel::L1Blueprint)
        );
        assert_eq!(
            GranularityLevel::from_str("L1"),
            Some(GranularityLevel::L1Blueprint)
        );
        assert_eq!(
            GranularityLevel::from_str("L2-Topology"),
            Some(GranularityLevel::L2Topology)
        );
        assert_eq!(
            GranularityLevel::from_str("L3"),
            Some(GranularityLevel::L3Implementation)
        );
        assert_eq!(GranularityLevel::from_str("L4"), None);
    }

    #[test]
    fn test_granularity_rank() {
        assert!(GranularityLevel::L1Blueprint.rank() < GranularityLevel::L2Topology.rank());
        assert!(GranularityLevel::L2Topology.rank() < GranularityLevel::L3Implementation.rank());
    }

    // ---- governance-mode tests ----

    #[test]
    fn test_parse_governance_mode_descriptive() {
        let nodes = SexpNode::parse(r#"
            (intent my-desc
              (granularity L1-Blueprint)
              (governance-mode "descriptive"))
        "#).unwrap();
        let meta = IntentFileMeta::from_sexp(&nodes, Path::new("desc.lisp"), 4).unwrap();
        assert_eq!(meta.governance_mode, Some(GovernanceMode::Descriptive));
    }

    #[test]
    fn test_parse_governance_mode_strict_codegen() {
        let nodes = SexpNode::parse(r#"
            (intent my-strict
              (granularity L3-Implementation)
              (parent "intent.lisp")
              (governance-mode "strict-codegen"))
        "#).unwrap();
        let meta = IntentFileMeta::from_sexp(&nodes, Path::new("strict.lisp"), 5).unwrap();
        assert_eq!(meta.governance_mode, Some(GovernanceMode::StrictCodegen));
    }

    #[test]
    fn test_parse_governance_mode_experimental() {
        let nodes = SexpNode::parse(r#"
            (intent my-sandbox
              (granularity L3-Implementation)
              (parent "intent.lisp")
              (governance-mode "experimental"))
        "#).unwrap();
        let meta = IntentFileMeta::from_sexp(&nodes, Path::new("sandbox.lisp"), 5).unwrap();
        assert_eq!(meta.governance_mode, Some(GovernanceMode::Experimental));
    }

    #[test]
    fn test_parse_governance_mode_absent() {
        let nodes = make_l1_sexp();
        let meta = IntentFileMeta::from_sexp(&nodes, Path::new("intent.lisp"), 10).unwrap();
        assert_eq!(meta.governance_mode, None);
    }

    #[test]
    fn test_governance_mode_from_str() {
        assert_eq!(GovernanceMode::from_str("descriptive"), Some(GovernanceMode::Descriptive));
        assert_eq!(GovernanceMode::from_str("strict-codegen"), Some(GovernanceMode::StrictCodegen));
        assert_eq!(GovernanceMode::from_str("experimental"), Some(GovernanceMode::Experimental));
        assert_eq!(GovernanceMode::from_str("unknown"), None);
    }

    #[test]
    fn test_governance_mode_label() {
        assert_eq!(GovernanceMode::Descriptive.label(), "descriptive");
        assert_eq!(GovernanceMode::StrictCodegen.label(), "strict-codegen");
        assert_eq!(GovernanceMode::Experimental.label(), "experimental");
    }

    #[test]
    fn test_has_cycle_no_cycle() {
        let mut edges = HashSet::new();
        edges.insert((0, 1));
        edges.insert((1, 2));
        assert!(!has_cycle(3, &edges));
    }

    #[test]
    fn test_has_cycle_with_cycle() {
        let mut edges = HashSet::new();
        edges.insert((0, 1));
        edges.insert((1, 2));
        edges.insert((2, 0));
        assert!(has_cycle(3, &edges));
    }

    #[test]
    fn test_has_cycle_self_loop() {
        let mut edges = HashSet::new();
        edges.insert((0, 0));
        assert!(has_cycle(1, &edges));
    }

    #[test]
    fn test_has_cycle_empty() {
        let edges = HashSet::new();
        assert!(!has_cycle(3, &edges));
    }

    #[test]
    fn test_resolve_graph_tempdir() {
        let dir = std::env::temp_dir().join("forge_intent_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // L1 root
        std::fs::write(
            dir.join("intent.lisp"),
            r#"(intent my-project
              (granularity L1-Blueprint)
              (version "1.0")
              (module-link "db.lisp" (level "L2-Topology")))"#,
        )
        .unwrap();

        // L2 child
        std::fs::write(
            dir.join("db.lisp"),
            r#"(intent my-project-db
              (granularity L2-Topology)
              (parent "intent.lisp")
              (module-link "tables.lisp" (level "L3-Implementation")))"#,
        )
        .unwrap();

        // L3 leaf
        std::fs::write(
            dir.join("tables.lisp"),
            r#"(intent my-project-tables
              (granularity L3-Implementation)
              (parent "db.lisp")
              (table users (col id uuid)))"#,
        )
        .unwrap();

        let graph = resolve_intent_graph(&dir).unwrap();
        assert_eq!(graph.project_name, "my-project");
        assert_eq!(graph.files.len(), 3);
        assert_eq!(graph.orphans.len(), 0);
        // root is L1
        assert_eq!(graph.files[graph.root].level, GranularityLevel::L1Blueprint);
        // edges: intent -> db, db -> tables, plus parent backlinks (same edges via HashSet)
        assert!(!graph.edges.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_resolve_graph_missing_root() {
        let dir = std::env::temp_dir().join("forge_intent_test_no_root");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        std::fs::write(
            dir.join("db.lisp"),
            r#"(intent db (granularity L2-Topology) (parent "intent.lisp"))"#,
        )
        .unwrap();

        let err = resolve_intent_graph(&dir).unwrap_err();
        assert!(err.iter().any(|e| e.field == "root"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_resolve_graph_level_mismatch() {
        let dir = std::env::temp_dir().join("forge_intent_test_mismatch");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        std::fs::write(
            dir.join("intent.lisp"),
            r#"(intent root
              (granularity L1-Blueprint)
              (module-link "child.lisp" (level "L3-Implementation")))"#,
        )
        .unwrap();

        // Declare L2 but parent expects L3
        std::fs::write(
            dir.join("child.lisp"),
            r#"(intent child
              (granularity L2-Topology)
              (parent "intent.lisp"))"#,
        )
        .unwrap();

        let err = resolve_intent_graph(&dir).unwrap_err();
        assert!(err.iter().any(|e| e.field == "module-link.level"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_resolve_graph_hierarchy_violation() {
        let dir = std::env::temp_dir().join("forge_intent_test_hierarchy");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // L1 root links to another L1 — hierarchy violation
        std::fs::write(
            dir.join("a.lisp"),
            r#"(intent root (granularity L1-Blueprint) (module-link "b.lisp"))"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("b.lisp"),
            r#"(intent child (granularity L1-Blueprint) (parent "a.lisp"))"#,
        )
        .unwrap();

        let err = resolve_intent_graph(&dir).unwrap_err();
        // Multiple L1 or hierarchy violation
        assert!(!err.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_resolve_graph_orphan_detection() {
        let dir = std::env::temp_dir().join("forge_intent_test_orphan");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        std::fs::write(
            dir.join("intent.lisp"),
            r#"(intent root (granularity L1-Blueprint))"#,
        )
        .unwrap();

        // Legacy file with no granularity
        std::fs::write(
            dir.join("old.lisp"),
            r#"(intent old-stuff (table users (col id uuid)))"#,
        )
        .unwrap();

        let graph = resolve_intent_graph(&dir).unwrap();
        assert_eq!(graph.orphans.len(), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_module_link_extraction_nested() {
        let nodes = SexpNode::parse(r#"
            (intent proj
              (granularity L1-Blueprint)
              (pillar alpha
                (component x
                  (module-link "x-detail.lisp" (level "L3-Implementation"))))
              (pillar beta
                (module-link "beta-topo.lisp" (level "L2-Topology"))))
        "#).unwrap();
        let meta = IntentFileMeta::from_sexp(&nodes, Path::new("intent.lisp"), 8).unwrap();
        assert_eq!(meta.module_links.len(), 2);
        assert_eq!(meta.module_links[0].source_pillar.as_deref(), Some("alpha"));
        assert_eq!(meta.module_links[1].source_pillar.as_deref(), Some("beta"));
    }
}
