//! ComfyUI Node code generator (Layer 2 Pattern).
//!
//! Composes atomic codegen functions to generate per-node `_ForgeBase` classes.
//! Uses inheritance isolation: generated base class has metadata,
//! human subclass has execute() logic.
//!
//! ## Output per node
//! ```python
//! class CLIPTextEncode_ForgeBase(ComfyNodeABC):
//!     CATEGORY = "conditioning"
//!     FUNCTION = "encode"
//!     @classmethod
//!     def INPUT_TYPES(cls) -> InputTypeDict: ...
//!     RETURN_TYPES = (IO.CONDITIONING,)
//! ```

use nc_jarvis_ir::python::comfy_node::*;

use crate::common::{forge_header, w};
use crate::python::atoms::py_class_def::{gen_py_class_def, PyClassDefInput};
use crate::python::atoms::py_input_types::gen_input_types;
use crate::python::atoms::py_return_types::gen_return_types;

/// Generate all `_ForgeBase` classes for a set of ComfyUI nodes.
///
/// Returns a single Python file containing all base classes.
pub fn generate_comfy_node(nodes: &[ComfyNodeDecl]) -> String {
    let mut out = String::with_capacity(nodes.len() * 2048);

    forge_header(&mut out, "python", ".missiond/intent-comfy.lisp");
    out.push('\n');

    // Imports
    gen_imports(&mut out, nodes);
    out.push('\n');

    // Generate each node's _ForgeBase class
    for (i, decl) in nodes.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        gen_single_node(&mut out, decl);
    }

    out
}

fn gen_imports(out: &mut String, nodes: &[ComfyNodeDecl]) {
    w(out, 0, "from comfy.comfy_types import IO, ComfyNodeABC, InputTypeDict");

    // Check if any node uses folder_paths
    let needs_folder_paths = nodes.iter().any(|n| {
        n.required
            .iter()
            .chain(n.optional.iter())
            .any(|i| i.options.folder_path.is_some())
    });
    if needs_folder_paths {
        w(out, 0, "import folder_paths");
    }
}

