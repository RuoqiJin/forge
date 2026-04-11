//! Canvas Node IR — typed representation of `(canvas-node ...)` declarations.
//!
//! A canvas node is a processing unit in a node-graph system (e.g. ComfyUI,
//! LiteGraph) with typed input/output slots, numeric constraints, and COMBO
//! option lists.
//!
//! Generates: TypeScript node registry array with typed slot definitions.

use nc_parser::SexpNode;

// ---------------------------------------------------------------------------
// Declaration types
// ---------------------------------------------------------------------------

/// Top-level canvas node declaration.
///
/// Parsed from:
/// ```lisp
/// (canvas-node KSampler
///   :category "sampling"
///   :description "Runs a diffusion sampling step"
///   (inputs ...)
///   (outputs ...))
/// ```
#[derive(Debug, Clone)]
pub struct CanvasNodeDecl {
    pub name: String,
    pub category: Option<String>,
    pub description: Option<String>,
    pub inputs: Vec<NodeSlot>,
    pub outputs: Vec<NodeSlot>,
}

/// A typed input or output slot on a canvas node.
///
/// Parsed from a child of `(inputs ...)` or `(outputs ...)`:
/// ```lisp
/// (seed INT :default 0)
/// (steps INT :default 20 :min 1 :max 10000)
/// (sampler_name COMBO :options (euler euler_ancestral heun dpm_2))
/// ```
#[derive(Debug, Clone)]
pub struct NodeSlot {
    pub name: String,
    /// Slot type: MODEL, CONDITIONING, LATENT, INT, FLOAT, STRING, COMBO, BOOLEAN, IMAGE, MASK
    pub slot_type: String,
    pub required: bool,
    pub default_value: Option<String>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    /// Option values for COMBO type slots.
    pub options: Vec<String>,
    pub description: Option<String>,
}

// ---------------------------------------------------------------------------
// Allowed slot types (whitelist)
// ---------------------------------------------------------------------------

const ALLOWED_SLOT_TYPES: &[&str] = &[
    "MODEL", "CONDITIONING", "LATENT", "INT", "FLOAT", "STRING",
    "COMBO", "BOOLEAN", "IMAGE", "MASK",
];

fn validate_slot_type(ty: &str) -> Result<(), String> {
    if ALLOWED_SLOT_TYPES.contains(&ty) {
        Ok(())
    } else {
        Err(format!(
            "unknown slot type `{ty}` (expected one of: {})",
            ALLOWED_SLOT_TYPES.join(", ")
        ))
    }
}

// ---------------------------------------------------------------------------
// S-EXP -> IR lowering
// ---------------------------------------------------------------------------

impl CanvasNodeDecl {
    /// Parse a single `(canvas-node ...)` S-expression into IR.
    pub fn from_sexp(node: &SexpNode) -> Result<Self, String> {
        let children = expect_list(node)?;
        expect_tag(node, "canvas-node")?;

        if children.len() < 2 {
            return Err("canvas-node: missing node name".into());
        }
        let name = expect_atom(&children[1])?.to_string();

        let category = node.keyword(":category").map(String::from);
        let description = node.keyword(":description").map(String::from);

        let inputs = match node.find("inputs") {
            Some(inputs_node) => parse_slots(inputs_node, &name, "input")?,
            None => vec![],
        };

        let outputs = match node.find("outputs") {
            Some(outputs_node) => parse_slots(outputs_node, &name, "output")?,
            None => vec![],
        };

        Ok(CanvasNodeDecl {
            name,
            category,
            description,
            inputs,
            outputs,
        })
    }
}

/// Parse all `(canvas-node ...)` declarations from a list of top-level nodes.
pub fn parse_canvas_nodes(nodes: &[SexpNode]) -> Result<Vec<CanvasNodeDecl>, String> {
    nodes
        .iter()
        .filter(|n| n.tag() == Some("canvas-node"))
        .map(CanvasNodeDecl::from_sexp)
        .collect()
}

// ---------------------------------------------------------------------------
// Sub-parsers
// ---------------------------------------------------------------------------

fn parse_slots(node: &SexpNode, node_name: &str, kind: &str) -> Result<Vec<NodeSlot>, String> {
    let children = expect_list(node)?;
    let mut slots = Vec::new();

    for child in children.iter().skip(1) {
        // skip "inputs"/"outputs" tag
        if let SexpNode::List(parts) = child {
            if parts.len() < 2 {
                continue;
            }
            let slot_name = expect_atom(&parts[0])
                .map_err(|e| format!("canvas-node `{node_name}` {kind}: {e}"))?
                .to_string();
            if slot_name.starts_with(':') {
                continue;
            }
            let slot_type = expect_atom(&parts[1])
                .map_err(|e| {
                    format!("canvas-node `{node_name}` {kind} `{slot_name}`: {e}")
                })?
                .to_string();

            validate_slot_type(&slot_type).map_err(|e| {
                format!("canvas-node `{node_name}` {kind} `{slot_name}`: {e}")
            })?;

            let required = has_flag(child, ":required");
            let default_value = child.keyword(":default").map(String::from);
            let min = child
                .keyword(":min")
                .and_then(|v| v.parse::<f64>().ok());
            let max = child
                .keyword(":max")
                .and_then(|v| v.parse::<f64>().ok());
            let options = parse_options_list(child);
            let description = child.keyword(":description").map(String::from);

            slots.push(NodeSlot {
                name: slot_name,
                slot_type,
                required,
                default_value,
                min,
                max,
                options,
                description,
            });
        }
    }
    Ok(slots)
}

