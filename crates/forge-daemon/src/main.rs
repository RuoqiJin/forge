//! Jarvis Forge daemon — serves the Code Map UI and provides codegen APIs.
//!
//! Port 9210 (HTTP API), UI on port 9211 (Next.js dev server).

use axum::{
    extract::{Path, State},
    response::Json,
    routing::{get, post, put},
    Router,
};
use forge_core::governance;
use forge_core::graph_types::MappedGraph;
use forge_core::intent_graph;
use forge_core::ir;
use forge_core::sexp_parser::SexpNode;
use serde_json::{json, Value};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tracing::info;

use tokio::sync::RwLock;

struct AppState {
    /// Currently active project root (switchable via API)
    active_project: RwLock<PathBuf>,
    db: PgPool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "forge_daemon=debug,info".parse().unwrap()),
        )
        .init();

    let project_root = std::env::var("FORGE_PROJECT_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            // Default: assume we're in ~/Projects/jarvis-forge, target is ~/Projects/jarvis
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
            PathBuf::from(home).join("Projects").join("jarvis")
        });

    info!(project_root = %project_root.display(), "Forge daemon starting");

    // ── Database ─────────────────────────────────────────────────────────
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://localhost/jarvis_forge".to_string());

    let db = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await?;

    info!("Connected to PostgreSQL");

    // Run migrations
    sqlx::migrate!("./migrations").run(&db).await?;
    info!("Migrations applied");

    // Register default project (jarvis) in DB
    sqlx::query("INSERT INTO projects (name, root_path) VALUES ($1, $2) ON CONFLICT (name) DO UPDATE SET root_path = $2")
        .bind("jarvis")
        .bind(project_root.to_string_lossy().as_ref())
        .execute(&db)
        .await?;

    let state = Arc::new(AppState { active_project: RwLock::new(project_root), db });

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/api/health", get(health))
        .route("/api/intent/source", get(get_intent_source_raw))
        .route("/api/intent/history", get(get_intent_history))
        .route("/api/intent/validate", get(validate_intent))
        .route("/api/intent/files", get(list_intent_files))
        .route("/api/intent/graph", get(get_intent_graph))
        .route("/api/intent/lint", get(lint_intent))
        .route("/api/forge/generate", get(generate_preview))
        .route("/api/code-map/graph", get(get_code_map_graph))
        .route("/api/code-map/dual-graph", get(get_code_map_dual_graph))
        .route("/api/projects", get(list_projects))
        .route("/api/projects/reorder", put(reorder_projects))
        .route("/api/projects/register", post(register_project))
        .route("/api/projects/{name}/activate", post(activate_project))
        .route("/api/projects/{name}/scan", post(scan_project))
        .route("/api/projects/{name}/reality", get(get_project_reality))
        .route("/api/projects/{name}/intent", post(write_project_intent))
        .route("/api/projects/{name}/coverage", get(get_project_coverage))
        .route("/api/projects/{name}/open-editor", post(open_project_in_editor))
        .route("/api/projects/{name}/color", put(set_project_color))
        .route("/api/universe", get(get_universe))
        .with_state(state)
        .layer(cors);

    let addr = "0.0.0.0:9210";
    let listener = tokio::net::TcpListener::bind(addr).await?;
    info!(addr = addr, "Forge daemon listening");

    axum::serve(listener, app).await?;
    Ok(())
}

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok", "service": "jarvis-forge" }))
}

