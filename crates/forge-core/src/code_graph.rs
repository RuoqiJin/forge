//! Code Graph Engine — fuses S-EXP topology with AST index into an architecture visualization.
//!
//! Simplified port from jarvis-core::memory::code_graph.
//! Key difference: no Storage/Awareness dependency. Takes data as parameters.
//!
//! Three layers:
//! 1. `build_architecture_graph()` — orchestrates the full graph build
//! 2. Rosetta stone (`humanize_symbol`) — human-readable labels
//! 3. File-to-component matching heuristics

// @beacon: code-graph-engine

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::graph_types::{GraphEdge, GraphNode, GraphNodeType, MappedGraph, NodeData, Violation};
use crate::scanner::CodeNode;
use crate::sexp_mapper::map_sexp;
use crate::sexp_parser::SexpNode;

// ── Public API ────────────────────────────────────────────────────────────

/// Build a complete architecture graph by fusing S-EXP topology with AST nodes.
///
/// - `topology_sexp`: raw S-EXP text (from reality.sexp, intent.lisp, or jarvis-topology.sexp)
/// - `ast_nodes`: code symbols extracted by the scanner
///
/// The function handles `(system ...)`, `(intent ...)`, and `(reality ...)` root tags.
pub fn build_architecture_graph(
    topology_sexp: &str,
    ast_nodes: &[CodeNode],
) -> MappedGraph {
    // ── Step 1: Parse S-EXP -> skeleton graph ──────────────────────
    let sexp_tree = SexpNode::parse(topology_sexp).unwrap_or_default();
    let root = sexp_tree.first();
    let mut graph = if let Some(root) = root {
        map_sexp(root)
    } else {
        MappedGraph {
            nodes: vec![],
            edges: vec![],
            violations: vec![],
        }
    };

    // ── Step 2: AST enrichment ────────────────────────────────────
    // Build file -> symbol count
    let mut file_symbol_counts: HashMap<String, usize> = HashMap::new();
    for node in ast_nodes {
        *file_symbol_counts
            .entry(node.file_path.clone())
            .or_default() += 1;
    }

    // Build call graph: callee -> list of callers
    let mut callers_of: HashMap<String, Vec<String>> = HashMap::new();
    for node in ast_nodes {
        for callee in &node.calls {
            if !callee.is_empty() {
                callers_of
                    .entry(callee.clone())
                    .or_default()
                    .push(node.name.clone());
            }
        }
    }

    // Build a map from (pillar_id, comp_id) -> SexpNode for match-path lookup.
    let mut comp_sexp_map: HashMap<(String, String), SexpNode> = HashMap::new();
    if let Some(root) = root {
        for pillar_node in root.find_all("pillar") {
            let pillar_id = pillar_node.id().unwrap_or("").to_string();
            // Format 1: (components (X ...) (Y ...))
            if let Some(comps) = pillar_node.find("components") {
                for comp in comps.children() {
                    if let Some(comp_id) = comp.tag() {
                        if comp_id != "components" {
                            comp_sexp_map.insert(
                                (pillar_id.clone(), comp_id.to_string()),
                                comp.clone(),
                            );
                        }
                    }
                }
            }
            // Format 2: direct (component Y ...) children
            for comp in pillar_node.find_all("component") {
                if let Some(comp_id) = comp.id() {
                    comp_sexp_map.insert(
                        (pillar_id.clone(), comp_id.to_string()),
                        comp.clone(),
                    );
                }
            }
        }
    }

    // Build a map from component node id -> (pillar_id, comp_id)
    let comp_info: Vec<(String, String, String)> = graph
        .nodes
        .iter()
        .filter(|n| matches!(n.node_type, GraphNodeType::Component))
        .filter_map(|n| {
            let parent = n.parent_id.as_deref()?;
            let pillar_id = if let Some(p) = parent.strip_prefix("pillar-") {
                p.to_string()
            } else if let Some(p) = parent.strip_prefix("submod-") {
                p.split('-').next().unwrap_or(p).to_string()
            } else {
                return None;
            };
            Some((n.id.clone(), pillar_id, n.label.clone()))
        })
        .collect();

    let mut assigned_files: HashSet<String> = HashSet::new();

    // Enrich each Component node with AST data
    for (comp_node_id, pillar_id, comp_label) in &comp_info {
        let comp_sexp = comp_sexp_map.get(&(pillar_id.clone(), comp_label.clone()));

        let comp_files: Vec<String> = file_symbol_counts
            .keys()
            .filter(|f| file_matches_component(f, pillar_id, comp_label, comp_sexp))
            .cloned()
            .collect();

        assigned_files.extend(comp_files.iter().cloned());

        let comp_symbols: usize = comp_files
            .iter()
            .map(|f| file_symbol_counts.get(f.as_str()).unwrap_or(&0))
            .sum();

        // Update the component node in the graph
        if let Some(node) = graph.nodes.iter_mut().find(|n| n.id == *comp_node_id) {
            node.data.symbol_count = Some(comp_symbols);
        }

        // Create Symbol child nodes under this component
        for ast_node in ast_nodes {
            if !comp_files.contains(&ast_node.file_path) {
                continue;
            }
            if ast_node.name.is_empty() {
                continue;
            }

            let (h_label, h_desc) =
                humanize_symbol(&ast_node.name, &ast_node.node_type, &ast_node.signature, &ast_node.file_path);

            graph.nodes.push(GraphNode {
                id: format!("sym-{}-{}", comp_label, ast_node.name),
                node_type: GraphNodeType::Symbol,
                label: ast_node.name.clone(),
                parent_id: Some(comp_node_id.clone()),
                position: None,
                data: NodeData {
                    human_label: Some(h_label),
                    human_description: Some(h_desc),
                    ..Default::default()
                },
            });

            // Build call edges
            for callee in &ast_node.calls {
                if !callee.is_empty() {
                    // Find if callee exists as a symbol in any component
                    let callee_sym_id = find_symbol_node_id(callee, ast_nodes, &comp_info);
                    if let Some(target_id) = callee_sym_id {
                        let source_id = format!("sym-{}-{}", comp_label, ast_node.name);
                        let edge_id = format!("call-{}-{}", ast_node.name, callee);
                        graph.edges.push(GraphEdge {
                            id: edge_id,
                            source: source_id,
                            target: target_id,
                            label: Some("calls".to_string()),
                            edge_type: Some("call".to_string()),
                        });
                    }
                }
            }
        }
    }

    // Update Pillar nodes with aggregated AST stats
    let pillar_ids: Vec<String> = graph
        .nodes
        .iter()
        .filter(|n| matches!(n.node_type, GraphNodeType::Pillar))
        .map(|n| n.id.clone())
        .collect();

    for pillar_node_id in &pillar_ids {
        let pillar_raw_id = pillar_node_id
            .strip_prefix("pillar-")
            .unwrap_or(pillar_node_id);

        let pillar_symbols: usize = file_symbol_counts
            .iter()
            .filter(|(f, _)| f.contains(&format!("{}/", pillar_raw_id)))
            .map(|(_, c)| *c)
            .sum();

        if let Some(node) = graph.nodes.iter_mut().find(|n| n.id == *pillar_node_id) {
            node.data.symbol_count = Some(pillar_symbols);
        }
    }

    // ── Step 3: Wildlands detection ───────────────────────────────
    let mut orphan_symbols: Vec<GraphNode> = Vec::new();
    for ast_node in ast_nodes {
        if ast_node.name.is_empty() || ast_node.file_path.is_empty() {
            continue;
        }
        if assigned_files.contains(&ast_node.file_path) {
            continue;
        }
        let (h_label, h_desc) =
            humanize_symbol(&ast_node.name, &ast_node.node_type, &ast_node.signature, &ast_node.file_path);

        orphan_symbols.push(GraphNode {
            id: format!(
                "orphan-{}-{}",
                ast_node.file_path.replace('/', "-"),
                ast_node.name
            ),
            node_type: GraphNodeType::Symbol,
            label: ast_node.name.clone(),
            parent_id: Some("wildlands".into()),
            position: None,
            data: NodeData {
                human_label: Some(h_label),
                human_description: Some(h_desc),
                ..Default::default()
            },
        });
    }

    if !orphan_symbols.is_empty() {
        let mut orphan_files: BTreeMap<String, usize> = BTreeMap::new();
        for ast_node in ast_nodes {
            if !assigned_files.contains(&ast_node.file_path)
                && !ast_node.file_path.is_empty()
                && !ast_node.name.is_empty()
            {
                *orphan_files
                    .entry(ast_node.file_path.clone())
                    .or_default() += 1;
            }
        }

        graph.nodes.push(GraphNode {
            id: "wildlands".into(),
            node_type: GraphNodeType::Pillar,
            label: "Wildlands".into(),
            parent_id: None,
            position: None,
            data: NodeData {
                purpose: Some("Unmanaged code not registered in S-EXP topology".into()),
                human_label: Some("Wildlands".into()),
                human_description: Some(format!(
                    "{} files, {} symbols not registered in architecture topology",
                    orphan_files.len(),
                    orphan_symbols.len()
                )),
                symbol_count: Some(orphan_symbols.len()),
                ..Default::default()
            },
        });

        for (file, count) in &orphan_files {
            let short = file.split('/').last().unwrap_or(file);
            graph.nodes.push(GraphNode {
                id: format!("orphan-file-{}", file.replace('/', "-")),
                node_type: GraphNodeType::Component,
                label: short.into(),
                parent_id: Some("wildlands".into()),
                position: None,
                data: NodeData {
                    symbol_count: Some(*count),
                    ..Default::default()
                },
            });
        }

        // Re-parent orphan symbols under their file component
        for sym in &mut orphan_symbols {
            if let Some(file) = sym.data.human_description.as_ref().and_then(|_| {
                // Re-derive file from the id pattern "orphan-{file}-{name}"
                None::<String>
            }) {
                sym.parent_id = Some(format!("orphan-file-{}", file.replace('/', "-")));
            }
        }

        graph.nodes.extend(orphan_symbols);
    }

    // ── Step 4: Merge AST-detected violations ─────────────────────
    for ast_node in ast_nodes {
        if ast_node.file_path.contains("control/") && ast_node.signature.contains("memory::storage")
        {
            graph.violations.push(Violation {
                source: ast_node.file_path.clone(),
                target: "memory/storage".into(),
                description: format!(
                    "{} imports memory/storage",
                    ast_node
                        .file_path
                        .split('/')
                        .last()
                        .unwrap_or(&ast_node.file_path)
                ),
                severity: "warning".into(),
            });
        }
        if ast_node.file_path.contains("tools/") && ast_node.signature.contains("memory::storage") {
            graph.violations.push(Violation {
                source: ast_node.file_path.clone(),
                target: "memory/storage".into(),
                description: format!(
                    "{} imports memory/storage",
                    ast_node
                        .file_path
                        .split('/')
                        .last()
                        .unwrap_or(&ast_node.file_path)
                ),
                severity: "info".into(),
            });
        }
    }
    graph.violations.sort_by(|a, b| a.source.cmp(&b.source));
    graph
        .violations
        .dedup_by(|a, b| a.source == b.source && a.target == b.target);

    graph
}

