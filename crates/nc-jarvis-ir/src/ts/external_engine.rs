//! External Engine IR — typed representation of `(external-engine ...)` declarations.
//!
//! An external engine is a "black-box" interaction component (e.g. ReactFlow canvas,
//! Three.js scene, LiteGraph editor) that Forge does NOT generate, but generates
//! contracts and adapters around: typed props interface, event handler wiring,
//! and trait implementations.
//!
//! Generates: TypeScript props interface + event type union + adapter wrapper component.

use nc_parser::SexpNode;

// ---------------------------------------------------------------------------
// Declaration types
// ---------------------------------------------------------------------------

/// Top-level external engine declaration.
///
/// Parsed from:
/// ```lisp
/// (external-engine forge-canvas
///   :import-from "@xyflow/react"
///   :implements-trait ICanvasEngine
///   (accepts-props ...)
///   (emits-events ...))
/// ```
#[derive(Debug, Clone)]
pub struct ExternalEngineDecl {
    pub name: String,
    pub import_from: String,
    pub implements_trait: Option<String>,
    pub props: Vec<EngineProp>,
    pub events: Vec<EngineEvent>,
}

/// A typed prop accepted by the external engine.
///
/// Parsed from a child of `(accepts-props ...)`:
/// ```lisp
/// (nodes array :required :description "ReactFlow node array")
/// ```
#[derive(Debug, Clone)]
pub struct EngineProp {
    pub name: String,
    /// TypeScript type string: "array", "function", "string", "number", "boolean", "object"
    pub ts_type: String,
    pub required: bool,
    pub description: Option<String>,
}

/// An event emitted by the external engine.
///
/// Parsed from a child of `(emits-events ...)`:
/// ```lisp
/// (node-selected :payload "{ nodeId: string }")
/// ```
#[derive(Debug, Clone)]
pub struct EngineEvent {
    pub name: String,
    /// TypeScript type literal for the event payload.
    pub payload: Option<String>,
}

// ---------------------------------------------------------------------------
// Allowed TS types (whitelist)
// ---------------------------------------------------------------------------

const ALLOWED_TS_TYPES: &[&str] = &[
    "array", "function", "string", "number", "boolean", "object",
];

fn validate_ts_type(ty: &str) -> Result<(), String> {
    if ALLOWED_TS_TYPES.contains(&ty) {
        Ok(())
    } else {
        Err(format!(
            "unknown TS type `{ty}` (expected one of: {})",
            ALLOWED_TS_TYPES.join(", ")
        ))
    }
}

// ---------------------------------------------------------------------------
// S-EXP → IR lowering
// ---------------------------------------------------------------------------

impl ExternalEngineDecl {
    /// Parse a single `(external-engine ...)` S-expression into IR.
    pub fn from_sexp(node: &SexpNode) -> Result<Self, String> {
        let children = expect_list(node)?;
        expect_tag(node, "external-engine")?;

        if children.len() < 2 {
            return Err("external-engine: missing engine name".into());
        }
        let name = expect_atom(&children[1])?.to_string();

        let import_from = node
            .keyword(":import-from")
            .map(String::from)
            .ok_or_else(|| format!("external-engine `{name}`: missing :import-from"))?;

        let implements_trait = node.keyword(":implements-trait").map(String::from);

        let props = match node.find("accepts-props") {
            Some(props_node) => parse_props(props_node, &name)?,
            None => vec![],
        };

        let events = match node.find("emits-events") {
            Some(events_node) => parse_events(events_node, &name)?,
            None => vec![],
        };

        Ok(ExternalEngineDecl {
            name,
            import_from,
            implements_trait,
            props,
            events,
        })
    }
}

/// Parse all `(external-engine ...)` declarations from a list of top-level nodes.
pub fn parse_external_engines(nodes: &[SexpNode]) -> Result<Vec<ExternalEngineDecl>, String> {
    nodes
        .iter()
        .filter(|n| n.tag() == Some("external-engine"))
        .map(ExternalEngineDecl::from_sexp)
        .collect()
}