/// Return intent.lisp as raw text (IntentViewer calls .text() on this).
/// Accepts optional `?file=` query param for multi-file support.
async fn get_intent_source_raw(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> axum::response::Response {
    let project_root = state.active_project.read().await.clone();
    let path = if let Some(file) = params.get("file") {
        // Resolve relative path from project root, with fallback search
        let candidate = project_root.join(file);
        if candidate.exists() {
            candidate
        } else {
            // Try .missiond/ and .jarvis/ prefixes
            let missiond = project_root.join(".missiond").join(file);
            let jarvis = project_root.join(".jarvis").join(file);
            if missiond.exists() { missiond }
            else if jarvis.exists() { jarvis }
            else { candidate }
        }
    } else {
        let missiond_intent = project_root.join(".missiond").join("intent.lisp");
        let jarvis_intent = project_root.join(".jarvis").join("intent.lisp");
        let legacy_intent = project_root.join("intent.lisp");
        if missiond_intent.exists() { missiond_intent }
        else if jarvis_intent.exists() { jarvis_intent }
        else { legacy_intent }
    };
    match tokio::fs::read_to_string(&path).await {
        Ok(content) => axum::response::Response::builder()
            .header("content-type", "text/plain; charset=utf-8")
            .header("x-file-path", path.display().to_string())
            .header("access-control-expose-headers", "x-file-path")
            .body(axum::body::Body::from(content))
            .unwrap(),
        Err(_) => axum::response::Response::builder()
            .status(404)
            .body(axum::body::Body::from("intent.lisp not found"))
            .unwrap(),
    }
}

/// Stub: return empty history (Forge doesn't snapshot intent.lisp versions yet).
async fn get_intent_history() -> Json<Value> {
    Json(json!({ "snapshots": [] }))
}

/// Validate intent.lisp against the IR whitelist. Returns errors or OK.
async fn validate_intent(State(state): State<Arc<AppState>>) -> Json<Value> {
    let project_root = state.active_project.read().await.clone();
    let path = project_root.join("intent.lisp");
    let content = match tokio::fs::read_to_string(&path).await {
        Ok(c) => c,
        Err(e) => return Json(json!({ "valid": false, "error": format!("cannot read: {e}") })),
    };

    let nodes = match SexpNode::parse(&content) {
        Ok(n) => n,
        Err(e) => return Json(json!({ "valid": false, "error": format!("parse error: {e}") })),
    };

    let mut components = Vec::new();
    let mut errors: Vec<Value> = Vec::new();

    // Walk the top-level intent form looking for pillars -> components
    for node in &nodes {
        for child in node.children() {
            let tag = child.tag().unwrap_or("");
            if tag == "pillar" {
                let pillar_name = child.children().get(1).and_then(|n| n.as_atom()).unwrap_or("");
                let pillar = match ir::PillarKind::from_str(pillar_name) {
                    Ok(p) => p,
                    Err(e) => { errors.push(json!(e)); continue; }
                };
                for comp_node in child.children().iter().skip(2) {
                    if comp_node.tag() == Some("component") {
                        match ir::ComponentDecl::from_sexp(comp_node, pillar) {
                            Ok(decl) => components.push(json!({
                                "name": decl.name,
                                "pillar": format!("{:?}", decl.pillar),
                                "role": decl.role,
                                "events": decl.listens_to.len(),
                                "deps": decl.depends_on,
                            })),
                            Err(e) => errors.push(json!(e)),
                        }
                    }
                }
            }
        }
    }

    // ── Governance lint (Phase 4) ───────────────────────────────────────
    let missiond_dir = project_root.join(".missiond");
    let governance_section = match intent_graph::resolve_intent_graph(&missiond_dir) {
        Ok(graph) => {
            let violations = governance::lint_structure(&graph);
            let error_count = violations
                .iter()
                .filter(|v| matches!(v.severity, governance::ViolationSeverity::Error))
                .count();
            let warning_count = violations
                .iter()
                .filter(|v| matches!(v.severity, governance::ViolationSeverity::Warning))
                .count();
            json!({
                "governed": true,
                "errors": error_count,
                "warnings": warning_count,
                "violations": violations.iter().map(|v| json!({
                    "file": v.file,
                    "line": v.line,
                    "rule": v.rule,
                    "severity": format!("{:?}", v.severity),
                    "message": v.message,
                    "suggestion": v.suggestion,
                })).collect::<Vec<_>>(),
            })
        }
        Err(_) => {
            json!({ "governed": false, "errors": 0, "warnings": 0, "violations": [] })
        }
    };

    Json(json!({
        "valid": errors.is_empty(),
        "components": components,
        "errors": errors,
        "governance": governance_section,
    }))
}

// ── Intent governance endpoints (Phase 4) ───────────────────────────────

/// Detect the S-EXP kind and optional granularity level from a .lisp file's content.
///
/// Returns `(kind, granularity)` where `kind` is the first S-EXP tag (e.g. "intent",
/// "crud-gateway") and `granularity` is the `(granularity ...)` declaration value if present.
fn detect_lisp_kind(content: &str) -> (Option<String>, Option<String>) {
    let nodes = match SexpNode::parse(content) {
        Ok(n) => n,
        Err(_) => return (None, None),
    };

    let root = match nodes.first() {
        Some(n) => n,
        None => return (None, None),
    };

    let kind = root.tag().map(String::from);

    // Check for (granularity ...) declaration — e.g. (granularity L1-Blueprint)
    let granularity = root.find("granularity").and_then(|g| {
        // The level value is the second child (first is the tag "granularity")
        g.children().get(1).and_then(|c| c.as_atom().map(String::from))
    });

    (kind, granularity)
}

/// GET /api/intent/files — list all .lisp intent files with kind, granularity, and governance summary.
async fn list_intent_files(State(state): State<Arc<AppState>>) -> Json<Value> {
    let project_root = state.active_project.read().await.clone();

    // Collect .lisp files from .missiond/, .jarvis/, and project root
    let search_dirs = [
        project_root.join(".missiond"),
        project_root.join(".jarvis"),
        project_root.clone(),
    ];

    let mut files: Vec<Value> = Vec::new();

    for dir in &search_dirs {
        if !dir.exists() {
            continue;
        }
        let entries = match tokio::fs::read_dir(dir).await {
            Ok(e) => e,
            Err(_) => continue,
        };

        let mut entries = entries;
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("lisp") {
                continue;
            }
            if !path.is_file() {
                continue;
            }

            let content = match tokio::fs::read_to_string(&path).await {
                Ok(c) => c,
                Err(_) => continue,
            };

            let (kind, granularity) = detect_lisp_kind(&content);
            let line_count = content.lines().count();

            // Relative path from project root for display
            let rel_path = path
                .strip_prefix(&project_root)
                .unwrap_or(&path)
                .display()
                .to_string();

            files.push(json!({
                "path": rel_path,
                "kind": kind,
                "granularity": granularity,
                "line_count": line_count,
            }));
        }
    }

    // ── Governance summary ──────────────────────────────────────────────
    let missiond_dir = project_root.join(".missiond");
    let governance = match intent_graph::resolve_intent_graph(&missiond_dir) {
        Ok(graph) => {
            let violations = governance::lint_structure(&graph);
            let error_count = violations
                .iter()
                .filter(|v| matches!(v.severity, governance::ViolationSeverity::Error))
                .count();
            let warning_count = violations
                .iter()
                .filter(|v| matches!(v.severity, governance::ViolationSeverity::Warning))
                .count();
            json!({
                "governed": true,
                "file_count": graph.files.len(),
                "orphan_count": graph.orphans.len(),
                "error_count": error_count,
                "warning_count": warning_count,
            })
        }
        Err(_) => {
            json!({
                "governed": false,
                "file_count": 0,
                "orphan_count": 0,
                "error_count": 0,
                "warning_count": 0,
            })
        }
    };

    Json(json!({
        "files": files,
        "governance": governance,
    }))
}

