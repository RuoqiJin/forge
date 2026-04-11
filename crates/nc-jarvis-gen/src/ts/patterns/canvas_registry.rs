//! Canvas Node Registry code generator (Layer 2 Pattern).
//!
//! Takes a list of `CanvasNodeDecl` IR nodes and produces a TypeScript module
//! containing interface definitions and a node type registry array.
//!
//! ## Layer 2 Principle
//!
//! This file delegates interface generation to the `ts_interface` atom
//! and assembles node definitions into a complete registry module.

use nc_jarvis_ir::ts::CanvasNodeDecl;

use crate::common::{w, forge_header, blank};
use crate::ts::atoms::ts_interface::{gen_ts_interface, InterfaceField};

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub fn generate_canvas_registry(nodes: &[CanvasNodeDecl]) -> String {
    let mut out = String::with_capacity(8192);

    forge_header(&mut out, "ts", "intent-canvas.lisp (canvas-node registry)");
    blank(&mut out);

    gen_slot_interface(&mut out);
    blank(&mut out);
    gen_node_type_interface(&mut out);
    blank(&mut out);
    gen_registry(&mut out, nodes);

    out
}

// ---------------------------------------------------------------------------
// NodeSlotDef interface
// ---------------------------------------------------------------------------

fn gen_slot_interface(out: &mut String) {
    let fields = vec![
        InterfaceField {
            name: "name".into(),
            ts_type: "string".into(),
            optional: false,
            description: None,
        },
        InterfaceField {
            name: "type".into(),
            ts_type: "string".into(),
            optional: false,
            description: None,
        },
        InterfaceField {
            name: "required".into(),
            ts_type: "boolean".into(),
            optional: true,
            description: None,
        },
        InterfaceField {
            name: "default".into(),
            ts_type: "unknown".into(),
            optional: true,
            description: None,
        },
        InterfaceField {
            name: "min".into(),
            ts_type: "number".into(),
            optional: true,
            description: None,
        },
        InterfaceField {
            name: "max".into(),
            ts_type: "number".into(),
            optional: true,
            description: None,
        },
        InterfaceField {
            name: "options".into(),
            ts_type: "string[]".into(),
            optional: true,
            description: None,
        },
    ];

    let code = gen_ts_interface("NodeSlotDef", &fields, true);
    out.push_str(&code);
}

// ---------------------------------------------------------------------------
// NodeTypeDef interface
// ---------------------------------------------------------------------------

fn gen_node_type_interface(out: &mut String) {
    let fields = vec![
        InterfaceField {
            name: "name".into(),
            ts_type: "string".into(),
            optional: false,
            description: None,
        },
        InterfaceField {
            name: "category".into(),
            ts_type: "string".into(),
            optional: false,
            description: None,
        },
        InterfaceField {
            name: "description".into(),
            ts_type: "string".into(),
            optional: false,
            description: None,
        },
        InterfaceField {
            name: "inputs".into(),
            ts_type: "NodeSlotDef[]".into(),
            optional: false,
            description: None,
        },
        InterfaceField {
            name: "outputs".into(),
            ts_type: "NodeSlotDef[]".into(),
            optional: false,
            description: None,
        },
    ];

    let code = gen_ts_interface("NodeTypeDef", &fields, true);
    out.push_str(&code);
}

// ---------------------------------------------------------------------------
// NODE_REGISTRY array
// ---------------------------------------------------------------------------

fn gen_registry(out: &mut String, nodes: &[CanvasNodeDecl]) {
    w(out, 0, "export const NODE_REGISTRY: NodeTypeDef[] = [");

    for node in nodes {
        let category = node.category.as_deref().unwrap_or("uncategorized");
        let description = node.description.as_deref().unwrap_or("");

        w(out, 1, "{");
        w(out, 2, &format!("name: \"{}\",", node.name));
        w(out, 2, &format!("category: \"{}\",", category));
        w(out, 2, &format!("description: \"{}\",", escape_quotes(description)));

        // Inputs
        w(out, 2, "inputs: [");
        for slot in &node.inputs {
            gen_slot_literal(out, slot, 3);
        }
        w(out, 2, "],");

        // Outputs
        w(out, 2, "outputs: [");
        for slot in &node.outputs {
            gen_slot_literal(out, slot, 3);
        }
        w(out, 2, "],");

        w(out, 1, "},");
    }

    w(out, 0, "];");
}

