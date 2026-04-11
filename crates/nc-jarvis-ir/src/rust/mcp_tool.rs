//! MCP Tool IR — typed representation of `(mcp-module ...)` declarations.
//!
//! Generates: ToolSchema registration + arg Deserialize struct + handle_tool dispatch + trait contract.

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
        write!(f, "MCP IR error in `{}`: {} (expected {:?}, got {:?})",
            self.field, self.message, self.expected, self.got)
    }
}

impl std::error::Error for IrError {}

// ---------------------------------------------------------------------------
// Arg type whitelist
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum ArgType {
    String,
    Integer,
    Number,
    Boolean,
    Array,
    Object,
}

impl ArgType {
    pub fn from_str(s: &str) -> Result<Self, IrError> {
        match s {
            "string" => Ok(Self::String),
            "integer" | "int" => Ok(Self::Integer),
            "number" | "float" => Ok(Self::Number),
            "boolean" | "bool" => Ok(Self::Boolean),
            "array" => Ok(Self::Array),
            "object" => Ok(Self::Object),
            other => Err(IrError {
                field: "arg type".into(),
                message: "unknown arg type".into(),
                expected: vec!["string", "integer", "number", "boolean", "array", "object"]
                    .into_iter().map(String::from).collect(),
                got: other.into(),
            }),
        }
    }

    pub fn json_type(&self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Integer => "integer",
            Self::Number => "number",
            Self::Boolean => "boolean",
            Self::Array => "array",
            Self::Object => "object",
        }
    }

    pub fn rust_extractor(&self) -> &'static str {
        match self {
            Self::String => "as_str()",
            Self::Integer => "as_i64()",
            Self::Number => "as_f64()",
            Self::Boolean => "as_bool()",
            Self::Array => "as_array()",
            Self::Object => "as_object()",
        }
    }
}

