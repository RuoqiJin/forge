//! Rust-target IR — validates Lisp S-expressions into Rust-specific typed IR.
//!
//! Contains Arc<T>, sqlx types, serde_json::Value, and other Rust-native
//! type references that are meaningless to a TypeScript pipeline.

pub mod admin_route_registry;
pub mod bootstrap;
pub mod connector;
pub mod crud;
pub mod db_store;
pub mod domain_engine;
pub mod domain_types;
pub mod error_enum;
pub mod mcp_tool;
pub mod pure_utility;
pub mod rpc_gateway;
pub mod s3_dispatcher;
pub mod s3_response_builder;
pub mod state_machine;
pub mod worker;

pub use admin_route_registry::AdminRouteRegistryDecl;
pub use bootstrap::BootstrapDecl;
pub use connector::ConnectorDecl;
pub use crud::CrudGatewayDecl;
pub use db_store::DbStoreDecl;
pub use domain_engine::DomainEngineDecl;
pub use domain_types::DomainTypesDecl;
pub use error_enum::ErrorEnumDecl;
pub use mcp_tool::McpModuleDecl;
pub use pure_utility::PureUtilityDecl;
pub use rpc_gateway::RpcGatewayDecl;
pub use s3_dispatcher::S3DispatcherDecl;
pub use s3_response_builder::S3ResponseBuilderDecl;
pub use state_machine::StateMachineDecl;
pub use worker::WorkerDecl;