// ---------------------------------------------------------------------------
// Sub-parsers
// ---------------------------------------------------------------------------

fn parse_props(node: &SexpNode, engine_name: &str) -> Result<Vec<EngineProp>, String> {
    let children = expect_list(node)?;
    let mut props = Vec::new();

    for child in children.iter().skip(1) {
        // skip "accepts-props" tag
        if let SexpNode::List(parts) = child {
            if parts.len() < 2 {
                continue;
            }
            let prop_name = expect_atom(&parts[0])
                .map_err(|e| format!("external-engine `{engine_name}` prop: {e}"))?
                .to_string();
            // Skip keywords that might appear as direct children
            if prop_name.starts_with(':') {
                continue;
            }
            let ts_type = expect_atom(&parts[1])
                .map_err(|e| format!("external-engine `{engine_name}` prop `{prop_name}`: {e}"))?
                .to_string();

            validate_ts_type(&ts_type)
                .map_err(|e| format!("external-engine `{engine_name}` prop `{prop_name}`: {e}"))?;

            let required = has_flag(child, ":required");
            let description = child.keyword(":description").map(String::from);

            props.push(EngineProp {
                name: prop_name,
                ts_type,
                required,
                description,
            });
        }
    }
    Ok(props)
}

fn parse_events(node: &SexpNode, engine_name: &str) -> Result<Vec<EngineEvent>, String> {
    let children = expect_list(node)?;
    let mut events = Vec::new();

    for child in children.iter().skip(1) {
        // skip "emits-events" tag
        if let SexpNode::List(parts) = child {
            if parts.is_empty() {
                continue;
            }
            let event_name = expect_atom(&parts[0])
                .map_err(|e| format!("external-engine `{engine_name}` event: {e}"))?
                .to_string();
            if event_name.starts_with(':') {
                continue;
            }
            let payload = child.keyword(":payload").map(String::from);

            events.push(EngineEvent {
                name: event_name,
                payload,
            });
        }
    }
    Ok(events)
}

// ---------------------------------------------------------------------------
// Utility helpers
// ---------------------------------------------------------------------------

fn expect_tag<'a>(node: &'a SexpNode, expected: &str) -> Result<&'a str, String> {
    let tag = node
        .tag()
        .ok_or_else(|| format!("expected list with tag `{expected}`, got atom"))?;
    if tag != expected {
        return Err(format!("expected tag `{expected}`, got `{tag}`"));
    }
    Ok(tag)
}

fn expect_list(node: &SexpNode) -> Result<&Vec<SexpNode>, String> {
    match node {
        SexpNode::List(c) => Ok(c),
        SexpNode::Atom(s) => Err(format!("expected list, got atom `{s}`")),
    }
}

fn expect_atom<'a>(node: &'a SexpNode) -> Result<&'a str, String> {
    match node {
        SexpNode::Atom(s) => Ok(s),
        SexpNode::List(_) => Err("expected atom, got list".into()),
    }
}

