(intent jarvis-forge
  (generated-at 2026-04-07T00:00:00Z)
  (version "3.2")

  (design-constraints
    (y-pipeline "IR and codegen split into rust/ + ts/ + python/ branches; Lisp declarations unified at top")
    (generation-gap "Every generated file has a frozen generated.rs/tsx + hand-written custom.rs/tsx")
    (black-box-engine "Complex interaction engines (canvas, 3D, physics) declared as external-engine, Forge generates contracts only")
    (deterministic "Same Lisp input always produces identical output; no side effects in codegen")
    (whitelist-ir "LLM cannot invent types outside the IR enum whitelists")
    (three-layer-codegen "Quarks (L0, fragments) -> Atoms (L1, one construct) -> Patterns (L2, complete file)")
    (layer-enforcement "CI guardrail script rejects direct struct/enum/impl generation in L2 pattern files; enforces L2->L1 composition")
    (intent-governance "Multi-file granularity enforcement: L1-Blueprint (master index, <=200 lines) -> L2-Topology (flow/wiring) -> L3-Implementation (code-level detail)")
    (governance-mode "Component-level AI behavior control: descriptive (read-only context, no codegen), strict-codegen (controlled generation, requires proof), experimental (AI sandbox, allows mutation experiments)")
    (strict-isolation (forge-core forge-daemon))
    (communication (must-use http-api))
    (db-access (only forge-daemon)))

  ;; ══════════════════════════════════════════
  ;; Pillar: forge-core
  ;; ══════════════════════════════════════════
  (pillar forge-core
    (purpose "Core libraries -- S-EXP parsing, AST scanning, reality mirror, graph mapping, legacy IR/codegen, universe graph, cascade repair, survey, flywheel strategy/evolution")

    (component sexp-parser
      (role "Recursive descent S-expression parser with rich query API")
      (invariants "Zero external dependencies; pure algorithm")
      (data-flow "String input -> tokenize -> recursive parse -> SexpNode tree")
      (files crates/forge-core/src/sexp_parser.rs)
      (symbols
        (enum SexpNode (sig "pub enum SexpNode { Atom(String), List(Vec<SexpNode>) }"))
        (function parse (sig "pub fn parse(input: &str) -> Result<Vec<SexpNode>, String>"))
        (function parse_one (sig "pub fn parse_one(input: &str) -> Result<SexpNode, String>"))
        (function as_atom (sig "pub fn as_atom(&self) -> Option<&str>"))
        (function children (sig "pub fn children(&self) -> &[SexpNode]"))
        (function tag (sig "pub fn tag(&self) -> Option<&str>"))
        (function id (sig "pub fn id(&self) -> Option<&str>"))
        (function body (sig "pub fn body(&self) -> Vec<&SexpNode>"))
        (function find (sig "pub fn find(&self, tag: &str) -> Option<&SexpNode>"))
        (function find_all (sig "pub fn find_all(&self, tag: &str) -> Vec<&SexpNode>"))
        (function str_val (sig "pub fn str_val(&self, tag: &str) -> Option<&str>"))
        (function list_val (sig "pub fn list_val(&self, tag: &str) -> Vec<&str>"))
        (function human_label (sig "pub fn human_label(&self) -> Option<&str>"))
        (function human_desc (sig "pub fn human_desc(&self) -> Option<&str>"))
        (function to_pretty (sig "pub fn to_pretty(&self, indent: usize) -> String"))))

    (component scanner
      (role "Pluggable AST scanner -- extracts code symbols via tree-sitter for Rust, TypeScript, Python")
      (invariants
        "No database dependency; uses ignore crate to skip node_modules/target/.git"
        "Pure in-memory pipeline"
        "Supports three languages: Rust, TypeScript, Python")
      (data-flow "project_root -> WalkBuilder -> file discovery -> tree-sitter parse -> Vec<CodeNode>")
      (files crates/forge-core/src/scanner.rs)
      (symbols
        (struct CodeNode (sig "pub struct CodeNode { id, file_path, node_type, name, signature, start_line, end_line, is_exported, docstring, stub_content, calls, beacons }"))
        (function detect_language (sig "pub fn detect_language(project_root: &Path) -> String"))
        (function auto_scan (sig "pub fn auto_scan(project_root: &Path, language: &str) -> Result<Vec<CodeNode>>"))
        (function extract_rust (sig "pub fn extract_rust(file_path: &str, source: &str) -> Vec<CodeNode>"))
        (function extract_typescript (sig "pub fn extract_typescript(file_path: &str, source: &str) -> Vec<CodeNode>"))
        (function extract_python (sig "pub fn extract_python(file_path: &str, source: &str) -> Vec<CodeNode>"))))

    (component reality-mirror
      (role "Generates reality.sexp from AST scan results -- pure in-memory, no DB dependency")
      (invariants "Never reads from PostgreSQL; takes Vec<CodeNode> directly")
      (data-flow "Vec<CodeNode> + optional existing topology -> cluster -> build S-EXP tree -> write .jarvis/reality.sexp")
      (files crates/forge-core/src/reality_mirror.rs)
      (symbols
        (function generate (sig "pub fn generate(nodes: &[CodeNode], project_name: &str, existing_topology: Option<&str>) -> String"))
        (function write_to_disk (sig "pub fn write_to_disk(project_root: &Path, nodes: &[CodeNode], project_name: &str, existing_topology: Option<&str>) -> Result<PathBuf>"))))

    (component sexp-mapper
      (role "Maps parsed S-EXP topology tree into graph nodes/edges for visualization")
      (files crates/forge-core/src/sexp_mapper.rs crates/forge-core/src/graph_types.rs)
      (symbols
        (struct MappedGraph (sig "pub struct MappedGraph { nodes: Vec<GraphNode>, edges: Vec<GraphEdge>, violations: Vec<Violation> }"))
        (struct GraphNode (sig "pub struct GraphNode { id, node_type: GraphNodeType, label, parent_id, position, data: NodeData }"))
        (enum GraphNodeType (sig "pub enum GraphNodeType { Infrastructure, Pillar, Component, Symbol, Submodule }"))
        (struct NodeData (sig "pub struct NodeData { description, human_label, human_description, purpose, exports, beacons, events, symbol_count }"))
        (struct GraphEdge (sig "pub struct GraphEdge { id, source, target, label, edge_type }"))
        (struct Violation (sig "pub struct Violation { source, target, description, severity }"))
        (function map_sexp (sig "pub fn map_sexp(root: &SexpNode) -> MappedGraph"))))

    (component code-graph
      (role "Fuses S-EXP topology with AST scan index -- links intent components to real code symbols")
      (files crates/forge-core/src/code_graph.rs)
      (symbols
        (function build_architecture_graph (sig "pub fn build_architecture_graph(...)"))
        (function humanize_symbol (sig "pub fn humanize_symbol(...)"))
        (function file_matches_component (sig "pub fn file_matches_component(...)"))))

    (component intent-graph
      (role "Multi-file intent governance IR -- parses .missiond/*.lisp into typed graph with granularity levels")
      (invariants
        "Every governed file must declare (granularity L1-Blueprint|L2-Topology|L3-Implementation)"
        "L2/L3 files must declare (parent ...) back-reference"
        "L1 uses (module-link ...) to delegate details to L2/L3"
        "Cycle detection via BFS on parent->child edges"
        "Non-governed files become orphans (backward-compatible)")
      (files crates/forge-core/src/intent_graph.rs)
      (symbols
        (enum GranularityLevel (sig "pub enum GranularityLevel { L1Blueprint, L2Topology, L3Implementation }"))
        (enum GovernanceMode (sig "pub enum GovernanceMode { Descriptive, StrictCodegen, Experimental }"))
        (struct IntentFileMeta (sig "pub struct IntentFileMeta { file_path, intent_name, level, parent, version, module_links, line_count, governance_mode }"))
        (struct ModuleLink (sig "pub struct ModuleLink { target_path, expected_level, source_pillar }"))
        (struct IntentGraph (sig "pub struct IntentGraph { project_name, root, files: Vec<IntentFileMeta>, edges, orphans }"))
        (function resolve_intent_graph (sig "pub fn resolve_intent_graph(missiond_dir: &Path) -> Result<IntentGraph, Vec<IrError>>"))
        (function has_cycle (sig "pub fn has_cycle(file_count: usize, edges: &HashSet<(usize, usize)>) -> bool"))))

    (component governance
      (role "Lint rule engine for intent granularity enforcement -- validates content and structure against declared level")
      (invariants
        "L1 files must not contain code-level detail (col :type, op, struct fields)"
        "L2 files must not contain implementation detail (col :type, SQL ops)"
        "Suggests file splits when line count or tag diversity exceeds thresholds")
      (files crates/forge-core/src/governance.rs)
      (symbols
        (enum ViolationSeverity (sig "pub enum ViolationSeverity { Error, Warning }"))
        (struct GranularityViolation (sig "pub struct GranularityViolation { file, line, rule, severity, message, suggestion }"))
        (struct SplitSuggestion (sig "pub struct SplitSuggestion { current_lines, table_count, api_count, flow_count, suggested_files }"))
        (function lint_content (sig "pub fn lint_content(meta: &IntentFileMeta, nodes: &[SexpNode]) -> Vec<GranularityViolation>"))
        (function lint_structure (sig "pub fn lint_structure(graph: &IntentGraph) -> Vec<GranularityViolation>"))
        (function suggest_split (sig "pub fn suggest_split(nodes: &[SexpNode], line_count: usize) -> Option<SplitSuggestion>"))))

    (component legacy-ir
      (role "Original Jarvis domain IR -- PillarKind, JarvisEvent, ComponentDecl for generation-gap codegen")
      (invariants "Pre-dates nc-jarvis-ir; used by forge-daemon for Jarvis-specific topology")
      (files crates/forge-core/src/ir.rs)
      (symbols
        (enum PillarKind (sig "pub enum PillarKind { Memory, Control, Tools }"))
        (enum JarvisEvent (sig "pub enum JarvisEvent { MessagesIngested, SessionStateChanged, UserMessageReceived, TextComplete, ContextReady, ToolCalled, CodebaseMutated, Custom(String) }"))
        (enum StateDep (sig "pub enum StateDep { Storage, SessionManager, EventBus, TopologyManager, RealityMirror }"))
        (struct ComponentDecl (sig "pub struct ComponentDecl { pillar, name, role, depends_on, listens_to, state_deps, invariants, data_flow, logic_fns }"))
        (struct LogicFnDecl (sig "pub struct LogicFnDecl { name, inputs: Vec<(String, String)>, output, steps }"))
        (struct FlowDecl (sig "pub struct FlowDecl { name, steps: Vec<String> }"))
        (struct IrError (sig "pub struct IrError { field, message, expected, got }"))))

    (component legacy-codegen
      (role "Original generation-gap code generator -- ComponentDecl -> (scaffold, impl_stub) pair")
      (files crates/forge-core/src/codegen.rs)
      (symbols
        (function generate_component (sig "pub fn generate_component(decl: &ComponentDecl) -> (String, String)"))))

    (component universe-graph
      (role "Cross-service dependency DAG -- parses universe.intent.lisp manifest, resolves upstream-dependency edges, computes blast radius")
      (invariants
        "Topological sort for repair ordering"
        "Cycle detection via BFS"
        "Validates universe integrity: orphans, undeclared cross-service refs")
      (data-flow "universe.intent.lisp -> parse manifest -> scan each service intent.lisp -> build DAG -> blast radius query")
      (files crates/forge-core/src/universe_graph.rs)
      (symbols
        (struct ServiceEntry (sig "pub struct ServiceEntry { id, path }"))
        (struct ServiceDependency (sig "pub struct ServiceDependency { provider_id, interfaces }"))
        (struct UniverseGraph (sig "pub struct UniverseGraph { services, dependencies, adjacency }"))
        (struct ServiceDelta (sig "pub struct ServiceDelta { service_id, changed_interfaces }"))
        (struct ImpactedService (sig "pub struct ImpactedService { service_id, depth, affected_interfaces }"))
        (struct UniverseViolation (sig "pub struct UniverseViolation { ... }"))
        (function parse_universe_manifest (sig "pub fn parse_universe_manifest(manifest_path: &Path) -> Result<Vec<ServiceEntry>, String>"))
        (function extract_upstream_dependencies (sig "pub fn extract_upstream_dependencies(...)"))
        (function resolve_universe_graph (sig "pub fn resolve_universe_graph(manifest_path: &Path) -> Result<UniverseGraph, Vec<String>>"))
        (function compute_blast_radius (sig "pub fn compute_blast_radius(graph: &UniverseGraph, delta: &ServiceDelta) -> Vec<ImpactedService>"))
        (function validate_universe_integrity (sig "pub fn validate_universe_integrity(graph: &UniverseGraph) -> Vec<UniverseViolation>"))))

    (component cascade
      (role "Topological repair orchestrator -- blast radius -> phased plan -> Forge build + cargo check + Mechanic repair per phase")
      (invariants
        "Hard halt on max_repair_cycles exceeded"
        "Skips all downstream dependents on phase failure"
        "Supports cancellation via CancelToken for sentinel preemption"
        "Monorepo-aware: scoped check/clippy, sub-path in shadow branches")
      (data-flow "ServiceDelta -> blast radius -> CascadePlan -> execute phases -> CascadeReport")
      (files crates/forge-core/src/cascade.rs)
      (symbols
        (struct CascadeConfig (sig "pub struct CascadeConfig { max_repair_cycles, phase_timeout, dry_run, enable_clippy }"))
        (enum CascadeAction (sig "pub enum CascadeAction { ForgeBuild, CargoCheck, MechanicRepair, ... }"))
        (enum PhaseStatus (sig "pub enum PhaseStatus { Pending, Running, Passed, Failed, HardHalt, Skipped }"))
        (struct CascadePhase (sig "pub struct CascadePhase { service_id, depth, status, actions }"))
        (struct CascadePlan (sig "pub struct CascadePlan { phases, ... }"))
        (struct CascadeReport (sig "pub struct CascadeReport { phases, stats, ... }"))
        (function create_plan (sig "pub fn create_plan(graph: &UniverseGraph, delta: &ServiceDelta, ...) -> CascadePlan"))
        (function execute_plan (sig "pub fn execute_plan(plan: &mut CascadePlan) -> CascadeReport"))
        (function execute_plan_with_cancel (sig "pub fn execute_plan_with_cancel(plan: &mut CascadePlan, cancel: CancelToken) -> CascadeReport"))))

    (component repair-log
      (role "Repair record persistence -- serializes CascadeReport to JSON log files for post-mortem analysis")
      (data-flow "CascadeReport -> RepairRecord -> save to .forge-logs/ -> load for distillation")
      (files crates/forge-core/src/repair_log.rs)
      (symbols
        (struct RepairRecord (sig "pub struct RepairRecord { timestamp, service_deltas, phases, ... }"))
        (struct PhaseRecord (sig "pub struct PhaseRecord { service_id, status, cycles, ... }"))
        (function record_from_report (sig "pub fn record_from_report(report: &CascadeReport) -> RepairRecord"))
        (function save_repair_record (sig "pub fn save_repair_record(record: &RepairRecord, log_dir: &Path) -> Result<PathBuf, String>"))
        (function load_repair_history (sig "pub fn load_repair_history(log_dir: &Path) -> Result<Vec<RepairRecord>, String>"))))

    (component strategy
      (role "P5 flywheel -- distills repair history into recurring patterns and briefing updates for AI agents")
      (data-flow "Vec<RepairRecord> -> pattern clustering -> DistillationReport -> briefing Markdown")
      (files crates/forge-core/src/strategy.rs)
      (symbols
        (struct RepairPattern (sig "pub struct RepairPattern { pattern_type, frequency, services, ... }"))
        (struct DistillationReport (sig "pub struct DistillationReport { patterns, stats }"))
        (struct DistillationStats (sig "pub struct DistillationStats { total_records, ... }"))
        (function distill (sig "pub fn distill(records: &[RepairRecord]) -> DistillationReport"))
        (function generate_briefing_update (sig "pub fn generate_briefing_update(patterns: &[RepairPattern]) -> String"))))

    (component evolution
      (role "P6 flywheel -- analyzes repair patterns to propose new Forge molds, closing the automation gap")
      (data-flow "DistillationReport -> pattern analysis -> MoldProposal ranking -> EvolutionReport")
      (files crates/forge-core/src/evolution.rs)
      (symbols
        (struct MoldProposal (sig "pub struct MoldProposal { name, description, priority, utility, ... }"))
        (enum ProposalPriority (sig "pub enum ProposalPriority { Critical, High, Medium, Low }"))
        (struct EvolutionReport (sig "pub struct EvolutionReport { proposals, ... }"))
        (function generate_proposals (sig "pub fn generate_proposals(report: &DistillationReport, min_utility: f64) -> EvolutionReport"))
        (function format_report_markdown (sig "pub fn format_report_markdown(report: &EvolutionReport) -> String"))))

    (component survey
      (role "LLM-assisted intent.lisp generator -- AST scan -> structured prompt -> claude CLI -> validated S-EXP output")
      (invariants
        "Phase 1 is pure (no LLM): file listing + symbol extraction + survey hash"
        "Phase 2 invokes claude -p with embedded cartographer prompt + templates"
        "Phase 3 validates output via sexp_parser"
        "Survey hash detects staleness without re-running LLM")
      (data-flow "project_root -> AST scan -> prompt assembly -> claude -p -> sexp_parser validation -> SurveyResult")
      (files crates/forge-core/src/survey.rs)
      (symbols
        (enum SurveyLevel (sig "pub enum SurveyLevel { L1Blueprint, L2Topology, L3Implementation }"))
        (struct SurveyConfig (sig "pub struct SurveyConfig { project_root, level, check, dry_run }"))
        (struct SurveyResult (sig "pub struct SurveyResult { intent_lisp, survey_hash, file_count, symbol_count, ... }"))
        (function run (sig "pub fn run(config: &SurveyConfig) -> Result<SurveyResult, String>")))))

  ;; ══════════════════════════════════════════
  ;; Pillar: nc-jarvis-ir (Y-pipeline IR layer)
  ;; ══════════════════════════════════════════
  (pillar nc-jarvis-ir
    (purpose "Typed whitelist IR -- validates Lisp S-expressions into target-specific typed structures")

    (component common
      (role "Shared error types and S-EXP extraction helpers for all three pipelines")
      (files crates/nc-jarvis-ir/src/common.rs)
      (symbols
        (struct IrError (sig "pub struct IrError { field, message, expected, got }"))))

    (component rust-ir
      (role "Rust-target IR -- 15 typed declaration structs with from_sexp parsers")
      (invariants
        "Contains Rust-native type references (Arc<T>, sqlx, serde_json::Value) meaningless to TS/Python"
        "Each struct has a from_sexp() constructor that validates against enum whitelists")
      (files crates/nc-jarvis-ir/src/rust/)
      (symbols
        (struct AdminRouteRegistryDecl (sig "admin route table with method + path + handler"))
        (struct BootstrapDecl (sig "application init: infra, modules, workers, target"))
        (struct ConnectorDecl (sig "LLM provider connector: base-url, auth, timeout, capabilities, secret-keys; Generation Gap trait-impl-only"))
        (struct CrudGatewayDecl (sig "tables with columns, ops, target path, identity; sub-types: TableDecl, ColumnDecl, IndexDecl, CompositePk, UniqueConstraint, CrudOpDecl"))
        (struct DbStoreDecl (sig "PostgreSQL store: table, entity, trait-name, queries; Generation Gap delegates to _impl() methods"))
        (struct DomainEngineDecl (sig "domain logic engine declaration"))
        (struct DomainTypesDecl (sig "shared domain type definitions"))
        (struct ErrorEnumDecl (sig "error enum with variants and HTTP status codes"))
        (struct McpModuleDecl (sig "MCP tool module: name, struct_name, tools, deps, target"))
        (struct PureUtilityDecl (sig "pure stateless utility function module"))
        (struct RpcGatewayDecl (sig "RPC route gateway with handlers"))
        (struct S3DispatcherDecl (sig "S3/MinIO dispatch layer"))
        (struct S3ResponseBuilderDecl (sig "S3 presigned URL response builder"))
        (struct StateMachineDecl (sig "state machine: states, transitions, guards, traits, groups"))
        (struct WorkerDecl (sig "async background worker: name, pattern, channels, target"))))

    (component ts-ir
      (role "TypeScript-target IR -- Vue SFC, Pinia stores, canvas nodes, external engine contracts")
      (invariants
        "Handles import tree management"
        "Supports external-engine declarations for black-box components")
      (files crates/nc-jarvis-ir/src/ts/)
      (symbols
        (struct CanvasNodeDecl (sig "pub struct CanvasNodeDecl -- input/output slots for React Flow nodes"))
        (struct NodeSlot (sig "pub struct NodeSlot -- single slot definition"))
        (struct ExternalEngineDecl (sig "pub struct ExternalEngineDecl { name, import_from, accepts_props, emits_events, implements_trait }"))
        (struct VueComponentDecl (sig "pub struct VueComponentDecl -- props, emits, slots for Vue SFC"))
        (struct ComponentProp (sig "single prop declaration"))
        (struct ComponentEmit (sig "single emit declaration"))
        (struct ComponentSlot (sig "single slot declaration"))
        (struct PiniaStoreDecl (sig "pub struct PiniaStoreDecl -- state fields, getters, actions"))
        (struct StoreField (sig "reactive state field"))
        (struct StoreGetter (sig "computed getter"))
        (struct StoreAction (sig "store action with parameters"))
        (struct ActionParam (sig "action parameter definition"))))

    (component python-ir
      (role "Python-target IR -- ComfyUI node definitions with INPUT_TYPES/RETURN_TYPES metadata")
      (files crates/nc-jarvis-ir/src/python/)
      (symbols
        (struct ComfyNodeDecl (sig "pub struct ComfyNodeDecl -- full ComfyUI node definition"))
        (struct ComfyInputDecl (sig "single input with type, default, options"))
        (struct ComfyInputOptions (sig "min/max/step/multiline constraints"))
        (struct ComfyHiddenDecl (sig "hidden input declaration"))
        (enum ComfyIOType (sig "pub enum ComfyIOType -- IMAGE, MASK, MODEL, CLIP, etc."))
        (function parse_comfy_nodes (sig "pub fn parse_comfy_nodes(nodes: &[SexpNode]) -> Result<Vec<ComfyNodeDecl>, IrError>")))))

  ;; ══════════════════════════════════════════
  ;; Pillar: nc-jarvis-gen (Y-pipeline codegen layer)
  ;; ══════════════════════════════════════════
  (pillar nc-jarvis-gen
    (purpose "Deterministic code generators -- IR to compilable source across Rust, TypeScript, Python")

    (component common
      (role "Shared utilities -- indentation writer, forge header, file safety")
      (files crates/nc-jarvis-gen/src/common.rs)
      (symbols
        (function w (sig "pub fn w(out: &mut String, indent: usize, line: &str)"))
        (function blank (sig "pub fn blank(out: &mut String)"))
        (function forge_header (sig "pub fn forge_header(out: &mut String, lang: &str, source: &str)"))))

    (component quarks
      (role "Layer 0 -- cross-language semantic primitives; return fragments, not compilable code")
      (invariants
        "Pure functions, no state, deterministic"
        "Sit above rust/ ts/ python/ -- usable by all target pipelines"
        "Non-compilable: quark output is a fragment, not valid code")
      (files crates/nc-jarvis-gen/src/quarks/)

      (subcomponent type-map
        (role "Cross-language type mapping -- IR types <-> JSON Schema <-> Rust/TS/Python types; dual-dialect PG+SQLite")
        (files crates/nc-jarvis-gen/src/quarks/type_map.rs)
        (symbols
          (enum JsonType (sig "pub enum JsonType { String, Integer, Number, Boolean, Array, Object }"))
          (function from_ir_arg_type (sig "pub fn from_ir_arg_type(arg_type: &ArgType) -> JsonType"))
          (function pg_ddl_type (sig "pub fn pg_ddl_type(col_type: &ColumnType) -> &'static str"))
          (function pg_rust_domain_type (sig "pub fn pg_rust_domain_type(col_type: &ColumnType, nullable: bool) -> String"))
          (function sqlite_ddl_type (sig "pub fn sqlite_ddl_type(col_type: &ColumnType) -> &'static str"))
          (function sqlite_rust_get_type (sig "pub fn sqlite_rust_get_type(col_type: &ColumnType) -> &'static str"))
          (function sqlite_rust_domain_type (sig "pub fn sqlite_rust_domain_type(col_type: &ColumnType, nullable: bool) -> String"))))

      (subcomponent ident
        (role "Identifier case conversion -- snake_case, camelCase, PascalCase, kebab-case")
        (files crates/nc-jarvis-gen/src/quarks/ident.rs)
        (symbols
          (function to_pascal_case (sig "pub fn to_pascal_case(s: &str) -> String"))
          (function to_snake_case (sig "pub fn to_snake_case(s: &str) -> String"))
          (function to_camel_case (sig "pub fn to_camel_case(s: &str) -> String"))
          (function to_kebab_case (sig "pub fn to_kebab_case(s: &str) -> String"))
          (function capitalize_first (sig "pub fn capitalize_first(s: &str) -> String"))
          (function swap_prefix (sig "pub fn swap_prefix(s: &str, from: &str, to: &str) -> String"))))

      (subcomponent doc-comment
        (role "Doc string formatting for Rust (///), TypeScript (/** */), Python (\"\"\")")
        (files crates/nc-jarvis-gen/src/quarks/doc_comment.rs)
        (symbols
          (function doc_line (sig "pub fn doc_line(lang: DocLang, text: &str) -> String"))
          (function doc_block (sig "pub fn doc_block(lang: DocLang, lines: &[&str]) -> String"))))

      (subcomponent derive-attr
        (role "Derive attribute generation for Rust structs")
        (files crates/nc-jarvis-gen/src/quarks/derive_attr.rs)
        (symbols
          (struct DeriveSet (sig "pub struct DeriveSet { derives: Vec<String> }"))
          (function deserialize_input (sig "pub fn deserialize_input() -> Self"))
          (function serialize_output (sig "pub fn serialize_output() -> Self"))
          (function serde_both (sig "pub fn serde_both() -> Self"))
          (function to_derive_line (sig "pub fn to_derive_line(&self) -> String")))))

    (component rust-gen
      (role "Rust code generation -- 18 atoms + 17 pattern generators")
      (files crates/nc-jarvis-gen/src/rust/)

      (subcomponent atoms
        (role "Layer 1 -- smallest indivisible codegen units; each generates ONE Rust construct")
        (invariants
          "Selection not creation -- atoms are lookup tables"
          "Each atom independently testable"
          "No cross-atom coupling -- atom A never calls atom B"
          "Deterministic -- same input always produces same output"
          "Language-agnostic IR -- atoms take typed structs, not S-expressions")
        (files crates/nc-jarvis-gen/src/rust/atoms/)
        (symbols
          (module struct_def (sig "StructDefInput { name, fields: Vec<FieldDef>, derives, doc, serde_rename_all, extra_struct_attrs, default_impl } -> struct with Default impl"))
          (module enum_def (sig "EnumDefInput { name, variants: Vec<VariantDef>, derives, derive_display, derive_from_str, serde_rename_all, extra_enum_attrs, gen_as_str_method } -> enum with Display/FromStr"))
          (module arg_extract (sig "JSON argument extraction with type coercion"))
          (module tool_schema (sig "MCP ToolSchema JSON registration block"))
          (module query_method (sig "sqlx::query_as!() method body"))
          (module ddl_column (sig "PostgreSQL DDL column definition"))
          (module row_to (sig "impl From<Row> for Struct conversion"))
          (module endpoint_route (sig "Axum route handler signature"))
          (module error_variant (sig "error enum variant with doc"))
          (module env_config (sig "environment variable config field"))
          (module di_field (sig "dependency injection field"))
          (module worker_loop (sig "async worker loop body"))
          (module w (sig "write helper: w(out, indent, line); re-exported at atoms root"))
          (module impl_block (sig "ImplBlockInput { type_name, trait_name, methods: Vec<MethodDef> } -> impl block; MethodDef supports attributes"))
          (module trait_contract (sig "TraitContractInput { name, methods: Vec<TraitMethodDef>, is_async_trait } -> trait def; has_self toggle for static methods"))
          (module match_arm (sig "MatchExprInput { subject, arms: Vec<MatchArmDef>, fallback, indent } -> match expression"))
          (module sqlx_query (sig "SqlxQueryInput { table, op: SqlxOpKind, columns, binds, where_conditions, set_clauses, on_conflict, row_type, pool_field } -> sqlx query chain"))
          (module pg_ddl (sig "CreateTableInput/MigrateTableInput -> CREATE TABLE DDL + sqlx migrate wrapper"))))

      (subcomponent patterns
        (role "Layer 2 -- compose atoms into complete generated files")
        (files crates/nc-jarvis-gen/src/rust/patterns/)
        (symbols
          (function generate_admin_route_registry (sig "AdminRouteRegistryDecl -> route table .rs"))
          (function generate_bootstrap (sig "BootstrapDecl -> gen_init.rs"))
          (function generate_connector (sig "ConnectorDecl -> trait-impl-only connector/generated.rs (no struct/constructor)"))
          (function generate_crud_gateway (sig "CrudGatewayDecl -> PostgreSQL storage/generated.rs"))
          (function generate_db_store (sig "DbStoreDecl -> PostgreSQL store/generated.rs + trait delegation"))
          (function generate_crud_gateway_rusqlite (sig "CrudGatewayDecl -> SQLite storage/generated.rs"))
          (function generate_domain_engine (sig "DomainEngineDecl -> engine/generated.rs"))
          (function generate_domain_types (sig "DomainTypesDecl -> types/generated.rs"))
          (function generate_error_enum (sig "ErrorEnumDecl -> error/generated.rs"))
          (function generate_mcp_module (sig "McpModuleDecl -> tools/*/generated.rs"))
          (function generate_missiond_tool (sig "MissionD-specific MCP tool variant"))
          (function generate_pure_utility (sig "PureUtilityDecl -> utility/generated.rs"))
          (function generate_rpc_gateway (sig "RpcGatewayDecl -> api/generated.rs"))
          (function generate_s3_dispatcher (sig "S3DispatcherDecl -> s3/generated.rs + custom variant"))
          (function generate_s3_response_builders (sig "S3ResponseBuilderDecl -> response/generated.rs + custom"))
          (function generate_state_machine (sig "StateMachineDecl -> state/generated.rs"))
          (function generate_worker (sig "WorkerDecl -> worker/generated.rs")))))

    (component ts-gen
      (role "TypeScript/Vue code generation -- 3 atoms + 3 pattern generators")
      (invariants "Outputs valid TypeScript; uses Zod for runtime validation where TS compiler cannot help")
      (files crates/nc-jarvis-gen/src/ts/)

      (subcomponent atoms
        (role "TS atoms: interface definition, Zod schema, import management")
        (files crates/nc-jarvis-gen/src/ts/atoms/)
        (symbols
          (module ts_interface (sig "export interface Foo { prop: Type }"))
          (module zod_schema (sig "const FooSchema = z.object({ ... })"))
          (module import_stmt (sig "import declaration management"))))

      (subcomponent patterns
        (role "TS patterns: Vue SFC, Pinia store, canvas registry")
        (files crates/nc-jarvis-gen/src/ts/patterns/)
        (symbols
          (function generate_vue_component (sig "VueComponentDecl -> defineComponent SFC"))
          (function generate_pinia_store (sig "PiniaStoreDecl -> defineStore module"))
          (function generate_canvas_registry (sig "CanvasNodeDecl -> React Flow node registry")))))

    (component python-gen
      (role "Python code generation -- 4 atoms + 3 pattern generators for ComfyUI")
      (invariants "Outputs valid Python; ComfyUI node protocol compliance")
      (files crates/nc-jarvis-gen/src/python/)

      (subcomponent atoms
        (role "Python atoms: class definition, ComfyUI INPUT_TYPES, RETURN_TYPES, schema node")
        (files crates/nc-jarvis-gen/src/python/atoms/)
        (symbols
          (module py_class_def (sig "class Foo: ... with decorators"))
          (module py_input_types (sig "ComfyUI INPUT_TYPES dict"))
          (module py_return_types (sig "ComfyUI RETURN_TYPES tuple"))
          (module py_schema_node (sig "ComfyUI schema node helper"))))

      (subcomponent patterns
        (role "Python patterns: ComfyUI node, registry, schema node generation")
        (files crates/nc-jarvis-gen/src/python/patterns/)
        (symbols
          (function generate_comfy_node (sig "ComfyNodeDecl -> full ComfyUI node .py"))
          (function generate_comfy_registry (sig "Vec<ComfyNodeDecl> -> NODE_CLASS_MAPPINGS registry"))
          (function generate_comfy_schema_node (sig "ComfyNodeDecl -> schema node helper"))))))

  ;; ══════════════════════════════════════════
  ;; Pillar: forge-cli
  ;; ══════════════════════════════════════════
  (pillar forge-cli
    (purpose "Stateless code stamping compiler -- reads .missiond/*.lisp, validates IR, writes generated files")
    (files crates/forge-cli/src/)

    (component dispatcher
      (role "CLI entry point -- discovers .lisp files, dispatches to pattern processors, writes output")
      (files crates/forge-cli/src/main.rs)
      (invariants
        "Skips intent.lisp (architecture spec, no codegen)"
        "Refuses to write files without GENERATED BY FORGE header"
        "Batch nodes (comfy-node etc.) collected and processed at end")
      (symbols
        (command build (sig "forge build [PROJECT_ROOT] [--dry-run] [--output-dir PATH]"))
        (command check (sig "forge check [PROJECT_ROOT] -- validate only, no write; includes advisory governance lint"))
        (command lint (sig "forge lint [PROJECT_ROOT] -- governance lint: granularity levels, structural rules, split hints"))
        (command audit (sig "forge audit [PROJECT_ROOT] [--json] -- three-state status report"))
        (command universe (sig "forge universe MANIFEST [--format text|json] -- display Universe Graph"))
        (command blast-radius (sig "forge blast-radius MANIFEST --service ID --changed iface1,iface2"))
        (command lint-universe (sig "forge lint-universe MANIFEST -- validate universe integrity"))
        (command distill (sig "forge distill LOG_DIR [--output FILE] [--format text|json] -- P5 repair pattern distillation"))
        (command evolve (sig "forge evolve LOG_DIR [--output FILE] [--min-utility 0.3] [--format text|json] -- P6 mold evolution"))
        (command survey (sig "forge survey [PROJECT_ROOT] [--level L1|L2|L3] [--check] [--dry-run] -- LLM-assisted intent.lisp generation"))
        (command cascade (sig "forge cascade MANIFEST --service ID --changed iface1,iface2 [--max-cycles N] [--dry-run]"))))

    (component registry
      (role "Tag-based pattern registry -- maps S-EXP tag -> PatternProcessor, eliminates hardcoded filename matching")
      (files crates/forge-cli/src/registry.rs)
      (symbols
        (trait PatternProcessor (sig "fn tag(&self) -> &str; fn process(&self, node: &SexpNode, ctx: &ForgeContext) -> Result<Vec<GeneratedFile>, String>"))
        (trait BatchPatternProcessor (sig "fn tag(&self) -> &str; fn process_batch(&self, nodes: &[&SexpNode], ctx: &ForgeContext) -> Result<Vec<GeneratedFile>, String>"))
        (struct PatternRegistry (sig "pub struct PatternRegistry { processors, batch_processors }"))
        (struct ForgeContext (sig "pub struct ForgeContext { project_root, dry_run, output_dir }"))
        (struct GeneratedFile (sig "pub struct GeneratedFile { path, content, overwrite }"))))

    (component layer-guardrail
      (role "CI script enforcing L2->L1 composition -- rejects direct struct/enum/impl generation in pattern files")
      (files crates/nc-jarvis-gen/scripts/check_layer_violations.sh)
      (invariants
        "Grep-based detection of w()/format!() struct/enum/impl generation in patterns/"
        "Whitelisted exceptions: error_enum (thiserror tuples), rpc_gateway (tagged union), crud (residual impl shell), mcp_tool (derive strip), pure_utility (unit struct)"))

    (component audit
      (role "Three-state implementation status scanner -- cross-references IR declarations with AST source")
      (files crates/forge-cli/src/audit.rs)
      (symbols
        (enum AuditStatus (sig "pub enum AuditStatus { Implemented { lines }, Pending, Missing }"))
        (struct AuditEntry (sig "pub struct AuditEntry { method, status, file, start_line, end_line }"))
        (struct PatternAudit (sig "pub struct PatternAudit { pattern, entries... }")))))

  ;; ══════════════════════════════════════════
  ;; Pillar: forge-daemon
  ;; ══════════════════════════════════════════
  (pillar forge-daemon
    (purpose "HTTP API daemon -- serves Forge UI, multi-project lifecycle, intent editing, coverage metrics")
    (files crates/forge-daemon/src/main.rs)

    (component api
      (role "Axum HTTP server on port 9210")
      (depends-on forge-core nc-jarvis-ir)
      (invariants
        "CORS enabled for UI on port 9211"
        "All state in AppState Arc<RwLock>"
        "PostgreSQL-backed for project registry and intent history")
      (symbols
        (struct AppState (sig "struct AppState { active_project: RwLock<PathBuf>, db: PgPool }"))
        (endpoint health (sig "GET /api/health"))
        (endpoint list_intent_files (sig "GET /api/intent/files -- list .missiond/*.lisp with kind detection"))
        (endpoint get_intent_graph (sig "GET /api/intent/graph -- governance graph: files, edges, orphans, granularity levels"))
        (endpoint lint_intent (sig "GET /api/intent/lint -- governance lint violations"))
        (endpoint get_intent_source_raw (sig "GET /api/intent/source -- raw Lisp file contents"))
        (endpoint get_intent_history (sig "GET /api/intent/history -- edit history (DB-backed)"))
        (endpoint validate_intent (sig "GET /api/intent/validate -- validation feedback with IR errors"))
        (endpoint generate_preview (sig "GET /api/forge/generate -- code generation preview"))
        (endpoint get_code_map_graph (sig "GET /api/code-map/graph -- architecture graph visualization"))
        (endpoint get_code_map_dual_graph (sig "GET /api/code-map/dual-graph -- intent vs reality comparison"))
        (endpoint list_projects (sig "GET /api/projects -- ordered by sort_order"))
        (endpoint reorder_projects (sig "PUT /api/projects/reorder -- drag-and-drop sort order persistence"))
        (endpoint register_project (sig "POST /api/projects/register"))
        (endpoint activate_project (sig "POST /api/projects/{name}/activate -- switch active project"))
        (endpoint scan_project (sig "POST /api/projects/{name}/scan -- run AST scanner"))
        (endpoint get_project_reality (sig "GET /api/projects/{name}/reality -- current reality.sexp"))
        (endpoint write_project_intent (sig "POST /api/projects/{name}/intent -- write intent.lisp"))
        (endpoint get_project_coverage (sig "GET /api/projects/{name}/coverage -- coverage metrics")))))

  ;; ══════════════════════════════════════════
  ;; Pillar: forge-ui (packages/ui/)
  ;; ══════════════════════════════════════════
  (pillar forge-ui
    (purpose "Next.js dashboard -- project management, intent editing, coverage metrics, architecture canvas")
    (files packages/ui/)

    (component sidebar
      (role "Project list with drag-and-drop reordering, language icon, status badge, coverage percentage")
      (files packages/ui/src/app/page.tsx))

    (component intent-viewer
      (role "S-EXP editor with live validation feedback")
      (files packages/ui/src/components/IntentViewer.tsx))

    (component canvas
      (role "ReactFlow architecture graph -- nodes from intent.lisp, coverage coloring, cross-project view")
      (invariants "ReactFlow is an external-engine; Forge generates node data, not canvas interaction code")
      (status planned)))

  ;; ══════════════════════════════════════════
  ;; Pillar: prompts (AI cartographer protocol)
  ;; ══════════════════════════════════════════
  (pillar prompts
    (purpose "AI cartographer protocol and granularity-level templates for generating governed .missiond/*.lisp files")
    (files prompts/)

    (component cartographer
      (role "Deep Cartographer Protocol v3 -- instructions for AI to scan codebases and generate layered intent files")
      (files prompts/cartographer.md))

    (component templates
      (role "Starter templates for each granularity level")
      (files prompts/templates/)
      (symbols
        (template l1-blueprint (sig "prompts/templates/l1-blueprint.lisp -- master index skeleton"))
        (template l2-topology (sig "prompts/templates/l2-topology.lisp -- flow/wiring skeleton"))
        (template l3-implementation (sig "prompts/templates/l3-implementation.lisp -- code-level detail skeleton")))))

  ;; ══════════════════════════════════════════
  ;; Flows
  ;; ══════════════════════════════════════════
  (flows
    (flow rust-stamping
      (description "Lisp -> IR -> Rust codegen (mature, 17 patterns)")
      (steps
        user-writes-missiond-lisp
        -> forge-cli-discovers-files
        -> sexp-parser-parses
        -> rust-ir-validates-via-from_sexp
        -> rust-gen-composes-atoms-into-patterns
        -> write-generated-rs))

    (flow ts-stamping
      (description "Lisp -> IR -> TypeScript codegen (3 patterns: Vue, Pinia, canvas)")
      (steps
        user-writes-missiond-lisp-with-target-lang-ts
        -> forge-cli-discovers-files
        -> sexp-parser-parses
        -> ts-ir-validates
        -> ts-gen-produces-code
        -> write-generated-tsx))

    (flow python-stamping
      (description "Lisp -> IR -> Python codegen (3 patterns: ComfyUI node, registry, schema)")
      (steps
        user-declares-comfy-node-in-lisp
        -> forge-cli-collects-batch
        -> sexp-parser-parses
        -> python-ir-validates-via-parse_comfy_nodes
        -> python-gen-produces-code
        -> write-generated-py))

    (flow external-engine-integration
      (description "Black-box engine declared in Lisp -> Forge generates contracts + adapter layer")
      (steps
        user-declares-external-engine-in-lisp
        -> ts-ir-parses-ExternalEngineDecl
        -> ts-gen-produces-interface-and-event-bus
        -> human-writes-engine-implementation
        -> forge-validates-contract-compliance))

    (flow coverage-metrics
      (description "Scan project for GENERATED BY FORGE markers -> compute coverage -> display in UI")
      (steps
        daemon-receives-coverage-request
        -> scanner-walks-project-files
        -> count-forge-markers-vs-total
        -> return-coverage-json
        -> ui-sidebar-displays-percentage))

    (flow governance-lint
      (description "Parse .missiond/*.lisp into intent graph -> validate granularity levels -> lint content/structure -> suggest splits")
      (steps
        forge-lint-discovers-lisp-files
        -> intent-graph-resolves-parent-child-topology
        -> governance-lint-content-per-file
        -> governance-lint-structure-across-graph
        -> suggest-split-for-oversized-files
        -> output-violations-report))

    (flow audit
      (description "Cross-reference IR custom method declarations with AST source -- three-state report")
      (steps
        forge-audit-parses-missiond-lisp
        -> extract-custom-method-declarations
        -> scanner-walks-project-source
        -> match-declarations-to-functions
        -> classify-implemented-pending-missing
        -> output-terminal-or-json-report))

    (flow project-onboarding
      (description "Register -> Scan -> Reality Mirror -> manual intent.lisp authoring")
      (steps
        register-project
        -> auto-scan-ast
        -> write-reality-sexp
        -> user-authors-intent-lisp
        -> forge-build-generates-code))

    (flow cascade-repair
      (description "Cross-service topological repair -- blast radius -> phased Forge build + cargo check + Mechanic repair")
      (steps
        service-intent-changes
        -> universe-graph-resolves-dependencies
        -> compute-blast-radius
        -> create-cascade-plan-by-topological-depth
        -> execute-phases-sequentially
        -> forge-build-per-service
        -> cargo-check-validates
        -> mechanic-repairs-on-failure
        -> hard-halt-on-max-cycles
        -> save-repair-log
        -> output-cascade-report))

    (flow survey
      (description "LLM-assisted intent.lisp generation -- AST scan -> prompt assembly -> claude -p -> validated output")
      (steps
        scanner-walks-project-files
        -> extract-symbols-and-compute-survey-hash
        -> assemble-cartographer-prompt-with-templates
        -> invoke-claude-cli
        -> sexp-parser-validates-output
        -> write-intent-lisp-to-disk))

    (flow flywheel
      (description "Four-stroke automation feedback loop: cascade -> log -> distill -> evolve -> new molds")
      (steps
        cascade-executes-repair-phases
        -> repair-log-persists-records
        -> strategy-distills-recurring-patterns
        -> evolution-proposes-new-forge-molds
        -> human-reviews-and-implements-molds))))