/// GET /api/intent/graph — resolve and return the full IntentGraph as JSON.
async fn get_intent_graph(State(state): State<Arc<AppState>>) -> Json<Value> {
    let project_root = state.active_project.read().await.clone();
    let missiond_dir = project_root.join(".missiond");

    match intent_graph::resolve_intent_graph(&missiond_dir) {
        Ok(graph) => {
            let files: Vec<Value> = graph
                .files
                .iter()
                .map(|f| {
                    json!({
                        "file_path": f.file_path.display().to_string(),
                        "intent_name": f.intent_name,
                        "level": f.level.label(),
                        "governance_mode": f.governance_mode.map(|m| m.label()),
                        "parent": f.parent,
                        "version": f.version,
                        "line_count": f.line_count,
                        "module_links": f.module_links.iter().map(|l| {
                            json!({
                                "target_path": l.target_path,
                                "expected_level": l.expected_level.map(|lv| lv.label()),
                                "source_pillar": l.source_pillar,
                            })
                        }).collect::<Vec<_>>(),
                    })
                })
                .collect();

            let edges: Vec<Value> = graph
                .edges
                .iter()
                .map(|&(from, to)| json!({"from": from, "to": to}))
                .collect();

            Json(json!({
                "governed": true,
                "project_name": graph.project_name,
                "root": graph.root,
                "files": files,
                "edges": edges,
                "orphans": graph.orphans.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
            }))
        }
        Err(errors) => {
            let all_missing = errors.iter().all(|e| e.field == "granularity");
            Json(json!({
                "governed": false,
                "reason": if all_missing { "no granularity tags (legacy project)" } else { "resolution errors" },
                "errors": errors.iter().map(|e| json!({
                    "field": e.field,
                    "message": e.message,
                })).collect::<Vec<_>>(),
            }))
        }
    }
}

/// GET /api/intent/lint — run governance lint and return all violations.
async fn lint_intent(State(state): State<Arc<AppState>>) -> Json<Value> {
    let project_root = state.active_project.read().await.clone();
    let missiond_dir = project_root.join(".missiond");

    match intent_graph::resolve_intent_graph(&missiond_dir) {
        Ok(graph) => {
            let mut all_violations = governance::lint_structure(&graph);

            // Content lint per governed file
            for meta in &graph.files {
                let full_path = missiond_dir.join(&meta.file_path);
                if let Ok(content) = tokio::fs::read_to_string(&full_path).await {
                    if let Ok(nodes) = SexpNode::parse(&content) {
                        all_violations.extend(governance::lint_content(meta, &nodes));
                    }
                }
            }

            let error_count = all_violations
                .iter()
                .filter(|v| matches!(v.severity, governance::ViolationSeverity::Error))
                .count();
            let warning_count = all_violations
                .iter()
                .filter(|v| matches!(v.severity, governance::ViolationSeverity::Warning))
                .count();

            Json(json!({
                "governed": true,
                "errors": error_count,
                "warnings": warning_count,
                "violations": all_violations.iter().map(|v| json!({
                    "file": v.file,
                    "line": v.line,
                    "rule": v.rule,
                    "severity": format!("{:?}", v.severity),
                    "message": v.message,
                    "suggestion": v.suggestion,
                })).collect::<Vec<_>>(),
            }))
        }
        Err(_) => {
            Json(json!({
                "governed": false,
                "errors": 0,
                "warnings": 0,
                "violations": [],
            }))
        }
    }
}

/// Preview code generation for validated components.
async fn generate_preview(State(state): State<Arc<AppState>>) -> Json<Value> {
    let path = state.active_project.read().await.join("intent.lisp");
    let content = match tokio::fs::read_to_string(&path).await {
        Ok(c) => c,
        Err(e) => return Json(json!({ "error": format!("cannot read: {e}") })),
    };

    let nodes = match SexpNode::parse(&content) {
        Ok(n) => n,
        Err(e) => return Json(json!({ "error": format!("parse error: {e}") })),
    };

    let mut generated = Vec::new();

    for node in &nodes {
        for child in node.children() {
            if child.tag() == Some("pillar") {
                let pillar_name = child.children().get(1).and_then(|n| n.as_atom()).unwrap_or("");
                let pillar = match ir::PillarKind::from_str(pillar_name) {
                    Ok(p) => p,
                    Err(_) => continue,
                };
                for comp_node in child.children().iter().skip(2) {
                    if comp_node.tag() == Some("component") {
                        if let Ok(decl) = ir::ComponentDecl::from_sexp(comp_node, pillar) {
                            let (scaffold, impl_stubs) = forge_core::codegen::generate_component(&decl);
                            generated.push(json!({
                                "component": decl.name,
                                "scaffold": scaffold,
                                "impl_stubs": impl_stubs,
                            }));
                        }
                    }
                }
            }
        }
    }

    Json(json!({ "generated": generated }))
}

// ── Code Map endpoints ──────────────────────────────────────────────────

