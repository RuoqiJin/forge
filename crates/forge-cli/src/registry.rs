//! Tag-based pattern registry for Forge CLI.
//!
//! Each Lisp top-level form has a tag (first atom in list).
//! The registry maps tag → PatternProcessor, eliminating hardcoded filename matching.

use nc_parser::SexpNode;
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

/// A generated file ready to be written.
pub struct GeneratedFile {
    /// Relative path for output (resolved against project_root).
    pub path: String,
    /// Generated source code.
    pub content: String,
    /// true = always overwrite (generated), false = write-once (custom scaffold).
    pub overwrite: bool,
}

/// Context passed to every pattern processor.
pub struct ForgeContext {
    pub project_root: PathBuf,
    pub dry_run: bool,
    pub output_dir: Option<PathBuf>,
}

/// Trait that every pattern processor implements.
pub trait PatternProcessor {
    /// The S-EXP tag this processor handles (e.g. "crud-gateway", "worker").
    fn tag(&self) -> &str;

    /// Process a single top-level S-EXP node, returning generated files.
    fn process(&self, node: &SexpNode, ctx: &ForgeContext) -> Result<Vec<GeneratedFile>, String>;
}

/// Trait for patterns that need ALL matching nodes at once (e.g. comfy-node).
pub trait BatchPatternProcessor {
    fn tag(&self) -> &str;
    fn process_batch(
        &self,
        nodes: &[&SexpNode],
        ctx: &ForgeContext,
    ) -> Result<Vec<GeneratedFile>, String>;
}

/// Central registry mapping tags to processors.
pub struct PatternRegistry {
    processors: Vec<Box<dyn PatternProcessor>>,
    batch_processors: Vec<Box<dyn BatchPatternProcessor>>,
}

impl PatternRegistry {
    pub fn new() -> Self {
        Self {
            processors: vec![],
            batch_processors: vec![],
        }
    }

    pub fn register(&mut self, processor: Box<dyn PatternProcessor>) {
        self.processors.push(processor);
    }

    pub fn register_batch(&mut self, processor: Box<dyn BatchPatternProcessor>) {
        self.batch_processors.push(processor);
    }

    pub fn get(&self, tag: &str) -> Option<&dyn PatternProcessor> {
        // For simple tags, direct match
        self.processors
            .iter()
            .find(|p| p.tag() == tag)
            .map(|p| p.as_ref())
    }

    /// Find processor for a `(pattern <sub-tag> ...)` or direct `(crud-gateway ...)` node.
    pub fn get_for_node(&self, node: &SexpNode) -> Option<&dyn PatternProcessor> {
        let tag = node.tag()?;
        // Direct crud-gateway tag: (crud-gateway name (table ...) ...)
        if tag == "crud-gateway" {
            return self.processors.iter()
                .find(|p| p.tag() == "pattern")
                .map(|p| p.as_ref());
        }
        // For the "pattern" meta-tag, try sub-tag matching
        if tag == "pattern" {
            let st = sub_tag(node)?;
            for p in &self.processors {
                if p.tag() == "pattern" {
                    if st == "crud-gateway" {
                        return Some(p.as_ref());
                    }
                }
            }
            None
        } else {
            self.get(tag)
        }
    }

    pub fn get_batch(&self, tag: &str) -> Option<&dyn BatchPatternProcessor> {
        self.batch_processors
            .iter()
            .find(|p| p.tag() == tag)
            .map(|p| p.as_ref())
    }

    /// Returns true if this tag is handled by a batch processor.
    pub fn is_batch_tag(&self, tag: &str) -> bool {
        self.batch_processors.iter().any(|p| p.tag() == tag)
    }

    #[allow(dead_code)]
    pub fn tags(&self) -> Vec<&str> {
        let mut tags: Vec<&str> = self.processors.iter().map(|p| p.tag()).collect();
        tags.extend(self.batch_processors.iter().map(|p| p.tag()));
        tags
    }
}

// ---------------------------------------------------------------------------
// Helper: resolve output path from :target field
// ---------------------------------------------------------------------------

/// Extract the second atom from a list (sub-tag for `(pattern crud-gateway ...)` forms).
fn sub_tag(node: &SexpNode) -> Option<&str> {
    match node {
        SexpNode::List(children) if children.len() >= 2 => match &children[1] {
            SexpNode::Atom(s) => Some(s),
            _ => None,
        },
        _ => None,
    }
}

