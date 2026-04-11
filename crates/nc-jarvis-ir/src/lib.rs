//! # nc-jarvis-ir — Jarvis Domain-Specific IR
//!
//! Typed intermediate representation for Jarvis architectural patterns.
//! Each pattern constrains LLM output to a finite, verified set of operations.
//!
//! ## Y-Shaped Pipeline
//! ```text
//! Lisp declarations (Layer 3)
//!        │
//!    ┌───┴───┐
//!    ▼       ▼
//!  rust/   ts/        ← target-specific IR (Layer 2)
//! ```
//!
//! - `rust/` — Rust-target IR (Arc<T>, sqlx, serde_json::Value)
//! - `ts/`   — TypeScript-target IR (interfaces, Zod, React Hooks)
//! - `common` — Shared error types and S-EXP extraction helpers

pub mod common;
pub mod rust;
pub mod ts;
pub mod python;

// Re-export Rust IR types at crate root for backward compatibility
pub use rust::*;