/// Convert a MappedGraph into the JSON shape the CodeMap frontend expects.
fn mapped_graph_to_json(graph: &MappedGraph) -> Value {
    let nodes: Vec<Value> = graph
        .nodes
        .iter()
        .map(|n| {
            json!({
                "id": n.id,
                "type": n.node_type,
                "label": n.label,
                "data": n.data,
                "position": n.position.as_ref().map(|p| json!({ "x": p.x, "y": p.y })).unwrap_or(json!({ "x": 0, "y": 0 })),
                "parentId": n.parent_id,
            })
        })
        .collect();

    let edges: Vec<Value> = graph
        .edges
        .iter()
        .map(|e| {
            json!({
                "id": e.id,
                "source": e.source,
                "target": e.target,
                "label": e.label,
                "edgeType": e.edge_type,
            })
        })
        .collect();

    let violations: Vec<Value> = graph
        .violations
        .iter()
        .map(|v| {
            json!({
                "source": v.source,
                "target": v.target,
                "description": v.description,
                "severity": v.severity,
            })
        })
        .collect();

    json!({
        "nodes": nodes,
        "edges": edges,
        "violations": violations,
        "traces": [],
    })
}

/// GET /api/code-map/graph — renders architecture graph from intent.lisp (sole source of truth).
///
/// For mapped projects: reads intent.lisp → sexp_mapper → real-time AST scan in memory → fused graph.
/// For unmapped projects: returns empty graph with error message.
/// Jarvis legacy: falls back to jarvis-topology.sexp if no intent.lisp exists.
async fn get_code_map_graph(State(state): State<Arc<AppState>>) -> Json<Value> {
    let project_root = state.active_project.read().await.clone();

    // Intent.lisp is the SOLE source of truth for architecture graph
    let intent_candidates = [
        project_root.join(".missiond").join("intent.lisp"),
        project_root.join(".jarvis").join("intent.lisp"),
        project_root.join("intent.lisp"),
    ];
    // Legacy fallback for Jarvis (has jarvis-topology.sexp at root)
    let legacy_candidates = [
        project_root.join("jarvis-topology.sexp"),
    ];

    let intent_path = intent_candidates.iter().find(|p| p.exists());
    let legacy_path = legacy_candidates.iter().find(|p| p.exists());

    let topology_content = if let Some(p) = intent_path {
        match tokio::fs::read_to_string(p).await {
            Ok(c) => c,
            Err(e) => return Json(json!({
                "error": format!("cannot read intent.lisp: {e}"),
                "nodes": [], "edges": [], "violations": [], "traces": [],
            })),
        }
    } else if let Some(p) = legacy_path {
        match tokio::fs::read_to_string(p).await {
            Ok(c) => c,
            Err(e) => return Json(json!({
                "error": format!("cannot read topology: {e}"),
                "nodes": [], "edges": [], "violations": [], "traces": [],
            })),
        }
    } else {
        return Json(json!({
            "error": "Project unmapped — no intent.lisp found. Run 'Elevate' to create one.",
            "nodes": [], "edges": [], "violations": [], "traces": [],
        }));
    };

    // Real-time AST scan in memory (no PG) for symbol enrichment
    let language = forge_core::scanner::detect_language(&project_root);
    let ast_nodes = forge_core::scanner::auto_scan(&project_root, &language).unwrap_or_default();

    let graph = forge_core::code_graph::build_architecture_graph(&topology_content, &ast_nodes);
    Json(mapped_graph_to_json(&graph))
}

