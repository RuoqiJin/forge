//! Python code generation pipeline.
//!
//! - `atoms/` — py_class_def, py_input_types, py_return_types
//! - `patterns/` — comfy_node, comfy_registry
//!
//! Pure functions: IR -> compilable Python source. Same input -> same output.

pub mod atoms;
pub mod patterns;

// Re-export pattern entry points
pub use patterns::generate_comfy_node;
pub use patterns::generate_comfy_registry;
pub use patterns::generate_comfy_schema_node;