fn resolve_target(target: &str, ctx: &ForgeContext) -> String {
    if let Some(ref out_dir) = ctx.output_dir {
        out_dir.join(target).to_string_lossy().into_owned()
    } else {
        ctx.project_root
            .join(target)
            .to_string_lossy()
            .into_owned()
    }
}

fn resolve_target_opt(target: Option<&str>, tag: &str, name: &str, ctx: &ForgeContext) -> String {
    if let Some(t) = target {
        resolve_target(t, ctx)
    } else if let Some(ref out_dir) = ctx.output_dir {
        out_dir
            .join(tag)
            .join(format!("{name}.rs"))
            .to_string_lossy()
            .into_owned()
    } else {
        ctx.project_root
            .join(".forge-output")
            .join(tag)
            .join(format!("{name}.rs"))
            .to_string_lossy()
            .into_owned()
    }
}

// ---------------------------------------------------------------------------
// Pattern: (pattern crud-gateway ...)
//
// The `crud-gateway` pattern uses `(pattern crud-gateway ...)` wrapping,
// so the top-level tag is "pattern" with "crud-gateway" as the second atom.
// We register under "pattern" and verify the sub-tag internally.
// ---------------------------------------------------------------------------

struct CrudGatewayProcessor;

impl PatternProcessor for CrudGatewayProcessor {
    fn tag(&self) -> &str {
        "pattern"
    }

    fn process(&self, node: &SexpNode, ctx: &ForgeContext) -> Result<Vec<GeneratedFile>, String> {
        let decl = nc_jarvis_ir::crud::CrudGatewayDecl::from_sexp(node)
            .map_err(|e| format!("IR error: {e}"))?;

        let output = if decl.backend == "sqlite" {
            nc_jarvis_gen::generate_crud_gateway_rusqlite(&decl)
        } else {
            nc_jarvis_gen::generate_crud_gateway(&decl)
        };

        eprintln!(
            "  crud-gateway: {} tables, {} ops, backend: {} -> {} lines",
            decl.tables.len(),
            decl.tables.iter().map(|t| t.ops.len()).sum::<usize>(),
            decl.backend,
            output.lines().count()
        );

        let path = resolve_target(&decl.target, ctx);
        Ok(vec![GeneratedFile {
            path,
            content: output,
            overwrite: true,
        }])
    }
}

// ---------------------------------------------------------------------------
// Pattern: worker
// ---------------------------------------------------------------------------

struct WorkerProcessor;

impl PatternProcessor for WorkerProcessor {
    fn tag(&self) -> &str {
        "worker"
    }

    fn process(&self, node: &SexpNode, ctx: &ForgeContext) -> Result<Vec<GeneratedFile>, String> {
        // worker IR parses a single node
        let decl = nc_jarvis_ir::worker::WorkerDecl::from_sexp(node)
            .map_err(|e| format!("IR error: {e}"))?;

        let output = nc_jarvis_gen::generate_worker(&decl);
        eprintln!(
            "  worker: {} ({:?}) -> {} lines",
            decl.name,
            decl.pattern,
            output.lines().count()
        );

        let path =
            resolve_target_opt(decl.target.as_deref(), "worker", &decl.name, ctx);
        Ok(vec![GeneratedFile {
            path,
            content: output,
            overwrite: true,
        }])
    }
}

// ---------------------------------------------------------------------------
// Pattern: mcp-module
// ---------------------------------------------------------------------------

struct McpModuleProcessor;

impl PatternProcessor for McpModuleProcessor {
    fn tag(&self) -> &str {
        "mcp-module"
    }

    fn process(&self, node: &SexpNode, ctx: &ForgeContext) -> Result<Vec<GeneratedFile>, String> {
        let decl = nc_jarvis_ir::mcp_tool::McpModuleDecl::from_sexp(node)
            .map_err(|e| format!("IR error: {e}"))?;

        let output = nc_jarvis_gen::generate_mcp_module(&decl);
        eprintln!(
            "  mcp-module: {} ({} tools) -> {} lines",
            decl.name,
            decl.tools.len(),
            output.lines().count()
        );

        let path =
            resolve_target_opt(decl.target.as_deref(), "mcp-module", &decl.name, ctx);
        Ok(vec![GeneratedFile {
            path,
            content: output,
            overwrite: true,
        }])
    }
}

