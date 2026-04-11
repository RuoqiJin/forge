//! Domain Engine IR — typed representation of `(domain-engine ...)` declarations.
//!
//! The "domain engine shell" pattern:
//! - Declares dependency injection parameters
//! - Declares internal state fields
//! - Declares the public interface (exported methods)
//!
//! Forge generates ONLY:
//!   - Struct skeleton with all dependencies as fields
//!   - `new()` constructor with DI parameters
//!   - `pub trait EngineLogic` with exported method signatures
//!
//! Forge NEVER generates internal business logic.
//! The "brain" lives 100% in custom.rs.

use nc_parser::SexpNode;
use std::fmt;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct IrError {
    pub field: String,
    pub message: String,
    pub expected: Vec<String>,
    pub got: String,
}

impl fmt::Display for IrError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "DomainEngine IR error in `{}`: {} (expected {:?}, got {:?})",
            self.field, self.message, self.expected, self.got
        )
    }
}

impl std::error::Error for IrError {}

// ---------------------------------------------------------------------------
// Declaration types
// ---------------------------------------------------------------------------

/// A dependency injected into the engine constructor.
#[derive(Debug, Clone)]
pub struct DepDecl {
    pub name: String,
    pub rust_type: String,
}

/// An internal state field managed by the engine.
#[derive(Debug, Clone)]
pub struct StateFieldDecl {
    pub name: String,
    pub rust_type: String,
    pub default: Option<String>,
}

/// An exported method in the engine's public interface.
#[derive(Debug, Clone)]
pub struct ExportDecl {
    pub name: String,
    pub params: Vec<(String, String)>, // (name, type)
    pub returns: String,
    pub is_async: bool,
    pub doc: Option<String>,
}

/// Top-level domain engine declaration.
#[derive(Debug, Clone)]
pub struct DomainEngineDecl {
    pub name: String,
    pub struct_name: String,
    pub target: Option<String>,
    pub deps: Vec<DepDecl>,
    pub state: Vec<StateFieldDecl>,
    pub exports: Vec<ExportDecl>,
}

// ---------------------------------------------------------------------------
// S-EXP → IR lowering
// ---------------------------------------------------------------------------

impl DomainEngineDecl {
    pub fn from_sexp(node: &SexpNode) -> Result<Self, IrError> {
        let children = expect_list(node)?;
        if children.len() < 2 {
            return Err(err("domain-engine", "missing engine name", &["name"], ""));
        }

        expect_tag(node, "domain-engine")?;
        let name = expect_atom(&children[1])?.to_string();
        let struct_name = node
            .keyword(":struct")
            .map(String::from)
            .unwrap_or_else(|| to_pascal_case(&name));
        let target = node.keyword(":target").map(String::from);

        let deps = parse_deps(node);
        let state = parse_state(node);
        let exports = parse_exports(node)?;

        Ok(DomainEngineDecl {
            name,
            struct_name,
            target,
            deps,
            state,
            exports,
        })
    }
}

/// Parse all `(domain-engine ...)` declarations from a top-level S-EXP list.
pub fn parse_domain_engines(nodes: &[SexpNode]) -> Result<Vec<DomainEngineDecl>, IrError> {
    nodes
        .iter()
        .filter(|n| n.tag() == Some("domain-engine"))
        .map(DomainEngineDecl::from_sexp)
        .collect()
}

// ---------------------------------------------------------------------------
// Sub-parsers
// ---------------------------------------------------------------------------

fn parse_deps(parent: &SexpNode) -> Vec<DepDecl> {
    let children = match parent {
        SexpNode::List(c) => c,
        _ => return vec![],
    };
    for child in children {
        if child.tag() == Some("depends-on") {
            if let SexpNode::List(items) = child {
                return items
                    .iter()
                    .skip(1)
                    .filter_map(|item| {
                        if let SexpNode::List(parts) = item {
                            if parts.len() >= 2 {
                                let name = expect_atom(&parts[0]).ok()?.to_string();
                                let rust_type = expect_atom(&parts[1]).ok()?.to_string();
                                return Some(DepDecl { name, rust_type });
                            }
                        }
                        None
                    })
                    .collect();
            }
        }
    }
    vec![]
}