// ---------------------------------------------------------------------------
// Declaration types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ArgDecl {
    pub name: String,
    pub arg_type: ArgType,
    pub required: bool,
    pub description: Option<String>,
    pub enum_values: Vec<String>,
    pub default: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ValidationRule {
    pub field: String,
    pub min_len: Option<usize>,
    pub max_len: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct EventEmitDecl {
    pub event_name: String,
    pub condition: Option<String>,
    pub payload: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ToolDecl {
    pub name: String,
    pub description: String,
    pub args: Vec<ArgDecl>,
    pub returns: String,
    pub dispatch_on: Option<String>,
    pub strict_args: bool,
    pub events: Vec<EventEmitDecl>,
    pub validations: Vec<ValidationRule>,
}

#[derive(Debug, Clone)]
pub struct DepDecl {
    pub name: String,
    pub rust_type: String,
}

#[derive(Debug, Clone)]
pub struct McpModuleDecl {
    pub name: String,
    pub struct_name: String,
    pub target: Option<String>,
    pub deps: Vec<DepDecl>,
    pub dep_trait: Option<String>,
    pub tools: Vec<ToolDecl>,
}

// ---------------------------------------------------------------------------
// S-EXP → IR lowering
// ---------------------------------------------------------------------------

impl McpModuleDecl {
    pub fn from_sexp(node: &SexpNode) -> Result<Self, IrError> {
        let children = expect_list(node)?;
        expect_tag(node, "mcp-module")?;

        if children.len() < 2 {
            return Err(err("mcp-module", "missing module name", &["module_name"], ""));
        }
        let name = expect_atom(&children[1])?.to_string();
        let struct_name = keyword_or(node, ":struct", &pascal_case(&name));
        let target = node.keyword(":target").map(String::from);
        let dep_trait = node.keyword(":dep-trait").map(String::from);

        let deps = parse_deps(node);

        let mut tools = Vec::new();
        for child in children.iter().skip(2) {
            if child.tag() == Some("tool") {
                tools.push(parse_tool(child)?);
            }
        }

        Ok(McpModuleDecl { name, struct_name, target, deps, dep_trait, tools })
    }
}

pub fn parse_mcp_modules(nodes: &[SexpNode]) -> Result<Vec<McpModuleDecl>, IrError> {
    nodes.iter()
        .filter(|n| n.tag() == Some("mcp-module"))
        .map(McpModuleDecl::from_sexp)
        .collect()
}

// ---------------------------------------------------------------------------
// Sub-parsers
// ---------------------------------------------------------------------------

fn parse_deps(parent: &SexpNode) -> Vec<DepDecl> {
    let node = match parent.keyword_node(":deps") {
        Some(n) => n,
        None => return vec![],
    };
    match node {
        SexpNode::List(items) => items.iter().filter_map(|item| {
            if let SexpNode::List(parts) = item {
                if parts.len() >= 2 {
                    let name = expect_atom(&parts[0]).ok()?.to_string();
                    let rust_type = expect_atom(&parts[1]).ok()?.to_string();
                    return Some(DepDecl { name, rust_type });
                }
            }
            None
        }).collect(),
        _ => vec![],
    }
}

fn parse_tool(node: &SexpNode) -> Result<ToolDecl, IrError> {
    let children = expect_list(node)?;
    if children.len() < 2 {
        return Err(err("tool", "missing tool name", &["tool_name"], ""));
    }
    let name = expect_atom(&children[1])?.to_string();
    let description = node.keyword(":description").unwrap_or("").to_string();
    let returns = keyword_or(node, ":returns", "Value");
    let dispatch_on = node.keyword(":dispatch-on").map(String::from);
    let strict_args = has_flag(node, ":strict-args");

    let mut args = Vec::new();
    for child in children.iter().skip(2) {
        if child.tag() == Some("input") {
            args = parse_input_args(child)?;
            break;
        }
    }

    let events = parse_events(node);
    let validations = parse_validations(node);

    Ok(ToolDecl { name, description, args, returns, dispatch_on, strict_args, events, validations })
}

fn parse_input_args(node: &SexpNode) -> Result<Vec<ArgDecl>, IrError> {
    let children = expect_list(node)?;
    let mut args = Vec::new();

    for child in children.iter().skip(1) { // skip "input" tag
        if let SexpNode::List(parts) = child {
            if parts.len() >= 2 {
                let name = expect_atom(&parts[0]).map_err(|_| {
                    err("input", "arg name must be atom", &["arg_name"], "")
                })?.to_string();
                // Skip if it's a keyword
                if name.starts_with(':') { continue; }
                let type_str = expect_atom(&parts[1]).map_err(|_| {
                    err("input", "arg type must be atom", &["string|integer|..."], "")
                })?.to_string();
                let arg_type = ArgType::from_str(&type_str)?;

                let required = has_flag(child, ":required");
                let description = child.keyword(":description").map(String::from);
                let default = child.keyword(":default").map(String::from);

                let enum_values = match child.keyword_node(":enum") {
                    Some(n) => collect_atoms(n),
                    None => vec![],
                };

                args.push(ArgDecl { name, arg_type, required, description, enum_values, default });
            }
        }
    }
    Ok(args)
}

fn parse_events(parent: &SexpNode) -> Vec<EventEmitDecl> {
    let node = match parent.keyword_node(":events") {
        Some(n) => n,
        None => return vec![],
    };
    match node {
        SexpNode::List(items) => items.iter().filter_map(|item| {
            if let SexpNode::List(parts) = item {
                if !parts.is_empty() {
                    let event_name = expect_atom(&parts[0]).ok()?.to_string();
                    let condition = item.keyword(":when").map(String::from);
                    let payload = item.keyword(":payload").map(String::from);
                    return Some(EventEmitDecl { event_name, condition, payload });
                }
            }
            None
        }).collect(),
        _ => vec![],
    }
}

fn parse_validations(parent: &SexpNode) -> Vec<ValidationRule> {
    let node = match parent.keyword_node(":validation") {
        Some(n) => n,
        None => return vec![],
    };
    match node {
        SexpNode::List(items) => items.iter().filter_map(|item| {
            if let SexpNode::List(parts) = item {
                if !parts.is_empty() {
                    let field = expect_atom(&parts[0]).ok()?.to_string();
                    let min_len = item.keyword(":min-len").and_then(|s| s.parse().ok());
                    let max_len = item.keyword(":max-len").and_then(|s| s.parse().ok());
                    return Some(ValidationRule { field, min_len, max_len });
                }
            }
            None
        }).collect(),
        _ => vec![],
    }
}

// ---------------------------------------------------------------------------
// Utility helpers
// ---------------------------------------------------------------------------

fn expect_tag<'a>(node: &'a SexpNode, expected: &str) -> Result<&'a str, IrError> {
    let tag = node.tag().ok_or_else(|| err("node", "expected list", &[expected], "atom"))?;
    if tag != expected {
        return Err(err("tag", "wrong tag", &[expected], tag));
    }
    Ok(tag)
}

fn expect_list(node: &SexpNode) -> Result<&Vec<SexpNode>, IrError> {
    match node {
        SexpNode::List(c) => Ok(c),
        SexpNode::Atom(s) => Err(err("node", "expected list", &["(...)"], s)),
    }
}

fn expect_atom<'a>(node: &'a SexpNode) -> Result<&'a str, IrError> {
    match node {
        SexpNode::Atom(s) => Ok(s),
        SexpNode::List(_) => Err(err("node", "expected atom", &["atom"], "(…)")),
    }
}

