//! Python atomic codegen functions (Layer 1).
//!
//! Each atom generates exactly ONE Python construct.
//! Patterns (Layer 2) compose these atoms — they never duplicate logic.

pub mod py_class_def;
pub mod py_input_types;
pub mod py_return_types;
pub mod py_schema_node;

// Re-export the shared write helper
pub use crate::common::w;