// ---------------------------------------------------------------------------
// Pattern: mcp-module (MissionD tool defs variant)
//
// Uses the same tag "mcp-module" but generates MissionD tool definitions.
// This is handled as a separate processor registered under a virtual tag
// "missiond-tool-defs" that the CLI maps from the old "intent-mcp-defs.lisp"
// convention. However, since we dispatch on tag, and the lisp also uses
// (mcp-module ...), we need the caller to decide. Instead, we provide a
// separate processor that the CLI can invoke explicitly for mcp-defs files.
//
// Actually — re-reading the original code, intent-mcp-defs.lisp also contains
// (mcp-module ...) forms. The difference is which generator is called:
// generate_mcp_module vs generate_missiond_tool. Both produce different output.
//
// Since we dispatch on tag, and the tag is the same, we need a way to
// differentiate. The cleanest approach: register "missiond-tool-defs" as a
// virtual tag, and have the CLI file-level router remap when it sees
// intent-mcp-defs.lisp. BUT that defeats the purpose of tag-based dispatch.
//
// Better approach: we DON'T register this as a separate tag. Instead, the
// mcp-module processor generates BOTH files (mcp handler + missiond defs).
// The user can control which output they want via :target in the Lisp.
//
// For now, to preserve backward compatibility, we keep both processors
// and let the BuildRunner apply the missiond-tool-defs processor on nodes
// whose tag is "mcp-module" when the file is named intent-mcp-defs.lisp.
// This is a transitional measure.
// ---------------------------------------------------------------------------

pub struct MissiondToolDefsProcessor;

impl PatternProcessor for MissiondToolDefsProcessor {
    fn tag(&self) -> &str {
        "missiond-tool-defs"
    }

    fn process(&self, node: &SexpNode, ctx: &ForgeContext) -> Result<Vec<GeneratedFile>, String> {
        // Accepts (mcp-module ...) nodes but generates MissionD tool definitions
        let decl = nc_jarvis_ir::mcp_tool::McpModuleDecl::from_sexp(node)
            .map_err(|e| format!("IR error: {e}"))?;

        let output = nc_jarvis_gen::generate_missiond_tool(&decl);
        eprintln!(
            "  missiond-tool-defs: {} ({} tools) -> {} lines",
            decl.name,
            decl.tools.len(),
            output.lines().count()
        );

        let path = resolve_target_opt(
            decl.target.as_deref(),
            "missiond-tool-defs",
            &decl.name,
            ctx,
        );
        Ok(vec![GeneratedFile {
            path,
            content: output,
            overwrite: true,
        }])
    }
}

// ---------------------------------------------------------------------------
// Pattern: state-machine
// ---------------------------------------------------------------------------

struct StateMachineProcessor;

impl PatternProcessor for StateMachineProcessor {
    fn tag(&self) -> &str {
        "state-machine"
    }

    fn process(&self, node: &SexpNode, ctx: &ForgeContext) -> Result<Vec<GeneratedFile>, String> {
        let decl = nc_jarvis_ir::state_machine::StateMachineDecl::from_sexp(node)
            .map_err(|e| format!("IR error: {e}"))?;

        let output = nc_jarvis_gen::generate_state_machine(&decl);
        eprintln!(
            "  state-machine: {} states, {} transitions -> {} lines",
            decl.states.len(),
            decl.transitions.len(),
            output.lines().count()
        );

        if decl.target.is_empty() {
            return Ok(vec![]);
        }

        let path = resolve_target(&decl.target, ctx);
        Ok(vec![GeneratedFile {
            path,
            content: output,
            overwrite: true,
        }])
    }
}

// ---------------------------------------------------------------------------
// Pattern: bootstrap
// ---------------------------------------------------------------------------

struct BootstrapProcessor;

impl PatternProcessor for BootstrapProcessor {
    fn tag(&self) -> &str {
        "bootstrap"
    }

    fn process(&self, node: &SexpNode, ctx: &ForgeContext) -> Result<Vec<GeneratedFile>, String> {
        let decl = nc_jarvis_ir::bootstrap::BootstrapDecl::from_sexp(node)
            .map_err(|e| format!("IR error: {e}"))?;

        let output = nc_jarvis_gen::generate_bootstrap(&decl);
        eprintln!(
            "  bootstrap: {} infra, {} modules, {} workers -> {} lines",
            decl.infra.len(),
            decl.modules.len(),
            decl.workers.len(),
            output.lines().count()
        );

        if decl.target.is_empty() {
            return Ok(vec![]);
        }

        let path = resolve_target(&decl.target, ctx);
        Ok(vec![GeneratedFile {
            path,
            content: output,
            overwrite: true,
        }])
    }
}

