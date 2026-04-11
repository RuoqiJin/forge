//! Rust code generation pipeline.
//!
//! - `atoms/` — smallest indivisible codegen units (struct, enum, schema...)
//! - `patterns/` — compose atoms into higher-level generators (crud, mcp, worker...)

pub mod atoms;
pub mod patterns;

// Re-export pattern entry points
pub use patterns::admin_route_registry::generate_admin_route_registry;
pub use patterns::bootstrap::generate_bootstrap;
pub use patterns::connector::generate_connector;
pub use patterns::crud::generate_crud_gateway;
pub use patterns::db_store::generate_db_store;
pub use patterns::crud_rusqlite::generate_crud_gateway_rusqlite;
pub use patterns::domain_engine::generate_domain_engine;
pub use patterns::domain_types::generate_domain_types;
pub use patterns::error_enum::generate_error_enum;
pub use patterns::mcp_tool::generate_mcp_module;
pub use patterns::missiond_tool::generate_missiond_tool;
pub use patterns::pure_utility::generate_pure_utility;
pub use patterns::rpc_gateway::generate_rpc_gateway;
pub use patterns::s3_dispatcher::{generate_s3_dispatcher, generate_s3_dispatcher_custom};
pub use patterns::s3_response_builder::{generate_s3_response_builders, generate_s3_response_custom};
pub use patterns::state_machine::generate_state_machine;
pub use patterns::worker::generate_worker;
