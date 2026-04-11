//! Atom: py-schema-node
//!
//! Generates ComfyUI V3 API `define_schema()` and `IO.Schema(...)` code.
//! The V3 API is cleaner than legacy INPUT_TYPES — everything in one Schema call.
//!
//! ## What it generates
//!
//! ```python
//! class Canny_ForgeBase(io.ComfyNode):
//!     @classmethod
//!     def define_schema(cls):
//!         return io.Schema(
//!             node_id="Canny",
//!             display_name="Canny",
//!             category="image/preprocessors",
//!             inputs=[
//!                 io.Image.Input("image"),
//!                 io.Float.Input("low_threshold", default=0.4, min=0.01, max=0.99, step=0.01),
//!             ],
//!             outputs=[io.Image.Output()],
//!         )
//! ```

use crate::common::w;
use nc_jarvis_ir::python::comfy_node::{ComfyNodeDecl, ComfyInputDecl, ComfyIOType, ComfyInputOptions};

/// Generate the V3 `define_schema()` classmethod.
///
/// Indentation levels:
/// - 1: @classmethod + def define_schema
/// - 2: return io.Schema(
/// - 3: node_id=..., category=..., inputs=[
/// - 4: io.Type.Input("name", opts),
pub fn gen_schema_node(decl: &ComfyNodeDecl) -> String {
    let mut out = String::with_capacity(2048);

    // Class definition
    w(&mut out, 0, &format!("class {}_ForgeBase(io.ComfyNode):", decl.class_name));

    // define_schema method
    w(&mut out, 1, "@classmethod");
    w(&mut out, 1, "def define_schema(cls):");
    w(&mut out, 2, "return io.Schema(");

    // Schema fields
    w(&mut out, 3, &format!("node_id=\"{}\",", decl.class_name));

    if let Some(ref dn) = decl.display_name {
        w(&mut out, 3, &format!("display_name=\"{dn}\","));
    }

    w(&mut out, 3, &format!("category=\"{}\",", decl.category));

    if let Some(ref desc) = decl.description {
        w(&mut out, 3, &format!("description=\"{desc}\","));
    }

    if !decl.search_aliases.is_empty() {
        let aliases: Vec<String> = decl.search_aliases.iter().map(|a| format!("\"{a}\"")).collect();
        w(&mut out, 3, &format!("search_aliases=[{}],", aliases.join(", ")));
    }

    if decl.deprecated {
        w(&mut out, 3, "is_deprecated=True,");
    }

    if decl.output_node {
        w(&mut out, 3, "is_output_node=True,");
    }

    // Inputs
    let all_inputs: Vec<&ComfyInputDecl> = decl.required.iter().chain(decl.optional.iter()).collect();
    if !all_inputs.is_empty() {
        w(&mut out, 3, "inputs=[");
        for input in &decl.required {
            let line = format_v3_input(input, false);
            w(&mut out, 4, &format!("{line},"));
        }
        for input in &decl.optional {
            let line = format_v3_input(input, true);
            w(&mut out, 4, &format!("{line},"));
        }
        w(&mut out, 3, "],");
    }

    // Outputs
    if !decl.return_types.is_empty() {
        w(&mut out, 3, "outputs=[");
        for (i, rt) in decl.return_types.iter().enumerate() {
            let name = decl.return_names.get(i).map(|n| format!("\"{}\"", n));
            let line = format_v3_output(rt, name.as_deref());
            w(&mut out, 4, &format!("{line},"));
        }
        w(&mut out, 3, "],");
    }

    // Hidden inputs
    if !decl.hidden.is_empty() {
        w(&mut out, 3, "hidden=[");
        for h in &decl.hidden {
            w(&mut out, 4, &format!("io.Hidden.{},", h.hidden_type.to_lowercase()));
        }
        w(&mut out, 3, "],");
    }

    w(&mut out, 2, ")");
    out.push('\n');

    out
}

/// Format a V3 input: `io.Float.Input("name", default=0.4, min=0.01, max=0.99)`
fn format_v3_input(input: &ComfyInputDecl, optional: bool) -> String {
    let type_name = v3_type_name(&input.io_type);

    // Special: COMBO with folder-path
    if input.io_type == ComfyIOType::Combo {
        if let Some(ref fp) = input.options.folder_path {
            return format!("io.Combo.Input(\"{}\", folder=\"{fp}\"{})",
                input.name,
                if optional { ", optional=True" } else { "" });
        }
        if !input.options.options.is_empty() {
            let vals: Vec<String> = input.options.options.iter().map(|v| format!("\"{v}\"")).collect();
            return format!("io.Combo.Input(\"{}\", options=[{}]{})",
                input.name,
                vals.join(", "),
                if optional { ", optional=True" } else { "" });
        }
    }

    let mut parts = vec![format!("\"{}\"", input.name)];
    append_v3_opts(&mut parts, &input.options, &input.io_type);
    if optional {
        parts.push("optional=True".into());
    }

    format!("io.{type_name}.Input({})", parts.join(", "))
}

/// Format a V3 output: `io.Image.Output()` or `io.Image.Output("name")`
fn format_v3_output(io_type: &ComfyIOType, name: Option<&str>) -> String {
    let type_name = v3_type_name(io_type);
    match name {
        Some(n) => format!("io.{type_name}.Output({n})"),
        None => format!("io.{type_name}.Output()"),
    }
}