fn parse_state(parent: &SexpNode) -> Vec<StateFieldDecl> {
    let children = match parent {
        SexpNode::List(c) => c,
        _ => return vec![],
    };
    for child in children {
        if child.tag() == Some("state") {
            if let SexpNode::List(items) = child {
                return items
                    .iter()
                    .skip(1)
                    .filter_map(|item| {
                        if let SexpNode::List(parts) = item {
                            if parts.len() >= 2 {
                                let name = expect_atom(&parts[0]).ok()?.to_string();
                                let rust_type = expect_atom(&parts[1]).ok()?.to_string();
                                let default = item.keyword(":default").map(String::from);
                                return Some(StateFieldDecl {
                                    name,
                                    rust_type,
                                    default,
                                });
                            }
                        }
                        None
                    })
                    .collect();
            }
        }
    }
    vec![]
}

fn parse_exports(parent: &SexpNode) -> Result<Vec<ExportDecl>, IrError> {
    let children = match parent {
        SexpNode::List(c) => c,
        _ => return Ok(vec![]),
    };
    let mut exports = Vec::new();
    for child in children {
        if child.tag() == Some("exports") {
            if let SexpNode::List(items) = child {
                for item in items.iter().skip(1) {
                    if item.tag() == Some("fn") {
                        exports.push(parse_one_export(item)?);
                    }
                }
            }
        }
    }
    Ok(exports)
}

fn parse_one_export(node: &SexpNode) -> Result<ExportDecl, IrError> {
    let children = expect_list(node)?;
    if children.len() < 2 {
        return Err(err("fn", "missing function name", &["fn_name"], ""));
    }

    let name = expect_atom(&children[1])?.to_string();
    let returns = node
        .keyword(":returns")
        .map(String::from)
        .unwrap_or_else(|| "()".to_string());
    let is_async = node.keyword(":async").map(|v| v != "false").unwrap_or(true);
    let doc = node.keyword(":doc").map(String::from);
    let params = parse_params(node);

    Ok(ExportDecl {
        name,
        params,
        returns,
        is_async,
        doc,
    })
}

fn parse_params(parent: &SexpNode) -> Vec<(String, String)> {
    let node = match parent.keyword_node(":params") {
        Some(n) => n,
        None => return vec![],
    };
    match node {
        SexpNode::List(items) => items
            .iter()
            .filter_map(|item| {
                if let SexpNode::List(parts) = item {
                    if parts.len() >= 2 {
                        let name = expect_atom(&parts[0]).ok()?.to_string();
                        let rust_type = expect_atom(&parts[1]).ok()?.to_string();
                        return Some((name, rust_type));
                    }
                }
                None
            })
            .collect(),
        _ => vec![],
    }
}

// ---------------------------------------------------------------------------
// Utilities
// ---------------------------------------------------------------------------

fn to_pascal_case(s: &str) -> String {
    s.split(|c: char| c == '-' || c == '_')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(c) => {
                    let upper: String = c.to_uppercase().collect();
                    upper + chars.as_str()
                }
                None => String::new(),
            }
        })
        .collect()
}

fn expect_tag<'a>(node: &'a SexpNode, expected: &str) -> Result<&'a str, IrError> {
    let tag = node
        .tag()
        .ok_or_else(|| err("node", "expected a list", &[expected], "atom"))?;
    if tag != expected {
        return Err(err("tag", "wrong tag", &[expected], tag));
    }
    Ok(tag)
}

fn expect_list(node: &SexpNode) -> Result<&Vec<SexpNode>, IrError> {
    match node {
        SexpNode::List(children) => Ok(children),
        SexpNode::Atom(s) => Err(err("node", "expected list", &["(...)"], s)),
    }
}

fn expect_atom<'a>(node: &'a SexpNode) -> Result<&'a str, IrError> {
    match node {
        SexpNode::Atom(s) => Ok(s),
        SexpNode::List(_) => Err(err("node", "expected atom", &["atom"], "(…)")),
    }
}