// ---------------------------------------------------------------------------
// Pattern: domain-types
// ---------------------------------------------------------------------------

struct DomainTypesProcessor;

impl PatternProcessor for DomainTypesProcessor {
    fn tag(&self) -> &str {
        "domain-types"
    }

    fn process(&self, node: &SexpNode, ctx: &ForgeContext) -> Result<Vec<GeneratedFile>, String> {
        let decl = nc_jarvis_ir::rust::domain_types::DomainTypesDecl::from_sexp(node)
            .map_err(|e| format!("IR error: {e}"))?;

        let output = nc_jarvis_gen::generate_domain_types(&decl);
        eprintln!(
            "  domain-types: {} structs, {} enums -> {} lines",
            decl.structs.len(),
            decl.enums.len(),
            output.lines().count()
        );

        let path = resolve_target(&decl.target, ctx);
        Ok(vec![GeneratedFile {
            path,
            content: output,
            overwrite: true,
        }])
    }
}

// ---------------------------------------------------------------------------
// Pattern: comfy-node (Python, batch processor)
//
// ComfyUI nodes are batched because the generator splits legacy vs v3,
// and produces a combined registry file across all nodes.
// ---------------------------------------------------------------------------

struct ComfyNodeBatchProcessor;

impl BatchPatternProcessor for ComfyNodeBatchProcessor {
    fn tag(&self) -> &str {
        "comfy-node"
    }

    fn process_batch(
        &self,
        nodes: &[&SexpNode],
        ctx: &ForgeContext,
    ) -> Result<Vec<GeneratedFile>, String> {
        let decls: Vec<nc_jarvis_ir::python::ComfyNodeDecl> = nodes
            .iter()
            .map(|n| {
                nc_jarvis_ir::python::ComfyNodeDecl::from_sexp(n)
                    .map_err(|e| format!("ComfyUI IR error: {e}"))
            })
            .collect::<Result<Vec<_>, _>>()?;

        if decls.is_empty() {
            return Ok(vec![]);
        }

        let legacy: Vec<_> = decls.iter().filter(|d| !d.v3).cloned().collect();
        let v3: Vec<_> = decls.iter().filter(|d| d.v3).cloned().collect();

        let base_dir = if let Some(ref out_dir) = ctx.output_dir {
            out_dir.join("forge_output")
        } else {
            ctx.project_root.join("forge_output")
        };

        let mut files = Vec::new();

        if !legacy.is_empty() {
            let code = nc_jarvis_gen::generate_comfy_node(&legacy);
            files.push(GeneratedFile {
                path: base_dir
                    .join("nodes_forge_base.py")
                    .to_string_lossy()
                    .into_owned(),
                content: code,
                overwrite: true,
            });
        }

        if !v3.is_empty() {
            let code = nc_jarvis_gen::generate_comfy_schema_node(&v3);
            files.push(GeneratedFile {
                path: base_dir
                    .join("nodes_forge_base_v3.py")
                    .to_string_lossy()
                    .into_owned(),
                content: code,
                overwrite: true,
            });
        }

        let registry_code = nc_jarvis_gen::generate_comfy_registry(&decls, "nodes");
        files.push(GeneratedFile {
            path: base_dir
                .join("forge_registry.py")
                .to_string_lossy()
                .into_owned(),
            content: registry_code,
            overwrite: true,
        });

        eprintln!(
            "  comfy-node: {} nodes ({} legacy + {} v3) -> {} files",
            decls.len(),
            legacy.len(),
            v3.len(),
            files.len()
        );

        Ok(files)
    }
}

// ---------------------------------------------------------------------------
// Pattern: s3-dispatcher
// ---------------------------------------------------------------------------

struct S3DispatcherProcessor;

impl PatternProcessor for S3DispatcherProcessor {
    fn tag(&self) -> &str {
        "s3-dispatcher"
    }

