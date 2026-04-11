//! # Atomic Codegen Functions (Layer 1)
//!
//! The smallest indivisible code generation units.
//! Each atom generates exactly ONE Rust construct.
//! Patterns (Layer 2) compose these atoms — they never duplicate logic.
//!
//! ## Design Principles (from neural-codegen GPU Mode)
//!
//! 1. **Selection, not creation** — atoms are lookup tables, not compilers
//! 2. **Each atom is independently testable** — golden test per atom
//! 3. **No cross-atom coupling** — atom A never calls atom B
//! 4. **Deterministic** — same input always produces same output
//! 5. **Language-agnostic IR** — atoms take typed structs, not S-expressions

pub mod struct_def;
pub mod enum_def;
pub mod arg_extract;
pub mod tool_schema;
pub mod query_method;
pub mod ddl_column;
pub mod row_to;
pub mod endpoint_route;
pub mod error_variant;
pub mod env_config;
pub mod di_field;
pub mod worker_loop;
pub mod w;
pub mod impl_block;
pub mod trait_contract;
pub mod match_arm;
pub mod sqlx_query;
pub mod pg_ddl;

// Re-export the shared write helper
pub use w::w;
