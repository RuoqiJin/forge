// @beacon: sexp-mapper
//! S-EXP -> Graph mapper — walks a parsed SexpNode tree and produces
//! GraphNode / GraphEdge / Violation structs for the architecture visualization.
//!
//! Ported from jarvis-core. This module is project-agnostic: it only knows
//! the S-EXP cartography protocol (observer, nervous-system, pillar, daemon,
//! binary, frontend, rules).

use crate::graph_types::{GraphEdge, GraphNode, GraphNodeType, MappedGraph, NodeData, Violation};
use crate::sexp_parser::SexpNode;

/// Walk the parsed S-EXP root node and produce the full
/// architecture graph skeleton (nodes, edges, violations).
///
/// Supports three root tag formats:
/// - `(system project-name ...)` — the original cartography protocol
/// - `(intent project-name ...)` — design intent declarations
/// - `(reality project-name ...)` — scanner-generated reality snapshots
///
/// For `intent` and `reality` roots, the body may contain direct `(pillar ...)` children
/// with either `(components (X ...) (Y ...))` or `(component X ...) (component Y ...)` format.
pub fn map_sexp(root: &SexpNode) -> MappedGraph {
    let mut nodes: Vec<GraphNode> = Vec::new();
    let mut edges: Vec<GraphEdge> = Vec::new();
    let mut violations: Vec<Violation> = Vec::new();

    // Collect pillar ids for global rule edge inference later.
    let mut pillar_ids: Vec<String> = Vec::new();
    // Communication target from rules (e.g. "event-bus").
    let mut comm_target: Option<String> = None;

    // Determine root tag — skip it (and the project-name atom) to get the body children.
    // For (system ...), body() already skips the tag. body() returns children after the
    // first atom, so for `(system project-name (pillar ...))` we get [project-name, (pillar ...)].
    // We iterate all body children and match on their tag regardless of root format.

    for child in root.body() {
        let tag = match child.tag() {
            Some(t) => t,
            None => continue,
        };

        match tag {
            "rules" | "design-constraints" => {
                // Extract (communication (must-use X))
                if let Some(comm) = child.find("communication") {
                    if let Some(target) = comm.str_val("must-use") {
                        comm_target = Some(target.to_string());
                    }
                }
            }

            "observer" => {
                let id = child.id().unwrap_or("observer").to_string();
                nodes.push(GraphNode {
                    id: id.clone(),
                    node_type: GraphNodeType::Infrastructure,
                    label: format!("{} (observer)", id),
                    parent_id: None,
                    position: None,
                    data: NodeData {
                        human_label: child.human_label().map(|s| s.to_string()),
                        human_description: child.human_desc().map(|s| s.to_string()),
                        description: child.str_val("role").map(|s| s.to_string()),
                        ..Default::default()
                    },
                });
                // Edges from (observes X Y ...)
                for target in child.list_val("observes") {
                    let edge_id = format!("{}-observes-{}", id, target);
                    edges.push(GraphEdge {
                        id: edge_id,
                        source: id.clone(),
                        target: target.to_string(),
                        label: Some("observes".to_string()),
                        edge_type: Some("observes".to_string()),
                    });
                }
            }

            "nervous-system" => {
                let id = child.id().unwrap_or("event-bus").to_string();
                let events: Vec<String> = child
                    .list_val("events")
                    .into_iter()
                    .map(|s| s.to_string())
                    .collect();
                nodes.push(GraphNode {
                    id: id.clone(),
                    node_type: GraphNodeType::Infrastructure,
                    label: format!("{} (event-bus)", id),
                    parent_id: None,
                    position: None,
                    data: NodeData {
                        human_label: child.human_label().map(|s| s.to_string()),
                        human_description: child.human_desc().map(|s| s.to_string()),
                        events: if events.is_empty() { None } else { Some(events) },
                        description: child.str_val("role").map(|s| s.to_string()),
                        ..Default::default()
                    },
                });
            }

            "pillar" => {
                let pillar_raw_id = child.id().unwrap_or("unknown").to_string();
                let pillar_id = format!("pillar-{}", pillar_raw_id);
                pillar_ids.push(pillar_id.clone());

                // Collect component names for exports.
                // Format 1: (components (X ...) (Y ...)) — nested block
                // Format 2: (component X ...) (component Y ...) — direct children
                // Both can coexist; we collect from both sources and dedup.
                let mut comp_names: Vec<String> = if let Some(comps) = child.find("components") {
                    comps
                        .body()
                        .iter()
                        .filter_map(|c| {
                            // body() skips the "components" tag, so children are the component entries
                            c.tag().or_else(|| c.as_atom()).map(|s| s.to_string())
                        })
                        .collect()
                } else {
                    Vec::new()
                };
                // Also collect direct (component X ...) children
                for direct_comp in child.find_all("component") {
                    if let Some(comp_id) = direct_comp.id() {
                        if !comp_names.contains(&comp_id.to_string()) {
                            comp_names.push(comp_id.to_string());
                        }
                    }
                }
                // Also include submodule names as exports so the pillar card shows them
                for submod in child.find_all("submodule") {
                    if let Some(id) = submod.id() {
                        if !id.is_empty() {
                            comp_names.push(id.to_string());
                        }
                    }
                }

                nodes.push(GraphNode {
                    id: pillar_id.clone(),
                    node_type: GraphNodeType::Pillar,
                    label: pillar_raw_id.clone(),
                    parent_id: None,
                    position: None,
                    data: NodeData {
                        human_label: child.human_label().map(|s| s.to_string()),
                        human_description: child.human_desc().map(|s| s.to_string()),
                        purpose: child.str_val("purpose").map(|s| s.to_string()),
                        exports: if comp_names.is_empty() {
                            None
                        } else {
                            Some(comp_names)
                        },
                        ..Default::default()
                    },
                });
                // Edges from (depends-on X Y ...)
                for target in child.list_val("depends-on") {
                    let edge_id = format!("{}-depends-{}", pillar_id, target);
                    edges.push(GraphEdge {
                        id: edge_id,
                        source: pillar_id.clone(),
                        target: target.to_string(),
                        label: Some("depends".to_string()),
                        edge_type: Some("depends".to_string()),
                    });
                }

                // Track which comp IDs we've already created nodes for (to avoid duplicates)
                let mut created_comp_ids: std::collections::HashSet<String> = std::collections::HashSet::new();

                // Pre-collect direct (component X ...) IDs so we can skip bare atoms that have full definitions
                let direct_comp_ids: std::collections::HashSet<String> = child
                    .find_all("component")
                    .iter()
                    .filter_map(|c| c.id().map(|s| s.to_string()))
                    .collect();

                // Component child nodes — Format 1: nested (components (X ...) (Y ...))
                if let Some(comps) = child.find("components") {
                    for comp in comps.body() {
                        let comp_raw_id = match comp.tag() {
                            Some(t) => t.to_string(),
                            None => {
                                // Handle bare atom entries: (components storage parser)
                                // Skip if a full (component X ...) definition exists
                                if let Some(atom_name) = comp.as_atom() {
                                    if direct_comp_ids.contains(atom_name) {
                                        continue; // Full definition will be processed in Format 2
                                    }
                                    let comp_id = format!("comp-{}-{}", pillar_raw_id, atom_name);
                                    if created_comp_ids.insert(comp_id.clone()) {
                                        nodes.push(GraphNode {
                                            id: comp_id,
                                            node_type: GraphNodeType::Component,
                                            label: atom_name.to_string(),
                                            parent_id: Some(pillar_id.clone()),
                                            position: None,
                                            data: NodeData::default(),
                                        });
                                    }
                                }
                                continue;
                            }
                        };
                        let comp_id = format!("comp-{}-{}", pillar_raw_id, comp_raw_id);

                        let beacons: Vec<String> = comp
                            .list_val("beacon")
                            .into_iter()
                            .map(|s| s.to_string())
                            .collect();

                        if created_comp_ids.insert(comp_id.clone()) {
                            nodes.push(GraphNode {
                                id: comp_id,
                                node_type: GraphNodeType::Component,
                                label: comp_raw_id,
                                parent_id: Some(pillar_id.clone()),
                                position: None,
                                data: NodeData {
                                    human_label: comp.human_label().map(|s| s.to_string()),
                                    human_description: comp.human_desc().map(|s| s.to_string()),
                                    beacons: if beacons.is_empty() {
                                        None
                                    } else {
                                        Some(beacons)
                                    },
                                    ..Default::default()
                                },
                            });
                        }
                    }
                }

                // Component child nodes — Format 2: direct (component X ...) children of pillar
                for direct_comp in child.find_all("component") {
                    let comp_raw_id = match direct_comp.id() {
                        Some(id) if !id.is_empty() => id.to_string(),
                        _ => continue,
                    };
                    let comp_id = format!("comp-{}-{}", pillar_raw_id, comp_raw_id);

                    // Skip if already created from (components ...) block
                    if !created_comp_ids.insert(comp_id.clone()) {
                        continue;
                    }

                    let beacons: Vec<String> = direct_comp
                        .list_val("beacon")
                        .into_iter()
                        .map(|s| s.to_string())
                        .collect();

                    nodes.push(GraphNode {
                        id: comp_id,
                        node_type: GraphNodeType::Component,
                        label: comp_raw_id,
                        parent_id: Some(pillar_id.clone()),
                        position: None,
                        data: NodeData {
                            human_label: direct_comp.human_label().map(|s| s.to_string()),
                            human_description: direct_comp.human_desc().map(|s| s.to_string()),
                            description: direct_comp.str_val("role").map(|s| s.to_string()),
                            beacons: if beacons.is_empty() {
                                None
                            } else {
                                Some(beacons)
                            },
                            ..Default::default()
                        },
                    });
                }

                // Submodule child nodes
                for submod in child.find_all("submodule") {
                    let submod_name = match submod.id() {
                        Some(id) if !id.is_empty() => id.to_string(),
                        _ => continue,
                    };

                    let submod_id = format!("submod-{}-{}", pillar_raw_id, submod_name);

                    let submod_comp_names: Vec<String> = submod
                        .list_val("components")
                        .into_iter()
                        .map(|s| s.to_string())
                        .collect();

                    nodes.push(GraphNode {
                        id: submod_id.clone(),
                        node_type: GraphNodeType::Submodule,
                        label: submod_name.clone(),
                        parent_id: Some(pillar_id.clone()),
                        position: None,
                        data: NodeData {
                            description: submod.str_val("path").map(|s| s.to_string()),
                            human_label: Some(submod_name.replace('_', " ").replace('-', " ")),
                            exports: if submod_comp_names.is_empty() {
                                None
                            } else {
                                Some(submod_comp_names.clone())
                            },
                            ..Default::default()
                        },
                    });

                    // Create component nodes in (components ...) declaration order.
                    let comp_defs: std::collections::HashMap<String, &SexpNode> = submod
                        .find_all("component")
                        .into_iter()
                        .filter_map(|c| c.id().map(|id| (id.to_string(), c)))
                        .collect();

                    for comp_name in &submod_comp_names {
                        let comp_id = format!("comp-{}-{}", pillar_raw_id, comp_name);

                        let (human_label, human_desc, beacons) = match comp_defs.get(comp_name) {
                            Some(comp) => {
                                let b: Vec<String> = comp
                                    .list_val("beacon")
                                    .into_iter()
                                    .map(|s| s.to_string())
                                    .collect();
                                (
                                    comp.human_label().map(|s| s.to_string()),
                                    comp.human_desc().map(|s| s.to_string()),
                                    b,
                                )
                            }
                            None => (None, None, Vec::new()),
                        };

                        nodes.push(GraphNode {
                            id: comp_id,
                            node_type: GraphNodeType::Component,
                            label: comp_name.clone(),
                            parent_id: Some(submod_id.clone()),
                            position: None,
                            data: NodeData {
                                human_label,
                                human_description: human_desc,
                                beacons: if beacons.is_empty() {
                                    None
                                } else {
                                    Some(beacons)
                                },
                                ..Default::default()
                            },
                        });
                    }
                }

                // Violations
                if let Some(viols) = child.find("violations") {
                    let severity = match pillar_raw_id.as_str() {
                        "control" => "warning",
                        "tools" => "info",
                        _ => "info",
                    };
                    for v in viols.body() {
                        let v_comp = match v.tag() {
                            Some(t) => t.to_string(),
                            None => continue,
                        };
                        let source = format!("comp-{}-{}", pillar_raw_id, v_comp);

                        // Try to extract (comp imports target "reason") pattern
                        let body = v.body();
                        if body.len() >= 2 {
                            if let Some(verb) = body[0].as_atom() {
                                if verb == "imports" {
                                    if let Some(target) = body[1].as_atom() {
                                        let description =
                                            format!("{} imports {}", v_comp, target);
                                        violations.push(Violation {
                                            source,
                                            target: target.to_string(),
                                            description,
                                            severity: severity.to_string(),
                                        });
                                        continue;
                                    }
                                }
                            }
                        }
                        // Fallback: first string child that contains spaces is description
                        if let Some(desc) = body.iter().find_map(|c| {
                            if let Some(s) = c.as_atom() {
                                if s.contains(' ') {
                                    return Some(s);
                                }
                            }
                            None
                        }) {
                            violations.push(Violation {
                                source,
                                target: String::new(),
                                description: desc.to_string(),
                                severity: severity.to_string(),
                            });
                        }
                    }
                }
            }

            "daemon" | "binary" | "frontend" => {
                let id = child.id().unwrap_or(tag).to_string();
                nodes.push(GraphNode {
                    id: id.clone(),
                    node_type: GraphNodeType::Infrastructure,
                    label: id.clone(),
                    parent_id: None,
                    position: None,
                    data: NodeData {
                        human_label: child.human_label().map(|s| s.to_string()),
                        human_description: child.human_desc().map(|s| s.to_string()),
                        description: child.str_val("role").map(|s| s.to_string()),
                        ..Default::default()
                    },
                });
                // Edges from (manages X Y Z) -> Edge(self -> "pillar-X")
                for target in child.list_val("manages") {
                    let pillar_target = format!("pillar-{}", target);
                    let edge_id = format!("{}-manages-{}", id, target);
                    edges.push(GraphEdge {
                        id: edge_id,
                        source: id.clone(),
                        target: pillar_target,
                        label: Some("manages".to_string()),
                        edge_type: Some("manages".to_string()),
                    });
                }

                // Edges from (depends-on X Y ...)
                for target in child.list_val("depends-on") {
                    let edge_id = format!("{}-depends-{}", id, target);
                    edges.push(GraphEdge {
                        id: edge_id,
                        source: id.clone(),
                        target: target.to_string(),
                        label: Some("depends".to_string()),
                        edge_type: Some("depends".to_string()),
                    });
                }
            }

            _ => {
                // Unknown top-level tags are silently ignored.
            }
        }
    }

    // ── Global rule edge inference ───────────────────────────────────────
    if let Some(ref target) = comm_target {
        for pid in &pillar_ids {
            let edge_id = format!("{}-events-{}", pid, target);
            edges.push(GraphEdge {
                id: edge_id,
                source: pid.clone(),
                target: target.clone(),
                label: Some("events".to_string()),
                edge_type: Some("events".to_string()),
            });
        }
    }

    MappedGraph {
        nodes,
        edges,
        violations,
    }
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn map_from_str(input: &str) -> MappedGraph {
        let nodes = SexpNode::parse(input).expect("parse failed");
        let root = &nodes[0];
        map_sexp(root)
    }

    #[test]
    fn nervous_system_produces_node_with_events() {
        let graph = map_from_str("(system test (nervous-system bus (events A B)))");
        assert_eq!(graph.nodes.len(), 1);
        let node = &graph.nodes[0];
        assert_eq!(node.id, "bus");
        assert_eq!(
            node.data.events.as_ref().unwrap(),
            &vec!["A".to_string(), "B".to_string()]
        );
    }

    #[test]
    fn pillar_with_components() {
        let input = r#"
(system test
  (pillar memory
    (human-label "Memory")
    (human-desc "Data layer")
    (purpose "data capture")
    (components
      (storage (human-label "Store") (human-desc "DB gateway") (beacon storage-layer))
      (parser  (human-label "Parser") (human-desc "JSONL parser")))))
"#;
        let graph = map_from_str(input);
        assert_eq!(graph.nodes.len(), 3);

        let pillar = &graph.nodes[0];
        assert_eq!(pillar.id, "pillar-memory");
        assert_eq!(pillar.data.human_label.as_deref(), Some("Memory"));
        assert_eq!(pillar.data.purpose.as_deref(), Some("data capture"));
        assert_eq!(
            pillar.data.exports.as_ref().unwrap(),
            &vec!["storage".to_string(), "parser".to_string()]
        );

        let comp0 = &graph.nodes[1];
        assert_eq!(comp0.id, "comp-memory-storage");
        assert_eq!(comp0.parent_id.as_deref(), Some("pillar-memory"));
    }

    #[test]
    fn daemon_with_manages() {
        let input = r#"
(system test
  (daemon my-daemon
    (human-label "Daemon")
    (manages memory control tools)
    (role "orchestrator")))
"#;
        let graph = map_from_str(input);
        assert_eq!(graph.nodes.len(), 1);
        assert_eq!(graph.edges.len(), 3);
        assert_eq!(graph.edges[0].target, "pillar-memory");
    }

    #[test]
    fn communication_rule_creates_edges() {
        let input = r#"
(system test
  (rules (communication (must-use event-bus)))
  (pillar memory (components (storage)))
  (pillar control (components (pty)))
  (pillar tools (components (board))))
"#;
        let graph = map_from_str(input);
        assert_eq!(graph.nodes.len(), 6);
        let events_edges: Vec<_> = graph
            .edges
            .iter()
            .filter(|e| e.label.as_deref() == Some("events"))
            .collect();
        assert_eq!(events_edges.len(), 3);
    }

    #[test]
    fn violations_extraction() {
        let input = r#"
(system test
  (pillar control
    (components (autopilot) (extraction))
    (violations
      (autopilot imports memory/storage "holds Storage ref via IoC")
      (extraction imports memory/storage "holds Storage ref via IoC")))
  (pillar tools
    (components (board))
    (violations
      (board imports memory/storage "cross-pillar; acceptable via IoC"))))
"#;
        let graph = map_from_str(input);
        assert_eq!(graph.violations.len(), 3);
        assert_eq!(graph.violations[0].source, "comp-control-autopilot");
        assert_eq!(graph.violations[0].target, "memory/storage");
        assert_eq!(graph.violations[0].severity, "warning");
        assert_eq!(graph.violations[2].severity, "info");
    }
}
