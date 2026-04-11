//! TypeScript pattern generators (Layer 2).
//!
//! Each pattern composes atoms from `super::atoms` to produce complete TS/Vue source files.

pub mod canvas_registry;
pub mod pinia_store;
pub mod vue_component;

pub use canvas_registry::generate_canvas_registry;
pub use pinia_store::generate_pinia_store;
pub use vue_component::generate_vue_component;
