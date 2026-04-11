//! Python-target IR types.
//!
//! - `comfy_node` — ComfyUI node definition (INPUT_TYPES, RETURN_TYPES, metadata)

pub mod comfy_node;

pub use comfy_node::{
    ComfyNodeDecl, ComfyInputDecl, ComfyInputOptions, ComfyHiddenDecl,
    ComfyIOType, parse_comfy_nodes,
};