    fn process(&self, node: &SexpNode, ctx: &ForgeContext) -> Result<Vec<GeneratedFile>, String> {
        let decl = nc_jarvis_ir::s3_dispatcher::S3DispatcherDecl::from_sexp(node)
            .map_err(|e| format!("IR error: {e}"))?;

        let generated = nc_jarvis_gen::generate_s3_dispatcher(&decl);
        let custom = nc_jarvis_gen::generate_s3_dispatcher_custom(&decl);

        eprintln!(
            "  s3-dispatcher: {} ({} delegates, {} customs) -> {} + {} lines",
            decl.name,
            decl.delegates.len(),
            decl.customs.len(),
            generated.lines().count(),
            custom.lines().count()
        );

        let base = resolve_target_opt(None, "s3-dispatcher", &decl.name, ctx);
        let gen_path = base.replace(".rs", "_generated.rs");
        let custom_path = base.replace(".rs", "_custom.rs");

        Ok(vec![
            GeneratedFile {
                path: gen_path,
                content: generated,
                overwrite: true,
            },
            GeneratedFile {
                path: custom_path,
                content: custom,
                overwrite: false,
            },
        ])
    }
}

// ---------------------------------------------------------------------------
// Pattern: admin-route-registry
// ---------------------------------------------------------------------------

struct AdminRouteRegistryProcessor;

impl PatternProcessor for AdminRouteRegistryProcessor {
    fn tag(&self) -> &str {
        "admin-route-registry"
    }

    fn process(&self, node: &SexpNode, ctx: &ForgeContext) -> Result<Vec<GeneratedFile>, String> {
        let decl = nc_jarvis_ir::admin_route_registry::AdminRouteRegistryDecl::from_sexp(node)
            .map_err(|e| format!("IR error: {e}"))?;

        let output = nc_jarvis_gen::generate_admin_route_registry(&decl);
        eprintln!(
            "  admin-route-registry: {} ({} groups, {} routes) -> {} lines",
            decl.name,
            decl.groups.len(),
            decl.total_routes(),
            output.lines().count()
        );

        let path = resolve_target_opt(None, "admin-route-registry", &decl.name, ctx);
        Ok(vec![GeneratedFile {
            path,
            content: output,
            overwrite: true,
        }])
    }
}

// ---------------------------------------------------------------------------
// Pattern: s3-response-builder
// ---------------------------------------------------------------------------

struct S3ResponseBuilderProcessor;

impl PatternProcessor for S3ResponseBuilderProcessor {
    fn tag(&self) -> &str {
        "s3-response-builder"
    }

    fn process(&self, node: &SexpNode, ctx: &ForgeContext) -> Result<Vec<GeneratedFile>, String> {
        let decl = nc_jarvis_ir::s3_response_builder::S3ResponseBuilderDecl::from_sexp(node)
            .map_err(|e| format!("IR error: {e}"))?;

        let generated = nc_jarvis_gen::generate_s3_response_builders(&decl);
        let custom = nc_jarvis_gen::generate_s3_response_custom(&decl);

        eprintln!(
            "  s3-response-builder: {} ({} builders) -> {} + {} lines",
            decl.name,
            decl.builders.len(),
            generated.lines().count(),
            custom.lines().count()
        );

        let base = resolve_target_opt(decl.target.as_deref(), "s3-response-builder", &decl.name, ctx);
        let gen_path = if decl.target.is_some() {
            base.clone()
        } else {
            base.replace(".rs", "_generated.rs")
        };
        let custom_path = if decl.target.is_some() {
            base.replace(".rs", "_custom.rs")
        } else {
            base.replace(".rs", "_custom.rs")
        };

        Ok(vec![
            GeneratedFile {
                path: gen_path,
                content: generated,
                overwrite: true,
            },
            GeneratedFile {
                path: custom_path,
                content: custom,
                overwrite: false,
            },
        ])
    }
}

// ---------------------------------------------------------------------------
// Pattern: error-enum
// ---------------------------------------------------------------------------

struct ErrorEnumProcessor;

impl PatternProcessor for ErrorEnumProcessor {
    fn tag(&self) -> &str {
        "error-enum"
    }

    fn process(&self, node: &SexpNode, ctx: &ForgeContext) -> Result<Vec<GeneratedFile>, String> {
        let decl = nc_jarvis_ir::error_enum::ErrorEnumDecl::from_sexp(node)
            .map_err(|e| format!("IR error: {e}"))?;

        let output = nc_jarvis_gen::generate_error_enum(&decl);
        eprintln!(
            "  error-enum: {} ({} variants) -> {} lines",
            decl.name,
            decl.all_variants().len(),
            output.lines().count()
        );

        let path =
            resolve_target_opt(decl.target.as_deref(), "error-enum", &decl.name, ctx);
        Ok(vec![GeneratedFile {
            path,
            content: output,
            overwrite: true,
        }])
    }
}

