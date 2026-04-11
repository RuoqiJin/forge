//! # nc-jarvis-gen — Jarvis Code Generator
//!
//! ## Y-Shaped Pipeline
//!
//! ```text
//! Layer 3: .lisp files    — user intent declarations (parsed by nc-jarvis-ir)
//!                │
//!            ┌───┴───┐
//!            ▼       ▼
//!          rust/   ts/     ← target-specific generators
//!          │       │
//!     atoms+patterns  atoms+patterns
//! ```
//!
//! - `rust/` — Rust codegen (struct, enum, sqlx, MCP, state machine...)
//! - `ts/`   — TypeScript/React codegen (interface, Zod, JSX, hooks...)
//! - `common` — Shared utilities (indentation writer, forge header)
//!
//! Pure functions: IR → compilable source. Same input → same output.

// Shared utilities
pub mod common;

// Layer 0: Quarks — indivisible semantic primitives (cross-language)
pub mod quarks;

// Target-specific pipelines
pub mod rust;
pub mod ts;
pub mod python;

// Re-export Rust generators at crate root for backward compatibility
pub use rust::generate_admin_route_registry;
pub use rust::generate_bootstrap;
pub use rust::generate_connector;
pub use rust::generate_crud_gateway;
pub use rust::generate_db_store;
pub use rust::generate_crud_gateway_rusqlite;
pub use rust::generate_domain_engine;
pub use rust::generate_domain_types;
pub use rust::generate_error_enum;
pub use rust::generate_mcp_module;
pub use rust::generate_missiond_tool;
pub use rust::generate_pure_utility;
pub use rust::generate_rpc_gateway;
pub use rust::generate_s3_dispatcher;
pub use rust::generate_s3_dispatcher_custom;
pub use rust::generate_s3_response_builders;
pub use rust::generate_s3_response_custom;
pub use rust::generate_state_machine;
pub use rust::generate_worker;

// Re-export TS generators
pub use ts::generate_vue_component;
pub use ts::generate_pinia_store;
pub use ts::generate_canvas_registry;

// Re-export Python generators
pub use python::generate_comfy_node;
pub use python::generate_comfy_registry;
pub use python::generate_comfy_schema_node;