fn gen_slot_literal(out: &mut String, slot: &nc_jarvis_ir::ts::NodeSlot, indent: usize) {
    let mut parts = Vec::new();
    parts.push(format!("name: \"{}\"", slot.name));
    parts.push(format!("type: \"{}\"", slot.slot_type));

    if slot.required {
        parts.push("required: true".into());
    }
    if let Some(ref default_val) = slot.default_value {
        parts.push(format!("default: {default_val}"));
    }
    if let Some(min) = slot.min {
        parts.push(format!("min: {}", format_number(min)));
    }
    if let Some(max) = slot.max {
        parts.push(format!("max: {}", format_number(max)));
    }
    if !slot.options.is_empty() {
        let opts: Vec<String> = slot.options.iter().map(|o| format!("\"{o}\"")).collect();
        parts.push(format!("options: [{}]", opts.join(", ")));
    }

    w(out, indent, &format!("{{ {} }},", parts.join(", ")));
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Escape double quotes in strings for safe embedding in JS string literals.
fn escape_quotes(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Format a float, dropping the decimal point when it's a whole number.
fn format_number(n: f64) -> String {
    if n == n.trunc() && n.abs() < i64::MAX as f64 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use nc_jarvis_ir::ts::NodeSlot;

    #[test]
    fn full_registry() {
        let nodes = vec![
            CanvasNodeDecl {
                name: "KSampler".into(),
                category: Some("sampling".into()),
                description: Some("Runs a diffusion sampling step".into()),
                inputs: vec![
                    NodeSlot {
                        name: "model".into(),
                        slot_type: "MODEL".into(),
                        required: true,
                        default_value: None,
                        min: None,
                        max: None,
                        options: vec![],
                        description: None,
                    },
                    NodeSlot {
                        name: "seed".into(),
                        slot_type: "INT".into(),
                        required: false,
                        default_value: Some("0".into()),
                        min: Some(0.0),
                        max: Some(18446744073709551615.0),
                        options: vec![],
                        description: None,
                    },
                    NodeSlot {
                        name: "sampler_name".into(),
                        slot_type: "STRING".into(),
                        required: false,
                        default_value: None,
                        min: None,
                        max: None,
                        options: vec![
                            "euler".into(),
                            "euler_ancestral".into(),
                            "heun".into(),
                        ],
                        description: None,
                    },
                ],
                outputs: vec![NodeSlot {
                    name: "LATENT".into(),
                    slot_type: "LATENT".into(),
                    required: false,
                    default_value: None,
                    min: None,
                    max: None,
                    options: vec![],
                    description: None,
                }],
            },
            CanvasNodeDecl {
                name: "VAEDecode".into(),
                category: Some("latent".into()),
                description: Some("Decode latent to image".into()),
                inputs: vec![NodeSlot {
                    name: "samples".into(),
                    slot_type: "LATENT".into(),
                    required: true,
                    default_value: None,
                    min: None,
                    max: None,
                    options: vec![],
                    description: None,
                }],
                outputs: vec![NodeSlot {
                    name: "IMAGE".into(),
                    slot_type: "IMAGE".into(),
                    required: false,
                    default_value: None,
                    min: None,
                    max: None,
                    options: vec![],
                    description: None,
                }],
            },
        ];

        let code = generate_canvas_registry(&nodes);

        // Header
        assert!(code.contains("GENERATED BY FORGE - DO NOT EDIT"));

        // Interfaces
        assert!(code.contains("export interface NodeSlotDef {"));
        assert!(code.contains("export interface NodeTypeDef {"));

        // Registry
        assert!(code.contains("export const NODE_REGISTRY: NodeTypeDef[] = ["));
        assert!(code.contains("name: \"KSampler\""));
        assert!(code.contains("category: \"sampling\""));
        assert!(code.contains("{ name: \"model\", type: \"MODEL\", required: true },"));
        assert!(code.contains("default: 0"));
        assert!(code.contains("options: [\"euler\", \"euler_ancestral\", \"heun\"]"));
        assert!(code.contains("name: \"VAEDecode\""));
        assert!(code.contains("];"));
    }

    #[test]
    fn empty_registry() {
        let code = generate_canvas_registry(&[]);
        assert!(code.contains("export const NODE_REGISTRY: NodeTypeDef[] = ["));
        assert!(code.contains("];"));
    }

    #[test]
    fn format_number_integers() {
        assert_eq!(format_number(0.0), "0");
        assert_eq!(format_number(42.0), "42");
        assert_eq!(format_number(8.5), "8.5");
    }

    #[test]
    fn escape_quotes_works() {
        assert_eq!(escape_quotes("hello \"world\""), "hello \\\"world\\\"");
        assert_eq!(escape_quotes("no quotes"), "no quotes");
    }
}