fn gen_single_node(out: &mut String, decl: &ComfyNodeDecl) {
    // Build class attributes
    let mut attrs = vec![];
    attrs.push(("CATEGORY".into(), format!("\"{}\"", decl.category)));
    attrs.push(("FUNCTION".into(), format!("\"{}\"", decl.function)));

    if let Some(ref desc) = decl.description {
        attrs.push(("DESCRIPTION".into(), format!("\"{}\"", desc)));
    }
    if decl.output_node {
        attrs.push(("OUTPUT_NODE".into(), "True".into()));
    }
    if decl.deprecated {
        attrs.push(("DEPRECATED".into(), "True".into()));
    }
    if !decl.search_aliases.is_empty() {
        let aliases: Vec<String> = decl.search_aliases.iter().map(|a| format!("\"{a}\"")).collect();
        attrs.push(("SEARCH_ALIASES".into(), format!("[{}]", aliases.join(", "))));
    }

    // Class definition via atom
    let class_code = gen_py_class_def(&PyClassDefInput {
        class_name: format!("{}_ForgeBase", decl.class_name),
        base_class: "ComfyNodeABC".into(),
        docstring: decl.description.clone(),
        attrs,
    });
    out.push_str(&class_code);
    out.push('\n');

    // INPUT_TYPES via atom
    let input_code = gen_input_types(&decl.required, &decl.optional, &decl.hidden);
    out.push_str(&input_code);
    out.push('\n');

    // RETURN_TYPES via atom
    let return_code = gen_return_types(&decl.return_types, &decl.return_names, &decl.output_tooltips);
    out.push_str(&return_code);
    out.push('\n');
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_clip_text_encode() -> ComfyNodeDecl {
        ComfyNodeDecl {
            class_name: "CLIPTextEncode".into(),
            category: "conditioning".into(),
            description: Some("Encodes text using CLIP".into()),
            display_name: Some("CLIP Text Encode (Prompt)".into()),
            function: "encode".into(),
            output_node: false,
            deprecated: false, v3: false,
            search_aliases: vec!["text".into(), "prompt".into()],
            required: vec![
                ComfyInputDecl {
                    name: "text".into(),
                    io_type: ComfyIOType::String,
                    options: ComfyInputOptions {
                        multiline: true,
                        dynamic_prompts: true,
                        tooltip: Some("The text to be encoded.".into()),
                        ..Default::default()
                    },
                },
                ComfyInputDecl {
                    name: "clip".into(),
                    io_type: ComfyIOType::Clip,
                    options: ComfyInputOptions::default(),
                },
            ],
            optional: vec![],
            hidden: vec![],
            return_types: vec![ComfyIOType::Conditioning],
            return_names: vec![],
            output_tooltips: vec![],
        }
    }

    #[test]
    fn generates_forge_base_class() {
        let code = generate_comfy_node(&[make_clip_text_encode()]);

        assert!(code.contains("GENERATED BY FORGE"));
        assert!(code.contains("from comfy.comfy_types import IO, ComfyNodeABC, InputTypeDict"));
        assert!(code.contains("class CLIPTextEncode_ForgeBase(ComfyNodeABC):"));
        assert!(code.contains("CATEGORY = \"conditioning\""));
        assert!(code.contains("FUNCTION = \"encode\""));
        assert!(code.contains("SEARCH_ALIASES = [\"text\", \"prompt\"]"));
        assert!(code.contains("@classmethod"));
        assert!(code.contains("\"text\": (IO.STRING,"));
        assert!(code.contains("\"multiline\": True"));
        assert!(code.contains("\"clip\": (IO.CLIP,),"));
        assert!(code.contains("RETURN_TYPES = (IO.CONDITIONING,)"));
    }

    #[test]
    fn no_execute_stub() {
        let code = generate_comfy_node(&[make_clip_text_encode()]);
        // Inheritance isolation: _ForgeBase has NO execute method
        assert!(!code.contains("def encode("));
        assert!(!code.contains("NotImplementedError"));
    }

    #[test]
    fn folder_paths_imported_when_needed() {
        let node = ComfyNodeDecl {
            class_name: "CheckpointLoaderSimple".into(),
            category: "loaders".into(),
            description: None,
            display_name: None,
            function: "load_checkpoint".into(),
            output_node: false,
            deprecated: false, v3: false,
            search_aliases: vec![],
            required: vec![ComfyInputDecl {
                name: "ckpt_name".into(),
                io_type: ComfyIOType::Combo,
                options: ComfyInputOptions {
                    folder_path: Some("checkpoints".into()),
                    ..Default::default()
                },
            }],
            optional: vec![],
            hidden: vec![],
            return_types: vec![ComfyIOType::Model, ComfyIOType::Clip, ComfyIOType::Vae],
            return_names: vec![],
            output_tooltips: vec![],
        };

        let code = generate_comfy_node(&[node]);
        assert!(code.contains("import folder_paths"));
        assert!(code.contains("folder_paths.get_filename_list(\"checkpoints\")"));
        assert!(code.contains("RETURN_TYPES = (IO.MODEL, IO.CLIP, IO.VAE,)"));
    }

    #[test]
    fn brace_and_indent_balance() {
        let code = generate_comfy_node(&[make_clip_text_encode()]);

        // Every open brace has a close brace
        let open = code.matches('{').count();
        let close = code.matches('}').count();
        assert_eq!(open, close, "unbalanced braces: {open} open vs {close} close");

        // Every line that starts with spaces has a multiple-of-4 indent
        for (i, line) in code.lines().enumerate() {
            if line.is_empty() {
                continue;
            }
            let spaces = line.len() - line.trim_start().len();
            assert_eq!(
                spaces % 4,
                0,
                "line {}: indent {} not multiple of 4: {:?}",
                i + 1,
                spaces,
                line
            );
        }
    }
}