// ---------------------------------------------------------------------------
// Pattern: pure-utility
// ---------------------------------------------------------------------------

struct PureUtilityProcessor;

impl PatternProcessor for PureUtilityProcessor {
    fn tag(&self) -> &str {
        "pure-utility"
    }

    fn process(&self, node: &SexpNode, ctx: &ForgeContext) -> Result<Vec<GeneratedFile>, String> {
        let decl = nc_jarvis_ir::pure_utility::PureUtilityDecl::from_sexp(node)
            .map_err(|e| format!("IR error: {e}"))?;

        let output = nc_jarvis_gen::generate_pure_utility(&decl);
        eprintln!(
            "  pure-utility: {} -> {} lines",
            decl.name,
            output.lines().count()
        );

        let path =
            resolve_target_opt(decl.target.as_deref(), "pure-utility", &decl.name, ctx);
        Ok(vec![GeneratedFile {
            path,
            content: output,
            overwrite: true,
        }])
    }
}

// ---------------------------------------------------------------------------
// Pattern: rpc-gateway
// ---------------------------------------------------------------------------

struct RpcGatewayProcessor;

impl PatternProcessor for RpcGatewayProcessor {
    fn tag(&self) -> &str {
        "rpc-gateway"
    }

    fn process(&self, node: &SexpNode, ctx: &ForgeContext) -> Result<Vec<GeneratedFile>, String> {
        let decl = nc_jarvis_ir::rpc_gateway::RpcGatewayDecl::from_sexp(node)
            .map_err(|e| format!("IR error: {e}"))?;

        let output = nc_jarvis_gen::generate_rpc_gateway(&decl);
        eprintln!(
            "  rpc-gateway: {} -> {} lines",
            decl.name,
            output.lines().count()
        );

        let path =
            resolve_target_opt(decl.target.as_deref(), "rpc-gateway", &decl.name, ctx);
        Ok(vec![GeneratedFile {
            path,
            content: output,
            overwrite: true,
        }])
    }
}

// ---------------------------------------------------------------------------
// Pattern: connector
// ---------------------------------------------------------------------------

struct ConnectorProcessor;

impl PatternProcessor for ConnectorProcessor {
    fn tag(&self) -> &str {
        "connector"
    }

    fn process(&self, node: &SexpNode, ctx: &ForgeContext) -> Result<Vec<GeneratedFile>, String> {
        let decl = nc_jarvis_ir::connector::ConnectorDecl::from_sexp(node)
            .map_err(|e| format!("IR error: {e}"))?;

        let output = nc_jarvis_gen::generate_connector(&decl);
        eprintln!(
            "  connector: {} (auth: {:?}) -> {} lines",
            decl.name,
            decl.auth,
            output.lines().count()
        );

        let path = resolve_target(
            &format!("src/connectors/gen_{}_connector.rs", decl.name.replace('-', "_")),
            ctx,
        );
        Ok(vec![GeneratedFile {
            path,
            content: output,
            overwrite: true,
        }])
    }
}

// ---------------------------------------------------------------------------
// Pattern: db-store
// ---------------------------------------------------------------------------

struct DbStoreProcessor;

impl PatternProcessor for DbStoreProcessor {
    fn tag(&self) -> &str {
        "db-store"
    }

    fn process(&self, node: &SexpNode, ctx: &ForgeContext) -> Result<Vec<GeneratedFile>, String> {
        let decl = nc_jarvis_ir::db_store::DbStoreDecl::from_sexp(node)
            .map_err(|e| format!("IR error: {e}"))?;

        let output = nc_jarvis_gen::generate_db_store(&decl);
        eprintln!(
            "  db-store: {} ({} queries, table: {}) -> {} lines",
            decl.name,
            decl.queries.len(),
            decl.table,
            output.lines().count()
        );

        let path = resolve_target(
            &format!("src/db/gen_{}_store.rs", decl.name.replace('-', "_")),
            ctx,
        );
        Ok(vec![GeneratedFile {
            path,
            content: output,
            overwrite: true,
        }])
    }
}

// ---------------------------------------------------------------------------
// Pattern: domain-engine
// ---------------------------------------------------------------------------

struct DomainEngineProcessor;

