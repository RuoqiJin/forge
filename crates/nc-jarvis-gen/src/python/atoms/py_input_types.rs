//! Atom: py-input-types
//!
//! Generates ComfyUI `INPUT_TYPES()` classmethod.
//! This is the most complex Python atom due to 4-level nested dict indentation.
//!
//! CRITICAL: Python indentation MUST be pixel-perfect.
//! w(out, level, ...) must match exactly for every nested scope.

use crate::common::w;
use nc_jarvis_ir::python::comfy_node::{ComfyInputDecl, ComfyHiddenDecl, ComfyIOType};

/// Generate the `INPUT_TYPES` classmethod body.
///
/// Indentation levels:
/// - 0: @classmethod + def
/// - 1: return {
/// - 2: "required": {
/// - 3: "param": (IO.TYPE, {opts}),
pub fn gen_input_types(
    required: &[ComfyInputDecl],
    optional: &[ComfyInputDecl],
    hidden: &[ComfyHiddenDecl],
) -> String {
    let mut out = String::with_capacity(2048);

    w(&mut out, 1, "@classmethod");
    w(&mut out, 1, "def INPUT_TYPES(cls) -> InputTypeDict:");
    w(&mut out, 2, "return {");

    // Required inputs
    if !required.is_empty() {
        w(&mut out, 3, "\"required\": {");
        for input in required {
            let line = format_input(input);
            w(&mut out, 4, &format!("\"{}\": {},", input.name, line));
        }
        w(&mut out, 3, "},");
    }

    // Optional inputs
    if !optional.is_empty() {
        w(&mut out, 3, "\"optional\": {");
        for input in optional {
            let line = format_input(input);
            w(&mut out, 4, &format!("\"{}\": {},", input.name, line));
        }
        w(&mut out, 3, "},");
    }

    // Hidden inputs
    if !hidden.is_empty() {
        w(&mut out, 3, "\"hidden\": {");
        for h in hidden {
            w(&mut out, 4, &format!("\"{}\": \"{}\",", h.name, h.hidden_type));
        }
        w(&mut out, 3, "},");
    }

    w(&mut out, 2, "}");

    out
}

/// Format a single input declaration: `(IO.TYPE, {options})` or `(["opt1", ...], {options})`
fn format_input(input: &ComfyInputDecl) -> String {
    let type_expr = format_type_expr(input);
    let opts = format_options(&input.options, &input.io_type);

    if opts.is_empty() {
        format!("({type_expr},)")
    } else {
        format!("({type_expr}, {{{opts}}})")
    }
}

/// Format the type expression part:
/// - Normal types: `IO.STRING`
/// - COMBO with folder_path: `folder_paths.get_filename_list("checkpoints")`
/// - COMBO with options: `["opt1", "opt2"]`
fn format_type_expr(input: &ComfyInputDecl) -> String {
    if input.io_type == ComfyIOType::Combo {
        if let Some(ref fp) = input.options.folder_path {
            return format!("folder_paths.get_filename_list(\"{fp}\")");
        }
        if !input.options.options.is_empty() {
            let vals: Vec<String> = input
                .options
                .options
                .iter()
                .map(|v| format!("\"{v}\""))
                .collect();
            return format!("[{}]", vals.join(", "));
        }
    }
    input.io_type.to_io_expr().to_string()
}

/// Format the options dict contents (only non-default values).
fn format_options(opts: &nc_jarvis_ir::python::comfy_node::ComfyInputOptions, io_type: &ComfyIOType) -> String {
    let mut parts: Vec<String> = vec![];

    if let Some(ref d) = opts.default {
        let val = format_default_value(d, io_type);
        parts.push(format!("\"default\": {val}"));
    }
    if let Some(min) = opts.min {
        parts.push(format!("\"min\": {}", format_number(min)));
    }
    if let Some(max) = opts.max {
        parts.push(format!("\"max\": {}", format_number(max)));
    }
    if let Some(step) = opts.step {
        parts.push(format!("\"step\": {}", format_number(step)));
    }
    if let Some(round) = opts.round {
        parts.push(format!("\"round\": {}", format_number(round)));
    }
    if opts.multiline {
        parts.push("\"multiline\": True".into());
    }
    if opts.dynamic_prompts {
        parts.push("\"dynamicPrompts\": True".into());
    }
    if opts.control_after_generate {
        parts.push("\"control_after_generate\": True".into());
    }
    if let Some(ref t) = opts.tooltip {
        parts.push(format!("\"tooltip\": \"{t}\""));
    }
    if let Some(ref l) = opts.label_on {
        parts.push(format!("\"label_on\": \"{l}\""));
    }
    if let Some(ref l) = opts.label_off {
        parts.push(format!("\"label_off\": \"{l}\""));
    }

    parts.join(", ")
}

