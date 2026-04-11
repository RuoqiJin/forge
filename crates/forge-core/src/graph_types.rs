//! Graph domain types for architecture visualization.
//!
//! Ported from jarvis-core::memory::code_graph — the subset needed by
//! the S-EXP mapper and the code-map API endpoints.

use serde::Serialize;

// ── Top-level graph container ────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct MappedGraph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub violations: Vec<Violation>,
}

// ── Node ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct GraphNode {
    pub id: String,
    #[serde(rename = "type")]
    pub node_type: GraphNodeType,
    pub label: String,
    #[serde(rename = "parentId", skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<Position>,
    pub data: NodeData,
}

#[derive(Debug, Clone, Serialize)]
pub struct Position {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone)]
pub enum GraphNodeType {
    Infrastructure,
    Pillar,
    Component,
    Symbol,
    Submodule,
}

impl Serialize for GraphNodeType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            GraphNodeType::Infrastructure => serializer.serialize_str("infrastructure"),
            GraphNodeType::Pillar => serializer.serialize_str("pillar"),
            GraphNodeType::Component => serializer.serialize_str("component"),
            GraphNodeType::Symbol => serializer.serialize_str("symbol"),
            GraphNodeType::Submodule => serializer.serialize_str("submodule"),
        }
    }
}

// ── NodeData ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct NodeData {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub human_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub human_description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub purpose: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exports: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub beacons: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub events: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol_count: Option<usize>,
}

// ── Edge ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct GraphEdge {
    pub id: String,
    pub source: String,
    pub target: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(rename = "edgeType", skip_serializing_if = "Option::is_none")]
    pub edge_type: Option<String>,
}

// ── Violation ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct Violation {
    pub source: String,
    pub target: String,
    pub description: String,
    pub severity: String,
}

// ── Trace (placeholder for future use) ───────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct Trace {
    pub id: String,
    pub path: Vec<String>,
    pub label: String,
}