impl PatternProcessor for DomainEngineProcessor {
    fn tag(&self) -> &str {
        "domain-engine"
    }

    fn process(&self, node: &SexpNode, ctx: &ForgeContext) -> Result<Vec<GeneratedFile>, String> {
        let decl = nc_jarvis_ir::domain_engine::DomainEngineDecl::from_sexp(node)
            .map_err(|e| format!("IR error: {e}"))?;

        let output = nc_jarvis_gen::generate_domain_engine(&decl);
        eprintln!(
            "  domain-engine: {} -> {} lines",
            decl.name,
            output.lines().count()
        );

        let path =
            resolve_target_opt(decl.target.as_deref(), "domain-engine", &decl.name, ctx);
        Ok(vec![GeneratedFile {
            path,
            content: output,
            overwrite: true,
        }])
    }
}

// ---------------------------------------------------------------------------
// Pattern: vue-component (TypeScript)
// ---------------------------------------------------------------------------

struct VueComponentProcessor;

impl PatternProcessor for VueComponentProcessor {
    fn tag(&self) -> &str {
        "vue-component"
    }

    fn process(&self, node: &SexpNode, ctx: &ForgeContext) -> Result<Vec<GeneratedFile>, String> {
        let decl = nc_jarvis_ir::ts::VueComponentDecl::from_sexp(node)
            .map_err(|e| format!("IR error: {e}"))?;

        let output = nc_jarvis_gen::generate_vue_component(&decl);
        eprintln!(
            "  vue-component: {} -> {} lines",
            decl.name,
            output.lines().count()
        );

        let path = resolve_target_opt(
            decl.target_path.as_deref(),
            "vue-component",
            &decl.name,
            ctx,
        );
        Ok(vec![GeneratedFile {
            path,
            content: output,
            overwrite: true,
        }])
    }
}

// ---------------------------------------------------------------------------
// Pattern: pinia-store (TypeScript)
// ---------------------------------------------------------------------------

struct PiniaStoreProcessor;

impl PatternProcessor for PiniaStoreProcessor {
    fn tag(&self) -> &str {
        "pinia-store"
    }

    fn process(&self, node: &SexpNode, ctx: &ForgeContext) -> Result<Vec<GeneratedFile>, String> {
        let decl = nc_jarvis_ir::ts::PiniaStoreDecl::from_sexp(node)
            .map_err(|e| format!("IR error: {e}"))?;

        let output = nc_jarvis_gen::generate_pinia_store(&decl);
        eprintln!(
            "  pinia-store: {} -> {} lines",
            decl.name,
            output.lines().count()
        );

        let path = resolve_target_opt(
            decl.target_path.as_deref(),
            "pinia-store",
            &decl.name,
            ctx,
        );
        Ok(vec![GeneratedFile {
            path,
            content: output,
            overwrite: true,
        }])
    }
}

// ---------------------------------------------------------------------------
// Registry builder
// ---------------------------------------------------------------------------

/// Build the full pattern registry with all known processors.
pub fn build_registry() -> PatternRegistry {
    let mut reg = PatternRegistry::new();

    // Per-node processors (one S-EXP form → generated files)
    reg.register(Box::new(ConnectorProcessor));
    reg.register(Box::new(CrudGatewayProcessor));
    reg.register(Box::new(DbStoreProcessor));
    reg.register(Box::new(WorkerProcessor));
    reg.register(Box::new(McpModuleProcessor));
    reg.register(Box::new(StateMachineProcessor));
    reg.register(Box::new(BootstrapProcessor));
    reg.register(Box::new(DomainTypesProcessor));
    reg.register(Box::new(S3DispatcherProcessor));
    reg.register(Box::new(AdminRouteRegistryProcessor));
    reg.register(Box::new(S3ResponseBuilderProcessor));
    reg.register(Box::new(ErrorEnumProcessor));
    reg.register(Box::new(PureUtilityProcessor));
    reg.register(Box::new(RpcGatewayProcessor));
    reg.register(Box::new(DomainEngineProcessor));
    reg.register(Box::new(PiniaStoreProcessor));
    reg.register(Box::new(VueComponentProcessor));

    // Batch processors (all matching nodes collected, then processed together)
    reg.register_batch(Box::new(ComfyNodeBatchProcessor));

    // MissiondToolDefsProcessor is NOT in the main registry —
    // it's used explicitly for backward-compat file-level overrides.
    reg
}