/// Format a default value for Python:
/// - Strings: `"value"`
/// - Numbers: `512`, `8.0`
/// - Booleans: `True`, `False`
fn format_default_value(val: &str, io_type: &ComfyIOType) -> String {
    match io_type {
        ComfyIOType::String => format!("\"{val}\""),
        ComfyIOType::Boolean => {
            if val == "true" { "True".into() } else { "False".into() }
        }
        ComfyIOType::Int | ComfyIOType::Float => {
            // Keep as-is for numeric types
            val.to_string()
        }
        _ => format!("\"{val}\""),
    }
}

/// Format a float as integer if it has no fractional part.
fn format_number(n: f64) -> String {
    if n == n.floor() && n.abs() < 1e15 {
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
    use nc_jarvis_ir::python::comfy_node::ComfyInputOptions;

    #[test]
    fn simple_required_inputs() {
        let code = gen_input_types(
            &[
                ComfyInputDecl {
                    name: "text".into(),
                    io_type: ComfyIOType::String,
                    options: ComfyInputOptions {
                        multiline: true,
                        tooltip: Some("The text to encode".into()),
                        ..Default::default()
                    },
                },
                ComfyInputDecl {
                    name: "clip".into(),
                    io_type: ComfyIOType::Clip,
                    options: ComfyInputOptions::default(),
                },
            ],
            &[],
            &[],
        );

        assert!(code.contains("@classmethod"));
        assert!(code.contains("def INPUT_TYPES(cls) -> InputTypeDict:"));
        assert!(code.contains("\"required\": {"));
        assert!(code.contains("\"text\": (IO.STRING, {\"multiline\": True, \"tooltip\": \"The text to encode\"}),"));
        assert!(code.contains("\"clip\": (IO.CLIP,),"));
    }

    #[test]
    fn combo_with_folder_path() {
        let code = gen_input_types(
            &[ComfyInputDecl {
                name: "ckpt_name".into(),
                io_type: ComfyIOType::Combo,
                options: ComfyInputOptions {
                    folder_path: Some("checkpoints".into()),
                    ..Default::default()
                },
            }],
            &[],
            &[],
        );

        assert!(code.contains("folder_paths.get_filename_list(\"checkpoints\")"));
    }

    #[test]
    fn combo_with_static_options() {
        let code = gen_input_types(
            &[ComfyInputDecl {
                name: "method".into(),
                io_type: ComfyIOType::Combo,
                options: ComfyInputOptions {
                    options: vec!["bilinear".into(), "area".into(), "bicubic".into()],
                    ..Default::default()
                },
            }],
            &[],
            &[],
        );

        assert!(code.contains("[\"bilinear\", \"area\", \"bicubic\"]"));
    }

    #[test]
    fn hidden_inputs() {
        let code = gen_input_types(
            &[],
            &[],
            &[
                ComfyHiddenDecl { name: "prompt".into(), hidden_type: "PROMPT".into() },
                ComfyHiddenDecl { name: "extra_pnginfo".into(), hidden_type: "EXTRA_PNGINFO".into() },
            ],
        );

        assert!(code.contains("\"hidden\": {"));
        assert!(code.contains("\"prompt\": \"PROMPT\","));
        assert!(code.contains("\"extra_pnginfo\": \"EXTRA_PNGINFO\","));
    }

    #[test]
    fn numeric_defaults() {
        let code = gen_input_types(
            &[ComfyInputDecl {
                name: "width".into(),
                io_type: ComfyIOType::Int,
                options: ComfyInputOptions {
                    default: Some("512".into()),
                    min: Some(16.0),
                    max: Some(16384.0),
                    step: Some(8.0),
                    ..Default::default()
                },
            }],
            &[],
            &[],
        );

        assert!(code.contains("\"default\": 512"));
        assert!(code.contains("\"min\": 16"));
        assert!(code.contains("\"max\": 16384"));
        assert!(code.contains("\"step\": 8"));
    }

    #[test]
    fn indentation_correct() {
        let code = gen_input_types(
            &[ComfyInputDecl {
                name: "x".into(),
                io_type: ComfyIOType::Int,
                options: ComfyInputOptions::default(),
            }],
            &[],
            &[],
        );

        // Verify exact indentation (4 spaces per level)
        assert!(code.contains("    @classmethod\n"));
        assert!(code.contains("    def INPUT_TYPES(cls) -> InputTypeDict:\n"));
        assert!(code.contains("        return {\n"));
        assert!(code.contains("            \"required\": {\n"));
        assert!(code.contains("                \"x\": (IO.INT,),\n"));
        assert!(code.contains("            },\n"));
        assert!(code.contains("        }\n"));
    }
}