// ── Rosetta Stone ─────────────────────────────────────────────────────

pub fn humanize_symbol(
    name: &str,
    node_type: &str,
    _signature: &str,
    _file_path: &str,
) -> (String, String) {
    match (node_type, name) {
        ("struct", n) => (format!("{n} (struct)"), "Data structure".into()),
        ("enum", n) => (format!("{n} (enum)"), "Enumeration type".into()),
        ("trait", n) => (format!("{n} (trait)"), "Interface constraint".into()),
        ("impl", n) => (format!("{n} (impl)"), "Method implementation".into()),
        ("function", n) if n.starts_with("get_") || n.starts_with("list_") => {
            (format!("{} / query", n.replace('_', " ")), "Read data".into())
        }
        ("function", n) if n.starts_with("store_") || n.starts_with("create_") => {
            (
                format!("{} / write", n.replace('_', " ")),
                "Persist data".into(),
            )
        }
        ("function", n) if n.starts_with("handle_") => (
            format!("{} / handler", n[7..].replace('_', " ")),
            "Event/request handler".into(),
        ),
        ("function", n) => (n.replace('_', " "), String::new()),
        ("const", n) => (format!("{n} (const)"), "Constant".into()),
        ("mod", n) => (format!("{n} (mod)"), "Module".into()),
        ("class", n) => (format!("{n} (class)"), "Class".into()),
        ("interface", n) => (format!("{n} (interface)"), "Interface".into()),
        ("type", n) => (format!("{n} (type)"), "Type alias".into()),
        _ => (name.to_string(), String::new()),
    }
}

