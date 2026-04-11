//! Bootstrap IR — typed representation of `(bootstrap ...)` declarations.
//!
//! Describes the dependency graph for daemon initialization:
//! infra components → MCP modules → workers.

use nc_parser::SexpNode;
use std::fmt;

#[derive(Debug, Clone)]
pub struct IrError {
    pub field: String,
    pub message: String,
    pub expected: Vec<String>,
    pub got: String,
}

impl fmt::Display for IrError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Bootstrap IR error in `{}`: {}", self.field, self.message)
    }
}

impl std::error::Error for IrError {}

// ---------------------------------------------------------------------------
// Declarations
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ComponentDecl {
    pub name: String,
    pub rust_type: String,
    pub wrap: Option<String>,
    pub deps: Vec<String>,
    pub init_expr: Option<String>,
    pub post_init: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ModuleDecl {
    pub name: String,
    pub struct_name: String,
    pub deps: Vec<String>,
    pub deps_raw: Option<String>,
}

#[derive(Debug, Clone)]
pub struct WorkerDecl {
    pub name: String,
    pub struct_name: String,
    pub module_path: Option<String>,
    pub init_expr: Option<String>,
    pub method: String,
    pub slot: Option<String>,
}

#[derive(Debug, Clone)]
pub struct BootstrapDecl {
    pub name: String,
    pub target: String,
    pub infra: Vec<ComponentDecl>,
    pub modules: Vec<ModuleDecl>,
    pub workers: Vec<WorkerDecl>,
}

// ---------------------------------------------------------------------------
// S-EXP → IR
// ---------------------------------------------------------------------------

impl BootstrapDecl {
    pub fn from_sexp(node: &SexpNode) -> Result<Self, IrError> {
        let children = expect_list(node)?;
        expect_tag(node, "bootstrap")?;
        if children.len() < 2 {
            return Err(err("bootstrap", "missing name"));
        }
        let name = expect_atom(&children[1])?.to_string();
        let target = node.keyword(":target").unwrap_or("").to_string();

        let mut infra = Vec::new();
        let mut modules = Vec::new();
        let mut workers = Vec::new();

        for child in children.iter().skip(2) {
            match child.tag() {
                Some("infra") => infra = parse_infra(child)?,
                Some("modules") => modules = parse_modules(child)?,
                Some("workers") => workers = parse_workers(child)?,
                _ => {}
            }
        }

        // Topological sort validation: check all deps reference declared components
        let names: Vec<&str> = infra.iter().map(|c| c.name.as_str()).collect();
        for comp in &infra {
            for dep in &comp.deps {
                if !names.contains(&dep.as_str()) {
                    return Err(IrError {
                        field: format!("component {}", comp.name),
                        message: format!("unknown dependency: {dep}"),
                        expected: names.iter().map(|s| s.to_string()).collect(),
                        got: dep.clone(),
                    });
                }
            }
        }

        Ok(BootstrapDecl { name, target, infra, modules, workers })
    }
}

fn parse_infra(node: &SexpNode) -> Result<Vec<ComponentDecl>, IrError> {
    let children = expect_list(node)?;
    let mut comps = Vec::new();
    for child in children.iter().skip(1) {
        if child.tag() == Some("component") {
            comps.push(parse_component(child)?);
        }
    }
    Ok(comps)
}

fn parse_component(node: &SexpNode) -> Result<ComponentDecl, IrError> {
    let children = expect_list(node)?;
    if children.len() < 2 {
        return Err(err("component", "missing name"));
    }
    let name = expect_atom(&children[1])?.to_string();
    let rust_type = node.keyword(":type").unwrap_or("").to_string();
    let wrap = node.keyword(":wrap").map(String::from);
    let init_expr = node.keyword(":init").map(String::from);
    let post_init = node.keyword(":post-init").map(String::from);

    let deps = match node.keyword_node(":deps") {
        Some(n) => collect_atoms(n),
        None => vec![],
    };

    Ok(ComponentDecl { name, rust_type, wrap, deps, init_expr, post_init })
}

fn parse_modules(node: &SexpNode) -> Result<Vec<ModuleDecl>, IrError> {
    let children = expect_list(node)?;
    let mut mods = Vec::new();
    for child in children.iter().skip(1) {
        if child.tag() == Some("module") {
            if let SexpNode::List(parts) = child {
                if parts.len() >= 2 {
                    let name = expect_atom(&parts[1])?.to_string();
                    let struct_name = child.keyword(":struct").unwrap_or(&name).to_string();
                    let deps_raw = child.keyword(":deps-raw").map(String::from);
                    let deps = match child.keyword_node(":deps") {
                        Some(n) => collect_atoms(n),
                        None => vec![],
                    };
                    mods.push(ModuleDecl { name, struct_name, deps, deps_raw });
                }
            }
        }
    }
    Ok(mods)
}

fn parse_workers(node: &SexpNode) -> Result<Vec<WorkerDecl>, IrError> {
    let children = expect_list(node)?;
    let mut workers = Vec::new();
    for child in children.iter().skip(1) {
        if child.tag() == Some("spawn") {
            if let SexpNode::List(parts) = child {
                if parts.len() >= 2 {
                    let name = expect_atom(&parts[1])?.to_string();
                    let struct_name = child.keyword(":struct").unwrap_or("").to_string();
                    let module_path = child.keyword(":path").map(String::from);
                    let init_expr = child.keyword(":init").map(String::from);
                    let method = child.keyword(":method").unwrap_or("run").to_string();
                    let slot = child.keyword(":slot").map(String::from);
                    workers.push(WorkerDecl { name, struct_name, module_path, init_expr, method, slot });
                }
            }
        }
    }
    Ok(workers)
}

// ---------------------------------------------------------------------------
// Utilities
// ---------------------------------------------------------------------------

fn expect_tag<'a>(node: &'a SexpNode, expected: &str) -> Result<&'a str, IrError> {
    let tag = node.tag().ok_or_else(|| err("node", "expected list"))?;
    if tag != expected { return Err(err("tag", &format!("expected {expected}, got {tag}"))); }
    Ok(tag)
}

