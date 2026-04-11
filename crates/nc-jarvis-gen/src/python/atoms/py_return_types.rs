//! Atom: py-return-types
//!
//! Generates ComfyUI `RETURN_TYPES`, `RETURN_NAMES`, and `OUTPUT_TOOLTIPS` tuples.
//! Uses ComfyIOType::to_io_expr() from the IR layer (NOT quarks).
//!
//! CRITICAL: Single-element tuples MUST have trailing comma: `(IO.IMAGE,)`

use crate::common::w;
use nc_jarvis_ir::python::comfy_node::ComfyIOType;

/// Generate RETURN_TYPES and optional RETURN_NAMES / OUTPUT_TOOLTIPS.
pub fn gen_return_types(
    return_types: &[ComfyIOType],
    return_names: &[String],
    output_tooltips: &[String],
) -> String {
    let mut out = String::with_capacity(256);

    if return_types.is_empty() {
        w(&mut out, 1, "RETURN_TYPES = ()");
    } else {
        let types: Vec<String> = return_types.iter().map(|t| t.to_io_expr()).collect();
        // Always use trailing comma for tuples (Python requirement for single-element)
        w(
            &mut out,
            1,
            &format!("RETURN_TYPES = ({},)", types.join(", ")),
        );
    }

    if !return_names.is_empty() {
        let names: Vec<String> = return_names.iter().map(|n| format!("\"{n}\"")).collect();
        w(
            &mut out,
            1,
            &format!("RETURN_NAMES = ({},)", names.join(", ")),
        );
    }

    if !output_tooltips.is_empty() {
        let tips: Vec<String> = output_tooltips.iter().map(|t| format!("\"{t}\"")).collect();
        w(
            &mut out,
            1,
            &format!("OUTPUT_TOOLTIPS = ({},)", tips.join(", ")),
        );
    }

    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_return_type_has_trailing_comma() {
        let code = gen_return_types(&[ComfyIOType::Conditioning], &[], &[]);
        assert!(code.contains("RETURN_TYPES = (IO.CONDITIONING,)"));
    }

    #[test]
    fn multiple_return_types() {
        let code = gen_return_types(
            &[ComfyIOType::Model, ComfyIOType::Clip, ComfyIOType::Vae],
            &[],
            &[],
        );
        assert!(code.contains("RETURN_TYPES = (IO.MODEL, IO.CLIP, IO.VAE,)"));
    }

    #[test]
    fn empty_return_types() {
        let code = gen_return_types(&[], &[], &[]);
        assert!(code.contains("RETURN_TYPES = ()"));
    }

    #[test]
    fn with_names_and_tooltips() {
        let code = gen_return_types(
            &[ComfyIOType::Image],
            &["output_image".into()],
            &["The processed image".into()],
        );
        assert!(code.contains("RETURN_TYPES = (IO.IMAGE,)"));
        assert!(code.contains("RETURN_NAMES = (\"output_image\",)"));
        assert!(code.contains("OUTPUT_TOOLTIPS = (\"The processed image\",)"));
    }

    #[test]
    fn indentation_correct() {
        let code = gen_return_types(&[ComfyIOType::Latent], &[], &[]);
        assert!(code.starts_with("    RETURN_TYPES"));
    }
}