fn keyword_or(node: &SexpNode, key: &str, default: &str) -> String {
    node.keyword(key).map(String::from).unwrap_or_else(|| default.to_string())
}

fn has_flag(node: &SexpNode, key: &str) -> bool {
    if let SexpNode::List(children) = node {
        children.iter().any(|c| matches!(c, SexpNode::Atom(s) if s == key))
    } else {
        false
    }
}

fn collect_atoms(node: &SexpNode) -> Vec<String> {
    match node {
        SexpNode::List(items) => items.iter().filter_map(|i| {
            if let SexpNode::Atom(s) = i {
                if !s.starts_with(':') { return Some(s.clone()); }
            }
            None
        }).collect(),
        SexpNode::Atom(s) => vec![s.clone()],
    }
}

fn pascal_case(s: &str) -> String {
    s.split(&['-', '_'][..])
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().chain(c).collect(),
            }
        })
        .collect()
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
    fn parse_simple_tool() {
        let sexp = parse(r#"(mcp-module board
          :struct BoardModule
          :deps ((storage "Arc<Storage>"))
          (tool jarvis_board_delete
            :description "Delete a Board task"
            (input (id string :required))
            :returns "Value"))"#);
        let m = McpModuleDecl::from_sexp(&sexp).unwrap();
        assert_eq!(m.name, "board");
        assert_eq!(m.struct_name, "BoardModule");
        assert_eq!(m.deps.len(), 1);
        assert_eq!(m.tools.len(), 1);
        assert_eq!(m.tools[0].name, "jarvis_board_delete");
        assert_eq!(m.tools[0].args.len(), 1);
        assert!(m.tools[0].args[0].required);
    }

    #[test]
    fn parse_tool_with_enum_and_default() {
        let sexp = parse(r#"(mcp-module kb
          :struct KbModule
          :deps ((storage "Arc<Storage>"))
          (tool jarvis_kb_search
            :description "Search KB"
            (input
              (query string :required)
              (category string :description "Filter")
              (limit integer :default 10))
            :returns "Value"))"#);
        let m = McpModuleDecl::from_sexp(&sexp).unwrap();
        let t = &m.tools[0];
        assert_eq!(t.args.len(), 3);
        assert!(t.args[0].required);
        assert!(!t.args[1].required);
        assert_eq!(t.args[2].default, Some("10".into()));
    }

    #[test]
    fn parse_tool_with_events() {
        let sexp = parse(r#"(mcp-module board
          :struct BoardModule
          :deps ((storage "Arc<Storage>"))
          (tool jarvis_board_create
            :description "Create task"
            (input (title string :required))
            :returns "Value"
            :events ((task_ready :when "auto_execute == true"))))"#);
        let m = McpModuleDecl::from_sexp(&sexp).unwrap();
        assert_eq!(m.tools[0].events.len(), 1);
        assert_eq!(m.tools[0].events[0].event_name, "task_ready");
        assert_eq!(m.tools[0].events[0].condition, Some("auto_execute == true".into()));
    }

    #[test]
    fn parse_full_pattern_file() {
        let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let pattern_path = std::path::Path::new(&manifest)
            .parent().unwrap().parent().unwrap()
            .join("patterns/jarvis-mcp-tools.lisp");
        let src = std::fs::read_to_string(&pattern_path)
            .unwrap_or_else(|e| panic!("failed to read: {e}"));
        let nodes = SexpNode::parse(&src).unwrap();
        let modules = parse_mcp_modules(&nodes).unwrap();

        assert_eq!(modules.len(), 4, "expected 4 mcp modules");
        assert_eq!(modules[0].name, "board");
        assert_eq!(modules[0].tools.len(), 5);
        assert_eq!(modules[1].name, "kb");
        assert_eq!(modules[1].tools.len(), 4);
        assert_eq!(modules[2].name, "code_map");
        assert_eq!(modules[2].tools.len(), 1);
        assert_eq!(modules[3].name, "context");
        assert_eq!(modules[3].tools.len(), 1);

        let total_tools: usize = modules.iter().map(|m| m.tools.len()).sum();
        assert_eq!(total_tools, 11, "expected 11 tools total");
    }

    #[test]
    fn reject_invalid_arg_type() {
        let sexp = parse(r#"(mcp-module x
          :struct X
          (tool t1 :description "test"
            (input (bad_arg mongodb_cursor))
            :returns "Value"))"#);
        let err = McpModuleDecl::from_sexp(&sexp).unwrap_err();
        assert_eq!(err.field, "arg type");
    }
}