/// GET /api/code-map/dual-graph — intent vs reality comparison.
/// Reads jarvis-topology.sexp (reality) + intent.lisp (intent),
/// builds simplified pillar/component graphs, extracts flows + constraints.
async fn get_code_map_dual_graph(State(state): State<Arc<AppState>>) -> Json<Value> {
    // 1. Read reality (prefer .jarvis/reality.sexp, fallback to jarvis-topology.sexp)
    let project_root = state.active_project.read().await.clone();
    let jarvis_reality = project_root.join(".jarvis").join("reality.sexp");
    let legacy_reality = project_root.join("jarvis-topology.sexp");
    let reality_path = if jarvis_reality.exists() { jarvis_reality } else { legacy_reality };
    let reality_text = tokio::fs::read_to_string(&reality_path).await.unwrap_or_default();
    let reality_nodes = SexpNode::parse(&reality_text).unwrap_or_default();
    let reality_graph = sexp_to_simple_graph(reality_nodes.first());

    // 2. Read intent (.missiond/intent.lisp or .jarvis/intent.lisp or root intent.lisp)
    let missiond_intent = project_root.join(".missiond").join("intent.lisp");
    let jarvis_intent = project_root.join(".jarvis").join("intent.lisp");
    let legacy_intent = project_root.join("intent.lisp");
    let intent_path = if missiond_intent.exists() { missiond_intent }
                      else if jarvis_intent.exists() { jarvis_intent }
                      else { legacy_intent };
    let intent_text = tokio::fs::read_to_string(&intent_path).await.unwrap_or_default();
    let intent_nodes = SexpNode::parse(&intent_text).unwrap_or_default();
    let intent_graph = if intent_text.is_empty() {
        Value::Null
    } else {
        sexp_to_simple_graph(intent_nodes.first())
    };

    // 3. Extract flows from intent
    let flows = if let Some(intent_root) = intent_nodes.first() {
        if let Some(flows_node) = intent_root.find("flows") {
            flows_node.find_all("flow").iter().map(|f| {
                let name = f.id().unwrap_or("").to_string();
                let description = f.str_val("description").unwrap_or("").to_string();
                let steps: Vec<String> = if let Some(sn) = f.find("steps") {
                    sn.children().iter().filter_map(|child| {
                        if let SexpNode::Atom(s) = child {
                            if s != "→" && s != "steps" { Some(s.clone()) } else { None }
                        } else { None }
                    }).collect()
                } else {
                    Vec::new()
                };
                json!({ "name": name, "description": description, "steps": steps })
            }).collect::<Vec<_>>()
        } else { Vec::new() }
    } else { Vec::new() };

    // 4. Extract design constraints from intent
    let constraints = if let Some(intent_root) = intent_nodes.first() {
        if let Some(dc) = intent_root.find("design-constraints") {
            let isolation = dc.find("strict-isolation")
                .map(|si| {
                    si.children().iter().skip(1).filter_map(|c| {
                        if let SexpNode::List(inner) = c {
                            Some(inner.iter().filter_map(|a| a.as_atom().map(String::from)).collect::<Vec<_>>())
                        } else if let SexpNode::Atom(s) = c {
                            if s != "strict-isolation" { Some(vec![s.clone()]) } else { None }
                        } else { None }
                    }).flatten().collect::<Vec<String>>()
                })
                .unwrap_or_default();
            let comm = dc.find("communication")
                .and_then(|c| c.find("must-use"))
                .and_then(|mu| mu.id())
                .unwrap_or("");
            let db = dc.find("db-access")
                .and_then(|c| c.find("only"))
                .and_then(|o| o.id())
                .unwrap_or("");
            json!({ "strictIsolation": isolation, "communication": comm, "dbAccess": db })
        } else { Value::Null }
    } else { Value::Null };

    // 5. Delta summary (simplified — count component differences)
    let (gaps, drifts) = count_simple_deltas(&intent_graph, &reality_graph);

    Json(json!({
        "intentGraph": intent_graph,
        "realityGraph": reality_graph,
        "deltas": [],
        "summary": {
            "implementation_gaps": gaps,
            "architectural_drifts": drifts,
            "location_mismatches": 0,
        },
        "flows": flows,
        "designConstraints": constraints,
    }))
}

// ── Dual-graph helpers ────────────────────────────────────────────────────

/// Extract simplified pillar/component graph from S-EXP tree.
fn sexp_to_simple_graph(root: Option<&SexpNode>) -> Value {
    let root = match root {
        Some(r) => r,
        None => return json!({"pillars": [], "orphans": []}),
    };

    let mut pillars = Vec::new();

    for pillar_node in root.find_all("pillar") {
        let pillar_id = match pillar_node.id() {
            Some(id) => id.to_string(),
            None => continue,
        };

        let mut components = Vec::new();

        // Format 1: direct (component Y ...) children
        for comp_node in pillar_node.find_all("component") {
            if let Some(comp_id) = comp_node.id() {
                components.push(extract_component_json(comp_node, comp_id));
            }
        }

        // Format 2: nested inside (components (Y ...) (Z ...))
        if let Some(comps_section) = pillar_node.find("components") {
            for child in comps_section.children() {
                if let Some(comp_id) = child.tag() {
                    if comp_id != "components" {
                        components.push(extract_component_json(child, comp_id));
                    }
                }
            }
        }

        pillars.push(json!({ "id": pillar_id, "components": components }));
    }

    json!({ "pillars": pillars, "orphans": [] })
}

/// Extract JSON metadata from a single component S-EXP node.
fn extract_component_json(node: &SexpNode, comp_id: &str) -> Value {
    let role = node.str_val("role");
    let files: Vec<&str> = node.list_val("files");
    let depends_on: Vec<&str> = node.list_val("depends-on");
    let symbol_count = node.find("symbols").map(|s| s.children().len().saturating_sub(1)).unwrap_or(0);
    let invariants = node.str_val("invariants").unwrap_or("");
    let data_flow = node.str_val("data-flow").unwrap_or("");

    json!({
        "id": comp_id,
        "role": role,
        "symbolCount": symbol_count,
        "files": files,
        "dependsOn": depends_on,
        "invariants": invariants,
        "dataFlow": data_flow,
    })
}

/// Simple delta counting: components in intent but not in reality = gaps,
/// components in reality but not in intent = drifts.
fn count_simple_deltas(intent: &Value, reality: &Value) -> (usize, usize) {
    let intent_comps = extract_all_component_ids(intent);
    let reality_comps = extract_all_component_ids(reality);

    let gaps = intent_comps.iter().filter(|c| !reality_comps.contains(c)).count();
    let drifts = reality_comps.iter().filter(|c| !intent_comps.contains(c)).count();
    (gaps, drifts)
}

fn extract_all_component_ids(graph: &Value) -> Vec<String> {
    let mut ids = Vec::new();
    if let Some(pillars) = graph.get("pillars").and_then(|p| p.as_array()) {
        for pillar in pillars {
            if let Some(comps) = pillar.get("components").and_then(|c| c.as_array()) {
                for comp in comps {
                    if let Some(id) = comp.get("id").and_then(|i| i.as_str()) {
                        ids.push(id.to_string());
                    }
                }
            }
        }
    }
    ids
}

// ── Multi-project management ──────────────────────────────────────────────