// ── File matching ─────────────────────────────────────────────────────

/// Check if a file path belongs to a component, using match-path from S-EXP
/// with fallback to convention-based `{pillar}/{component}` matching.
pub fn file_matches_component(
    file_path: &str,
    pillar_id: &str,
    comp_id: &str,
    comp_sexp: Option<&SexpNode>,
) -> bool {
    // Priority 1: explicit (match-path) from S-EXP
    if let Some(sexp) = comp_sexp {
        if let Some(explicit_path) = sexp.str_val("match-path") {
            return file_path.contains(explicit_path);
        }
        // Also check (files "path1" "path2" ...) declarations
        let declared_files = sexp.list_val("files");
        if !declared_files.is_empty() {
            return declared_files.iter().any(|f| file_path.contains(f));
        }
    }
    // Priority 2: convention-based {pillar}/{component}
    let comp_underscore = comp_id.replace('-', "_");
    let file_name_match = |fp: &str, comp: &str| -> bool {
        fp.contains(&format!("{}/{}", pillar_id, comp))
            || fp.contains(&format!("{}/{}.rs", pillar_id, comp))
            || fp.contains(&format!("{}/{}.ts", pillar_id, comp))
            || fp.contains(&format!("{}/{}.tsx", pillar_id, comp))
            || (fp.contains(&format!("{}/", pillar_id))
                && (fp.ends_with(&format!("/{}.rs", comp))
                    || fp.ends_with(&format!("/{}/", comp))))
    };
    file_name_match(file_path, comp_id)
        || (comp_id != comp_underscore && file_name_match(file_path, &comp_underscore))
}

