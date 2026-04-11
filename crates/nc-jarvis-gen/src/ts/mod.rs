//! TypeScript/Vue code generation pipeline.
//!
//! - `atoms/` — ts_interface, zod_schema, import_stmt
//! - `patterns/` — vue_component, pinia_store, canvas_registry
//!
//! Pure functions: IR -> compilable TypeScript source. Same input -> same output.

pub mod atoms;
pub mod patterns;

// Re-export pattern entry points
pub use patterns::generate_vue_component;
pub use patterns::generate_pinia_store;
pub use patterns::generate_canvas_registry;
