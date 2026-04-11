//! TypeScript-target IR — validates Lisp S-expressions into TS/Vue-specific typed IR.
//!
//! Handles: interface generation, Zod schemas, Vue SFC scaffolding,
//! Pinia store generation, canvas node registries, and external engine contracts.

pub mod canvas_node;
pub mod external_engine;
pub mod pinia_store;
pub mod vue_component;

pub use canvas_node::{CanvasNodeDecl, NodeSlot};
pub use external_engine::ExternalEngineDecl;
pub use pinia_store::{PiniaStoreDecl, StoreField, StoreGetter, StoreAction, ActionParam};
pub use vue_component::{VueComponentDecl, ComponentProp, ComponentEmit, ComponentSlot};
