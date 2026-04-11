//! Rust pattern generators (Layer 2).
//!
//! Each pattern composes atoms from `super::atoms` to produce complete Rust source files.

pub mod admin_route_registry;
pub mod bootstrap;
pub mod connector;
pub mod crud;
pub mod db_store;
pub mod crud_rusqlite;
pub mod domain_engine;
pub mod domain_types;
pub mod error_enum;
pub mod mcp_tool;
pub mod missiond_tool;
pub mod pure_utility;
pub mod rpc_gateway;
pub mod s3_dispatcher;
pub mod s3_response_builder;
pub mod state_machine;
pub mod worker;