fn expect_list(node: &SexpNode) -> Result<&Vec<SexpNode>, IrError> {
    match node {
        SexpNode::List(c) => Ok(c),
        SexpNode::Atom(s) => Err(err("node", &format!("expected list, got atom: {s}"))),
    }
}

fn expect_atom<'a>(node: &'a SexpNode) -> Result<&'a str, IrError> {
    match node {
        SexpNode::Atom(s) => Ok(s),
        SexpNode::List(_) => Err(err("node", "expected atom")),
    }
}

fn collect_atoms(node: &SexpNode) -> Vec<String> {
    match node {
        SexpNode::List(items) => items.iter().filter_map(|i| {
            if let SexpNode::Atom(s) = i { if !s.starts_with(':') { return Some(s.clone()); } }
            None
        }).collect(),
        SexpNode::Atom(s) => vec![s.clone()],
    }
}

fn err(field: &str, msg: &str) -> IrError {
    IrError { field: field.into(), message: msg.into(), expected: vec![], got: String::new() }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_full_pattern() {
        let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let path = std::path::Path::new(&manifest)
            .parent().unwrap().parent().unwrap()
            .join("patterns/jarvis-bootstrap.lisp");
        let src = std::fs::read_to_string(&path).unwrap();
        let node = SexpNode::parse_one(&src).unwrap();
        let decl = BootstrapDecl::from_sexp(&node).unwrap();

        assert_eq!(decl.name, "jarvis");
        assert_eq!(decl.infra.len(), 10, "expected 10 infra components");
        assert_eq!(decl.modules.len(), 4, "expected 4 MCP modules");
        assert_eq!(decl.workers.len(), 9, "expected 9 workers");

        // Check dep ordering
        assert_eq!(decl.infra[0].name, "pool");
        assert_eq!(decl.infra[1].name, "storage");
        assert!(decl.infra[1].deps.contains(&"pool".to_string()));
    }

    #[test]
    fn reject_unknown_dep() {
        let sexp = SexpNode::parse_one(r#"(bootstrap test
          (infra
            (component a :type A :deps (nonexistent))))"#).unwrap();
        let err = BootstrapDecl::from_sexp(&sexp).unwrap_err();
        assert!(err.message.contains("unknown dependency"));
    }
}