/// List all registered projects.
async fn list_projects(State(state): State<Arc<AppState>>) -> Json<Value> {
    let rows: Vec<(String, String, String, String, Option<String>)> = sqlx::query_as(
        "SELECT name, root_path, COALESCE(language, 'unknown'), COALESCE(status, 'unmapped'), color FROM projects ORDER BY sort_order, name"
    )
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    let active = state.active_project.read().await.to_string_lossy().to_string();

    let projects: Vec<Value> = rows.iter().map(|(name, root, lang, status, color)| {
        // Stat intent.lisp to get last-modified time
        let root_path = std::path::Path::new(root.as_str());
        let intent_candidates = [
            root_path.join(".missiond/intent.lisp"),
            root_path.join(".jarvis/intent.lisp"),
            root_path.join("intent.lisp"),
        ];
        let intent_updated_at = intent_candidates
            .iter()
            .filter_map(|p| std::fs::metadata(p).ok())
            .filter_map(|m| m.modified().ok())
            .next()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs());

        let intent_lines = intent_candidates
            .iter()
            .find(|p| p.exists())
            .and_then(|p| std::fs::read_to_string(p).ok())
            .map(|content| content.lines().count());

        json!({
            "name": name,
            "project_root": root,
            "language": lang,
            "status": status,
            "active": root == &active,
            "color": color,
            "intent_updated_at": intent_updated_at,
            "intent_lines": intent_lines,
        })
    }).collect();

    Json(json!({ "projects": projects }))
}

/// Reorder projects. Body: { "order": ["name-a", "name-b", ...] }
async fn reorder_projects(
    State(state): State<Arc<AppState>>,
    Json(body): Json<Value>,
) -> Json<Value> {
    let order = match body.get("order").and_then(|v| v.as_array()) {
        Some(arr) => arr
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect::<Vec<_>>(),
        None => return Json(json!({ "error": "order array is required" })),
    };

    for (idx, name) in order.iter().enumerate() {
        if let Err(e) = sqlx::query("UPDATE projects SET sort_order = $1 WHERE name = $2")
            .bind(idx as i32)
            .bind(name)
            .execute(&state.db)
            .await
        {
            return Json(json!({ "error": e.to_string() }));
        }
    }

    Json(json!({ "ok": true }))
}

/// Register a new project. Body: { "name": "...", "project_root": "/path/to/..." }
async fn register_project(
    State(state): State<Arc<AppState>>,
    Json(body): Json<Value>,
) -> Json<Value> {
    let name = body.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let root = body.get("project_root").and_then(|v| v.as_str()).unwrap_or("");

    if name.is_empty() || root.is_empty() {
        return Json(json!({ "error": "name and project_root are required" }));
    }

    let root_path = std::path::Path::new(root);
    if !root_path.exists() {
        return Json(json!({ "error": format!("path does not exist: {root}") }));
    }

    // Auto-detect language
    let language = forge_core::scanner::detect_language(root_path);

    // Determine initial status (check if .missiond/ or .jarvis/ artifacts already exist)
    let missiond_dir = root_path.join(".missiond");
    let jarvis_dir = root_path.join(".jarvis");
    let has_intent = missiond_dir.join("intent.lisp").exists()
        || jarvis_dir.join("intent.lisp").exists()
        || root_path.join("intent.lisp").exists();
    let has_reality = jarvis_dir.join("reality.sexp").exists()
        || root_path.join("jarvis-topology.sexp").exists();

    let status = if has_intent { "mapped" } else if has_reality { "scanned" } else { "unmapped" };

    match sqlx::query(
        "INSERT INTO projects (name, root_path, language, status) VALUES ($1, $2, $3, $4)
         ON CONFLICT (name) DO UPDATE SET root_path = $2, language = $3, status = $4"
    )
    .bind(name)
    .bind(root)
    .bind(&language)
    .bind(status)
    .execute(&state.db)
    .await {
        Ok(_) => Json(json!({ "ok": true, "name": name, "project_root": root, "language": language, "status": status })),
        Err(e) => Json(json!({ "error": e.to_string() })),
    }
}

/// Scan a project: run tree-sitter AST extraction → generate .jarvis/reality.sexp
async fn scan_project(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Json<Value> {
    // Look up project
    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT root_path, language FROM projects WHERE name = $1"
    )
    .bind(&name)
    .fetch_optional(&state.db)
    .await
    .unwrap_or(None);

    let (root, language) = match row {
        Some(r) => r,
        None => return Json(json!({ "error": format!("project not found: {name}") })),
    };

    let project_root = std::path::Path::new(&root);

    // Run AST scanner
    let nodes = match forge_core::scanner::auto_scan(project_root, &language) {
        Ok(n) => n,
        Err(e) => return Json(json!({ "error": format!("scan failed: {e}") })),
    };

    let node_count = nodes.len();

    // Generate reality.sexp and write to .jarvis/
    let existing_topology = {
        // Check for existing topology file (jarvis-topology.sexp at root or .jarvis/reality.sexp)
        let topo_path = project_root.join("jarvis-topology.sexp");
        if topo_path.exists() {
            std::fs::read_to_string(&topo_path).ok()
        } else {
            let jarvis_topo = project_root.join(".jarvis").join("reality.sexp");
            if jarvis_topo.exists() {
                std::fs::read_to_string(&jarvis_topo).ok()
            } else {
                None
            }
        }
    };

    // AST nodes are NOT stored in PG — Forge is stateless.
    // Code Map renders from intent.lisp + real-time in-memory scan.

    // ── Write reality.sexp to disk (the only persistent output) ──
    match forge_core::reality_mirror::write_to_disk(
        project_root,
        &nodes,
        &name,
        existing_topology.as_deref(),
    ) {
        Ok(path) => {
            // Update status to scanned
            let _ = sqlx::query("UPDATE projects SET status = 'scanned' WHERE name = $1 AND status = 'unmapped'")
                .bind(&name)
                .execute(&state.db)
                .await;

            info!(project = %name, nodes = node_count, "Scan complete");
            Json(json!({
                "ok": true,
                "project": name,
                "nodes_found": node_count,
                "reality_path": path.to_string_lossy(),
            }))
        }
        Err(e) => Json(json!({ "error": format!("failed to write reality.sexp: {e}") })),
    }
}