fn err(field: &str, msg: &str, expected: &[&str], got: &str) -> IrError {
    IrError {
        field: field.into(),
        message: msg.into(),
        expected: expected.iter().map(|s| s.to_string()).collect(),
        got: got.into(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(input: &str) -> SexpNode {
        SexpNode::parse_one(input).expect("valid S-EXP")
    }

    #[test]
    fn parse_full_engine() {
        let sexp = parse(
            r#"(domain-engine slot-orchestrator
              :struct SlotOrchestrator
              :target "missiond-daemon/src/slot_orchestrator/gen_engine.rs"
              (depends-on
                (db "DbExecutor")
                (pty_manager "Arc<PTYManager>")
                (event_bus "EventBus")
                (slot_config "Arc<SlotsConfig>"))
              (state
                (agents "HashMap<String, AgentHandle>" :default "HashMap::new()")
                (task_queue "VecDeque<SlotTask>" :default "VecDeque::new()"))
              (exports
                (fn spawn_agent
                  :params ((slot_id "&str") (engine CliEngine))
                  :returns "Result<AgentHandle>"
                  :doc "Spawn a new agent process for the given slot")
                (fn kill_agent
                  :params ((slot_id "&str"))
                  :returns "Result<()>"
                  :doc "Terminate an agent process")
                (fn dispatch_task
                  :params ((task SlotTask))
                  :returns "Result<()>"
                  :doc "Route a task to the best available slot")
                (fn status
                  :returns "Vec<SlotStatus>"
                  :doc "Get status of all managed slots")))"#,
        );
        let decl = DomainEngineDecl::from_sexp(&sexp).unwrap();

        assert_eq!(decl.name, "slot-orchestrator");
        assert_eq!(decl.struct_name, "SlotOrchestrator");
        assert_eq!(decl.deps.len(), 4);
        assert_eq!(decl.deps[0].name, "db");
        assert_eq!(decl.deps[0].rust_type, "DbExecutor");
        assert_eq!(decl.deps[3].name, "slot_config");

        assert_eq!(decl.state.len(), 2);
        assert_eq!(decl.state[0].name, "agents");
        assert_eq!(decl.state[0].default, Some("HashMap::new()".into()));

        assert_eq!(decl.exports.len(), 4);
        assert_eq!(decl.exports[0].name, "spawn_agent");
        assert_eq!(decl.exports[0].params.len(), 2);
        assert_eq!(decl.exports[0].returns, "Result<AgentHandle>");
        assert!(decl.exports[0].is_async);
        assert_eq!(decl.exports[3].name, "status");
        assert_eq!(decl.exports[3].params.len(), 0);
    }

    #[test]
    fn parse_minimal_engine() {
        let sexp = parse(
            r#"(domain-engine context-pipeline
              (depends-on (db "DbExecutor"))
              (exports
                (fn build_context
                  :params ((slot_id "&str") (budget usize))
                  :returns "Result<String>")))"#,
        );
        let decl = DomainEngineDecl::from_sexp(&sexp).unwrap();
        assert_eq!(decl.name, "context-pipeline");
        assert_eq!(decl.struct_name, "ContextPipeline"); // auto PascalCase
        assert!(decl.target.is_none());
        assert_eq!(decl.deps.len(), 1);
        assert_eq!(decl.state.len(), 0);
        assert_eq!(decl.exports.len(), 1);
    }

    #[test]
    fn parse_engine_no_deps() {
        let sexp = parse(
            r#"(domain-engine simple-processor
              (exports
                (fn process :returns "Result<()>")))"#,
        );
        let decl = DomainEngineDecl::from_sexp(&sexp).unwrap();
        assert_eq!(decl.deps.len(), 0);
        assert_eq!(decl.exports.len(), 1);
    }

    #[test]
    fn struct_name_auto_from_kebab() {
        let sexp = parse(
            r#"(domain-engine llm-gateway
              (exports (fn route :returns "Result<Value>")))"#,
        );
        let decl = DomainEngineDecl::from_sexp(&sexp).unwrap();
        assert_eq!(decl.struct_name, "LlmGateway");
    }

    #[test]
    fn sync_export() {
        let sexp = parse(
            r#"(domain-engine sync-engine
              (exports
                (fn get_name :async false :returns "&str")))"#,
        );
        let decl = DomainEngineDecl::from_sexp(&sexp).unwrap();
        assert!(!decl.exports[0].is_async);
    }
}
