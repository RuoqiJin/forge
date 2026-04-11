//! TypeScript atomic codegen functions (Layer 1).
//!
//! Each atom generates exactly ONE TypeScript construct.
//! Patterns (Layer 2) compose these atoms — they never duplicate logic.
//!
//! ## Atoms
//!
//! - `ts_interface` — TypeScript interface definitions
//! - `zod_schema` — Zod runtime validation schemas
//! - `import_stmt` — TypeScript import statements

pub mod import_stmt;
pub mod ts_interface;
pub mod zod_schema;