/// Map ComfyIOType to V3 Python class name (PascalCase).
fn v3_type_name(io_type: &ComfyIOType) -> String {
    let s = match io_type {
        ComfyIOType::String => "String",
        ComfyIOType::Image => "Image",
        ComfyIOType::Mask => "Mask",
        ComfyIOType::Latent => "Latent",
        ComfyIOType::Boolean => "Boolean",
        ComfyIOType::Int => "Int",
        ComfyIOType::Float => "Float",
        ComfyIOType::Combo => "Combo",
        ComfyIOType::Conditioning => "Conditioning",
        ComfyIOType::Sampler => "Sampler",
        ComfyIOType::Sigmas => "Sigmas",
        ComfyIOType::Guider => "Guider",
        ComfyIOType::Noise => "Noise",
        ComfyIOType::Clip => "Clip",
        ComfyIOType::ControlNet => "ControlNet",
        ComfyIOType::Vae => "Vae",
        ComfyIOType::Model => "Model",
        ComfyIOType::LoraModel => "LoraModel",
        ComfyIOType::ClipVision => "ClipVision",
        ComfyIOType::ClipVisionOutput => "ClipVisionOutput",
        ComfyIOType::StyleModel => "StyleModel",
        ComfyIOType::Gligen => "Gligen",
        ComfyIOType::UpscaleModel => "UpscaleModel",
        ComfyIOType::Audio => "Audio",
        ComfyIOType::Video => "Video",
        ComfyIOType::Webcam => "Webcam",
        ComfyIOType::Hooks => "Hooks",
        ComfyIOType::TimestepsRange => "TimestepsRange",
        ComfyIOType::HookKeyframes => "HookKeyframes",
        ComfyIOType::Floats => "Floats",
        ComfyIOType::ModelPatch => "ModelPatch",
        ComfyIOType::LatentUpscaleModel => "LatentUpscaleModel",
        ComfyIOType::Any => "Any",
        ComfyIOType::Custom(ref name) => return name.clone(),
    };
    s.into()
}

/// Append non-default options as keyword args.
fn append_v3_opts(parts: &mut Vec<String>, opts: &ComfyInputOptions, io_type: &ComfyIOType) {
    if let Some(ref d) = opts.default {
        let val = format_default(d, io_type);
        parts.push(format!("default={val}"));
    }
    if let Some(min) = opts.min {
        parts.push(format!("min={}", format_number(min)));
    }
    if let Some(max) = opts.max {
        parts.push(format!("max={}", format_number(max)));
    }
    if let Some(step) = opts.step {
        parts.push(format!("step={}", format_number(step)));
    }
    if let Some(round) = opts.round {
        parts.push(format!("round={}", format_number(round)));
    }
    if opts.multiline {
        parts.push("multiline=True".into());
    }
    if opts.dynamic_prompts {
        parts.push("dynamic_prompts=True".into());
    }
    if let Some(ref t) = opts.tooltip {
        parts.push(format!("tooltip=\"{t}\""));
    }
}

fn format_default(val: &str, io_type: &ComfyIOType) -> String {
    match io_type {
        ComfyIOType::String => format!("\"{val}\""),
        ComfyIOType::Boolean => if val == "true" { "True".into() } else { "False".into() },
        _ => val.to_string(),
    }
}

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

    #[test]
    fn generates_v3_schema() {
        let decl = ComfyNodeDecl {
            class_name: "Canny".into(),
            category: "image/preprocessors".into(),
            description: None,
            display_name: Some("Canny".into()),
            function: "execute".into(),
            output_node: false,
            deprecated: false, v3: false,
            search_aliases: vec!["edge detection".into()],
            required: vec![
                ComfyInputDecl {
                    name: "image".into(),
                    io_type: ComfyIOType::Image,
                    options: ComfyInputOptions::default(),
                },
                ComfyInputDecl {
                    name: "low_threshold".into(),
                    io_type: ComfyIOType::Float,
                    options: ComfyInputOptions {
                        default: Some("0.4".into()),
                        min: Some(0.01),
                        max: Some(0.99),
                        step: Some(0.01),
                        ..Default::default()
                    },
                },
            ],
            optional: vec![],
            hidden: vec![],
            return_types: vec![ComfyIOType::Image],
            return_names: vec![],
            output_tooltips: vec![],
        };

        let code = gen_schema_node(&decl);

        assert!(code.contains("class Canny_ForgeBase(io.ComfyNode):"));
        assert!(code.contains("def define_schema(cls):"));
        assert!(code.contains("return io.Schema("));
        assert!(code.contains("node_id=\"Canny\","));
        assert!(code.contains("display_name=\"Canny\","));
        assert!(code.contains("category=\"image/preprocessors\","));
        assert!(code.contains("search_aliases=[\"edge detection\"],"));
        assert!(code.contains("io.Image.Input(\"image\"),"));
        assert!(code.contains("io.Float.Input(\"low_threshold\", default=0.4, min=0.01, max=0.99, step=0.01),"));
        assert!(code.contains("io.Image.Output(),"));
    }

    #[test]
    fn indentation_correct() {
        let decl = ComfyNodeDecl {
            class_name: "Test".into(),
            category: "test".into(),
            description: None,
            display_name: None,
            function: "execute".into(),
            output_node: false,
            deprecated: false, v3: false,
            search_aliases: vec![],
            required: vec![ComfyInputDecl {
                name: "x".into(),
                io_type: ComfyIOType::Int,
                options: ComfyInputOptions::default(),
            }],
            optional: vec![],
            hidden: vec![],
            return_types: vec![ComfyIOType::Int],
            return_names: vec![],
            output_tooltips: vec![],
        };

        let code = gen_schema_node(&decl);
        for (i, line) in code.lines().enumerate() {
            if line.is_empty() { continue; }
            let spaces = line.len() - line.trim_start().len();
            assert_eq!(spaces % 4, 0, "line {}: indent {} not multiple of 4: {:?}", i + 1, spaces, line);
        }
    }
}