/// Extract the list of option atoms from `:options (a b c)`.
fn parse_options_list(node: &SexpNode) -> Vec<String> {
    match node.keyword_node(":options") {
        Some(SexpNode::List(items)) => items
            .iter()
            .filter_map(|item| match item {
                SexpNode::Atom(s) => Some(s.clone()),
                _ => None,
            })
            .collect(),
        _ => vec![],
    }
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
    fn parse_full_canvas_node() {
        let sexp = parse(
            r#"(canvas-node KSampler
              :category "sampling"
              :description "Runs a diffusion sampling step"
              (inputs
                (model MODEL :required)
                (positive CONDITIONING :required)
                (negative CONDITIONING :required)
                (latent_image LATENT :required)
                (seed INT :default 0)
                (steps INT :default 20 :min 1 :max 10000)
                (cfg FLOAT :default 8.0 :min 0.0 :max 100.0)
                (sampler_name COMBO :options (euler euler_ancestral heun dpm_2))
                (scheduler COMBO :options (normal karras exponential)))
              (outputs
                (LATENT LATENT)))"#,
        );
        let node = CanvasNodeDecl::from_sexp(&sexp).unwrap();

        assert_eq!(node.name, "KSampler");
        assert_eq!(node.category, Some("sampling".into()));
        assert_eq!(node.description, Some("Runs a diffusion sampling step".into()));

        assert_eq!(node.inputs.len(), 9);
        assert_eq!(node.inputs[0].name, "model");
        assert_eq!(node.inputs[0].slot_type, "MODEL");
        assert!(node.inputs[0].required);
        assert_eq!(node.inputs[0].default_value, None);

        // seed: INT :default 0
        assert_eq!(node.inputs[4].name, "seed");
        assert_eq!(node.inputs[4].slot_type, "INT");
        assert!(!node.inputs[4].required);
        assert_eq!(node.inputs[4].default_value, Some("0".into()));

        // steps: INT :default 20 :min 1 :max 10000
        assert_eq!(node.inputs[5].name, "steps");
        assert_eq!(node.inputs[5].min, Some(1.0));
        assert_eq!(node.inputs[5].max, Some(10000.0));

        // cfg: FLOAT :default 8.0 :min 0.0 :max 100.0
        assert_eq!(node.inputs[6].name, "cfg");
        assert_eq!(node.inputs[6].slot_type, "FLOAT");
        assert_eq!(node.inputs[6].default_value, Some("8.0".into()));

        // sampler_name: COMBO with options
        assert_eq!(node.inputs[7].name, "sampler_name");
        assert_eq!(node.inputs[7].slot_type, "COMBO");
        assert_eq!(
            node.inputs[7].options,
            vec!["euler", "euler_ancestral", "heun", "dpm_2"]
        );

        assert_eq!(node.outputs.len(), 1);
        assert_eq!(node.outputs[0].name, "LATENT");
        assert_eq!(node.outputs[0].slot_type, "LATENT");
    }

    #[test]
    fn parse_minimal_canvas_node() {
        let sexp = parse(r#"(canvas-node EmptyNode)"#);
        let node = CanvasNodeDecl::from_sexp(&sexp).unwrap();

        assert_eq!(node.name, "EmptyNode");
        assert_eq!(node.category, None);
        assert_eq!(node.description, None);
        assert!(node.inputs.is_empty());
        assert!(node.outputs.is_empty());
    }

    #[test]
    fn parse_outputs_only() {
        let sexp = parse(
            r#"(canvas-node LoadImage
              :category "image"
              (outputs
                (IMAGE IMAGE)
                (MASK MASK)))"#,
        );
        let node = CanvasNodeDecl::from_sexp(&sexp).unwrap();

        assert_eq!(node.name, "LoadImage");
        assert!(node.inputs.is_empty());
        assert_eq!(node.outputs.len(), 2);
        assert_eq!(node.outputs[0].slot_type, "IMAGE");
        assert_eq!(node.outputs[1].slot_type, "MASK");
    }

    #[test]
    fn reject_invalid_slot_type() {
        let sexp = parse(
            r#"(canvas-node BadNode
              (inputs
                (data MONGODB_CURSOR :required)))"#,
        );
        let err = CanvasNodeDecl::from_sexp(&sexp).unwrap_err();
        assert!(err.contains("unknown slot type"), "got: {err}");
    }

    #[test]
    fn parse_multiple_canvas_nodes() {
        let input = r#"
          (canvas-node NodeA
            :category "util"
            (inputs
              (x INT :default 0)))

          (canvas-node NodeB
            (outputs
              (result STRING)))

          ; non-canvas-node should be ignored
          (external-engine viewport :import-from "three")
        "#;
        let nodes = SexpNode::parse(input).unwrap();
        let canvas_nodes = parse_canvas_nodes(&nodes).unwrap();

        assert_eq!(canvas_nodes.len(), 2);
        assert_eq!(canvas_nodes[0].name, "NodeA");
        assert_eq!(canvas_nodes[1].name, "NodeB");
    }
}