/// Read a project's reality.sexp
async fn get_project_reality(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> axum::response::Response {
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT root_path FROM projects WHERE name = $1"
    )
    .bind(&name)
    .fetch_optional(&state.db)
    .await
    .unwrap_or(None);

    let root = match row {
        Some((r,)) => r,
        None => return axum::response::Response::builder()
            .status(404)
            .body(axum::body::Body::from("project not found"))
            .unwrap(),
    };

    // Try .jarvis/reality.sexp first, then jarvis-topology.sexp at root
    let reality_path = std::path::Path::new(&root).join(".jarvis").join("reality.sexp");
    let fallback_path = std::path::Path::new(&root).join("jarvis-topology.sexp");

    let path = if reality_path.exists() { reality_path } else { fallback_path };

    match tokio::fs::read_to_string(&path).await {
        Ok(content) => axum::response::Response::builder()
            .header("content-type", "text/plain; charset=utf-8")
            .body(axum::body::Body::from(content))
            .unwrap(),
        Err(_) => axum::response::Response::builder()
            .status(404)
            .body(axum::body::Body::from("reality.sexp not found — run scan first"))
            .unwrap(),
    }
}

/// Write intent.lisp for a project (called by Claude Code after elevation)
async fn write_project_intent(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    body: String,
) -> Json<Value> {
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT root_path FROM projects WHERE name = $1"
    )
    .bind(&name)
    .fetch_optional(&state.db)
    .await
    .unwrap_or(None);

    let root = match row {
        Some((r,)) => r,
        None => return Json(json!({ "error": format!("project not found: {name}") })),
    };

    let jarvis_dir = std::path::Path::new(&root).join(".jarvis");
    if let Err(e) = tokio::fs::create_dir_all(&jarvis_dir).await {
        return Json(json!({ "error": format!("failed to create .jarvis/: {e}") }));
    }

    let intent_path = jarvis_dir.join("intent.lisp");
    match tokio::fs::write(&intent_path, &body).await {
        Ok(_) => {
            // Auto-update status to mapped
            let _ = sqlx::query("UPDATE projects SET status = 'mapped' WHERE name = $1")
                .bind(&name)
                .execute(&state.db)
                .await;

            // Git-native version index: record commit hash + timestamp (not lisp text)
            let git_hash = std::process::Command::new("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(&root)
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .map(|s| s.trim().to_string())
                .unwrap_or_default();

            let _ = sqlx::query(
                "INSERT INTO intent_snapshots (project_name, git_hash, summary)
                 VALUES ($1, $2, $3)"
            )
            .bind(&name)
            .bind(&git_hash)
            .bind(format!("Intent written ({} lines)", body.lines().count()))
            .execute(&state.db)
            .await;

            info!(project = %name, lines = body.lines().count(), git = %git_hash, "Intent written");
            Json(json!({ "ok": true, "path": intent_path.to_string_lossy(), "lines": body.lines().count(), "git_hash": git_hash }))
        }
        Err(e) => Json(json!({ "error": format!("failed to write intent.lisp: {e}") })),
    }
}

/// Switch the active project. The Code Map will now show this project's topology.
async fn activate_project(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Json<Value> {
    // Look up project root from DB
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT root_path FROM projects WHERE name = $1"
    )
    .bind(&name)
    .fetch_optional(&state.db)
    .await
    .unwrap_or(None);

    match row {
        Some((root,)) => {
            let path = PathBuf::from(&root);
            if !path.exists() {
                return Json(json!({ "error": format!("project root no longer exists: {root}") }));
            }
            *state.active_project.write().await = path;
            info!(project = %name, root = %root, "Switched active project");
            Json(json!({ "ok": true, "name": name, "project_root": root }))
        }
        None => Json(json!({ "error": format!("project not found: {name}") })),
    }
}

// ── Coverage metrics ─────────────────────────────────────────────────────