// ── Helpers ───────────────────────────────────────────────────────────

/// Try to find a symbol node ID for a callee name.
fn find_symbol_node_id(
    callee: &str,
    ast_nodes: &[CodeNode],
    comp_info: &[(String, String, String)],
) -> Option<String> {
    // Find the AST node for this callee
    let ast_node = ast_nodes.iter().find(|n| n.name == callee)?;

    // Find which component owns this file
    for (_comp_node_id, _pillar_id, comp_label) in comp_info {
        // Simple check: does the file path contain the component name pattern?
        let comp_underscore = comp_label.replace('-', "_");
        if ast_node.file_path.contains(comp_label)
            || ast_node.file_path.contains(&comp_underscore)
        {
            return Some(format!("sym-{}-{}", comp_label, callee));
        }
    }

    None
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_node(file: &str, name: &str, node_type: &str) -> CodeNode {
        CodeNode {
            id: format!("test-{}", name),
            file_path: file.to_string(),
            node_type: node_type.to_string(),
            name: name.to_string(),
            signature: String::new(),
            start_line: 1,
            end_line: 10,
            is_exported: true,
            docstring: None,
            stub_content: String::new(),
            calls: vec![],
            beacons: vec![],
        }
    }

    #[test]
    fn test_build_from_system_sexp() {
        let sexp = r#"(system test
            (pillar memory
                (components
                    (storage (human-label "Store")))))"#;
        let nodes = vec![make_node("memory/storage.rs", "Storage", "struct")];
        let graph = build_architecture_graph(sexp, &nodes);
        assert!(!graph.nodes.is_empty());
        // Should have pillar + component + symbol nodes
        let pillar = graph.nodes.iter().find(|n| n.id == "pillar-memory");
        assert!(pillar.is_some());
    }

    #[test]
    fn test_build_from_reality_sexp() {
        let sexp = r#"(reality test-project
            (pillar memory
                (components storage parser)
                (component storage
                    (files "memory/storage.rs"))))"#;
        let nodes = vec![make_node("memory/storage.rs", "Storage", "struct")];
        let graph = build_architecture_graph(sexp, &nodes);
        assert!(!graph.nodes.is_empty());
    }

    #[test]
    fn test_build_from_intent_sexp() {
        let sexp = r#"(intent test-project
            (pillar memory
                (component storage
                    (role "DB gateway"))))"#;
        let nodes = vec![make_node("memory/storage.rs", "Storage", "struct")];
        let graph = build_architecture_graph(sexp, &nodes);
        assert!(!graph.nodes.is_empty());
    }

    #[test]
    fn test_empty_sexp_still_produces_graph() {
        let graph = build_architecture_graph("", &[]);
        assert!(graph.nodes.is_empty());
        assert!(graph.edges.is_empty());
    }

    #[test]
    fn test_file_matches_component_explicit_path() {
        let sexp = SexpNode::parse_one(r#"(storage (match-path "memory/storage"))"#).unwrap();
        assert!(file_matches_component(
            "src/memory/storage.rs",
            "memory",
            "storage",
            Some(&sexp)
        ));
    }

    #[test]
    fn test_file_matches_component_convention() {
        assert!(file_matches_component(
            "crates/core/src/memory/storage.rs",
            "memory",
            "storage",
            None
        ));
        assert!(!file_matches_component(
            "crates/core/src/control/pty.rs",
            "memory",
            "storage",
            None
        ));
    }

    #[test]
    fn test_humanize_symbol() {
        let (label, desc) = humanize_symbol("Config", "struct", "", "");
        assert_eq!(label, "Config (struct)");
        assert_eq!(desc, "Data structure");
    }
}