fn has_flag(node: &SexpNode, key: &str) -> bool {
    if let SexpNode::List(children) = node {
        children
            .iter()
            .any(|c| matches!(c, SexpNode::Atom(s) if s == key))
    } else {
        false
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(input: &str) -> SexpNode {
        SexpNode::parse_one(input).expect("valid S-EXP")
    }

    #[test]
    fn parse_full_external_engine() {
        let sexp = parse(
            r#"(external-engine forge-canvas
              :import-from "@xyflow/react"
              :implements-trait ICanvasEngine
              (accepts-props
                (nodes array :required :description "ReactFlow node array")
                (edges array :required :description "ReactFlow edge array")
                (onNodeClick function :description "Node click callback"))
              (emits-events
                (node-selected :payload "{ nodeId: string }")
                (layout-changed :payload "{ nodes: Node[] }")))"#,
        );
        let engine = ExternalEngineDecl::from_sexp(&sexp).unwrap();

        assert_eq!(engine.name, "forge-canvas");
        assert_eq!(engine.import_from, "@xyflow/react");
        assert_eq!(engine.implements_trait, Some("ICanvasEngine".into()));

        assert_eq!(engine.props.len(), 3);
        assert_eq!(engine.props[0].name, "nodes");
        assert_eq!(engine.props[0].ts_type, "array");
        assert!(engine.props[0].required);
        assert_eq!(
            engine.props[0].description,
            Some("ReactFlow node array".into())
        );
        assert_eq!(engine.props[1].name, "edges");
        assert!(engine.props[1].required);
        assert_eq!(engine.props[2].name, "onNodeClick");
        assert_eq!(engine.props[2].ts_type, "function");
        assert!(!engine.props[2].required);

        assert_eq!(engine.events.len(), 2);
        assert_eq!(engine.events[0].name, "node-selected");
        assert_eq!(
            engine.events[0].payload,
            Some("{ nodeId: string }".into())
        );
        assert_eq!(engine.events[1].name, "layout-changed");
        assert_eq!(
            engine.events[1].payload,
            Some("{ nodes: Node[] }".into())
        );
    }

    #[test]
    fn parse_minimal_engine() {
        let sexp = parse(
            r#"(external-engine three-viewport
              :import-from "three")"#,
        );
        let engine = ExternalEngineDecl::from_sexp(&sexp).unwrap();

        assert_eq!(engine.name, "three-viewport");
        assert_eq!(engine.import_from, "three");
        assert_eq!(engine.implements_trait, None);
        assert!(engine.props.is_empty());
        assert!(engine.events.is_empty());
    }

    #[test]
    fn parse_engine_with_optional_props() {
        let sexp = parse(
            r#"(external-engine litegraph-editor
              :import-from "litegraph.js"
              (accepts-props
                (graph object :required :description "LiteGraph graph instance")
                (theme string :description "Light or dark theme")))"#,
        );
        let engine = ExternalEngineDecl::from_sexp(&sexp).unwrap();

        assert_eq!(engine.props.len(), 2);
        assert!(engine.props[0].required);
        assert!(!engine.props[1].required);
        assert_eq!(engine.props[1].ts_type, "string");
    }

    #[test]
    fn parse_events_without_payload() {
        let sexp = parse(
            r#"(external-engine audio-engine
              :import-from "tone"
              (emits-events
                (playback-started)
                (playback-stopped :payload "{ position: number }")))"#,
        );
        let engine = ExternalEngineDecl::from_sexp(&sexp).unwrap();

        assert_eq!(engine.events.len(), 2);
        assert_eq!(engine.events[0].name, "playback-started");
        assert_eq!(engine.events[0].payload, None);
        assert_eq!(engine.events[1].name, "playback-stopped");
        assert!(engine.events[1].payload.is_some());
    }

    #[test]
    fn reject_missing_import_from() {
        let sexp = parse(r#"(external-engine bad-engine)"#);
        let err = ExternalEngineDecl::from_sexp(&sexp).unwrap_err();
        assert!(err.contains("missing :import-from"), "got: {err}");
    }

    #[test]
    fn reject_invalid_ts_type() {
        let sexp = parse(
            r#"(external-engine bad-types
              :import-from "bad"
              (accepts-props
                (x mongodb_cursor :required)))"#,
        );
        let err = ExternalEngineDecl::from_sexp(&sexp).unwrap_err();
        assert!(err.contains("unknown TS type"), "got: {err}");
    }

    #[test]
    fn parse_multiple_engines() {
        let input = r#"
          (external-engine canvas
            :import-from "@xyflow/react"
            (accepts-props
              (nodes array :required)))

          (external-engine viewport
            :import-from "three"
            (emits-events
              (frame-rendered)))

          ; non-engine node should be ignored
          (component sidebar :layout vertical)
        "#;
        let nodes = SexpNode::parse(input).unwrap();
        let engines = parse_external_engines(&nodes).unwrap();

        assert_eq!(engines.len(), 2);
        assert_eq!(engines[0].name, "canvas");
        assert_eq!(engines[1].name, "viewport");
    }
}