/// GET /api/projects/{name}/coverage — count Forge-generated vs total source files.
async fn get_project_coverage(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Json<Value> {
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT root_path FROM projects WHERE name = $1"
    )
    .bind(&name)
    .fetch_optional(&state.db)
    .await
    .unwrap_or(None);

    let root = match row {
        Some((r,)) => r,
        None => return Json(json!({ "error": format!("project not found: {name}") })),
    };

    let project_root = std::path::Path::new(&root);

    // Source file extensions we care about
    let source_exts: &[&str] = &["rs", "ts", "tsx", "py", "vue", "js", "jsx"];
    let forge_markers: &[&str] = &["GENERATED BY FORGE", "\u{1F916} GENERATED BY FORGE"];

    let mut total_by_ext: HashMap<String, u64> = HashMap::new();
    let mut generated_by_ext: HashMap<String, u64> = HashMap::new();
    let mut total_files: u64 = 0;
    let mut forge_generated: u64 = 0;

    let walker = ignore::WalkBuilder::new(project_root)
        .hidden(true)
        .git_ignore(true)
        .git_global(false)
        .git_exclude(false)
        .filter_entry(|entry| {
            let name = entry.file_name().to_string_lossy();
            !matches!(name.as_ref(), "target" | "node_modules" | ".git" | ".next" | "dist" | "build")
        })
        .build();

    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let ext = match path.extension().and_then(|e| e.to_str()) {
            Some(e) => e,
            None => continue,
        };

        if !source_exts.contains(&ext) {
            continue;
        }

        let ext_key = format!(".{ext}");
        total_files += 1;
        *total_by_ext.entry(ext_key.clone()).or_insert(0) += 1;

        // Check file content for Forge generation markers
        if let Ok(content) = std::fs::read_to_string(path) {
            // Only check first 2KB for performance
            let end = content.char_indices().take_while(|(i, _)| *i < 2048).last().map(|(i, c)| i + c.len_utf8()).unwrap_or(content.len());
            let header: &str = &content[..end];
            if forge_markers.iter().any(|m| header.contains(m)) {
                forge_generated += 1;
                *generated_by_ext.entry(ext_key).or_insert(0) += 1;
            }
        }
    }

    let coverage_percent = if total_files > 0 {
        (forge_generated as f64 / total_files as f64 * 1000.0).round() / 10.0
    } else {
        0.0
    };

    // Build by_extension map
    let mut by_extension = serde_json::Map::new();
    for ext in source_exts {
        let key = format!(".{ext}");
        let total = total_by_ext.get(&key).copied().unwrap_or(0);
        let generated = generated_by_ext.get(&key).copied().unwrap_or(0);
        if total > 0 {
            by_extension.insert(key, json!({ "generated": generated, "total": total }));
        }
    }

    Json(json!({
        "project": name,
        "forge_generated": forge_generated,
        "total_files": total_files,
        "coverage_percent": coverage_percent,
        "by_extension": by_extension,
    }))
}

// ── Project color label ─────────────────────────────────────────────────

/// PUT /api/projects/{name}/color — set or clear a color label.
/// Body: { "color": "red" } or { "color": null }
async fn set_project_color(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(body): Json<Value>,
) -> Json<Value> {
    let color = body.get("color").and_then(|v| v.as_str()).map(|s| s.to_string());

    match sqlx::query("UPDATE projects SET color = $1 WHERE name = $2")
        .bind(&color)
        .bind(&name)
        .execute(&state.db)
        .await
    {
        Ok(r) if r.rows_affected() > 0 => Json(json!({ "ok": true })),
        Ok(_) => Json(json!({ "error": format!("project not found: {name}") })),
        Err(e) => Json(json!({ "error": e.to_string() })),
    }
}

// ── Open in editor ──────────────────────────────────────────────────────

/// POST /api/projects/{name}/open-editor — open project in Cursor (new window).
async fn open_project_in_editor(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Json<Value> {
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT root_path FROM projects WHERE name = $1"
    )
    .bind(&name)
    .fetch_optional(&state.db)
    .await
    .unwrap_or(None);

    let root = match row {
        Some((r,)) => r,
        None => return Json(json!({ "error": format!("project not found: {name}") })),
    };

    match std::process::Command::new("cursor")
        .arg("--new-window")
        .arg(&root)
        .spawn()
    {
        Ok(_) => {
            info!(project = %name, root = %root, "Opened in Cursor");
            Json(json!({ "ok": true }))
        }
        Err(e) => Json(json!({ "error": format!("failed to launch cursor: {e}") })),
    }
}

// ── Universe Graph ──────────────────────────────────────────────────────

/// GET /api/universe — parse universe.intent.lisp and return the full topology.
async fn get_universe() -> Json<Value> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    let manifest_path = PathBuf::from(&home).join("Projects").join("universe.intent.lisp");

    if !manifest_path.exists() {
        return Json(json!({
            "error": format!("universe.intent.lisp not found at {}", manifest_path.display())
        }));
    }

    // Read raw content for the source viewer
    let source = std::fs::read_to_string(&manifest_path).unwrap_or_default();

    // Parse the full graph
    let graph = match forge_core::universe_graph::resolve_universe_graph(&manifest_path) {
        Ok(g) => g,
        Err(errors) => {
            return Json(json!({
                "source": source,
                "services": [],
                "edges": [],
                "errors": errors,
            }));
        }
    };

    // Build services array
    let services: Vec<Value> = graph.services.iter().map(|svc| {
        let svc_root = manifest_path.parent().unwrap().join(&svc.path);
        let has_intent = [".missiond/intent.lisp", ".jarvis/intent.lisp", "intent.lisp"]
            .iter()
            .any(|p| svc_root.join(p).exists());

        json!({
            "id": svc.id,
            "path": svc.path.to_string_lossy(),
            "has_intent": has_intent,
        })
    }).collect();

    // Build edges array (consumer → provider)
    let mut edges: Vec<Value> = Vec::new();
    for (consumer_id, deps) in &graph.dependencies {
        for dep in deps {
            edges.push(json!({
                "from": consumer_id,
                "to": dep.provider_id,
                "consumes": dep.consumes,
                "breaks_if": dep.breaks_if,
            }));
        }
    }

    Json(json!({
        "source": source,
        "services": services,
        "edges": edges,
    }))
}

// AST data is NOT stored in PG. Forge is stateless — real-time scan in memory.
