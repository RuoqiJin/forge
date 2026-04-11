//! RPC Gateway IR — typed representation of `(rpc-gateway ...)` declarations.
//!
//! The "generalized interface layer" pattern:
//! - Declares transport protocols (stdio, ipc, ws)
//! - Declares JSON-RPC method routes with input/output types
//! - Declares tool dispatch groups (prefix-based or exact match)
//!
//! Forge generates:
//!   - JSON-RPC 2.0 request/response structs + serialization
//!   - Transport read/write loops (newline-delimited JSON)
//!   - Dispatch match statement routing tool_name → handler module
//!   - Error wrapping (RpcError → ErrorObject → ToolResult)
//!   - `pub trait RpcGatewayLogic` for hand-written business handlers

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
            "RpcGateway IR error in `{}`: {} (expected {:?}, got {:?})",
            self.field, self.message, self.expected, self.got
        )
    }
}

impl std::error::Error for IrError {}

// ---------------------------------------------------------------------------
// Transport types — the whitelist
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum Transport {
    Stdio,
    Ipc,
    WebSocket,
}

impl Transport {
    pub fn from_str(s: &str) -> Result<Self, IrError> {
        match s {
            "stdio" => Ok(Self::Stdio),
            "ipc" => Ok(Self::Ipc),
            "ws" | "websocket" => Ok(Self::WebSocket),
            other => Err(IrError {
                field: "transport".into(),
                message: "unknown transport".into(),
                expected: vec!["stdio".into(), "ipc".into(), "ws".into()],
                got: other.into(),
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// Declaration types
// ---------------------------------------------------------------------------

/// An input/output parameter in a method signature.
#[derive(Debug, Clone)]
pub struct ParamDecl {
    pub name: String,
    pub rust_type: String,
    pub required: bool,
    pub rename: Option<String>, // serde rename (e.g., camelCase → snake_case)
    pub description: Option<String>,
}

/// A single RPC method in the gateway.
#[derive(Debug, Clone)]
pub struct MethodDecl {
    pub name: String,       // JSON-RPC method name (e.g., "tools/call", "ping")
    pub handler: String,    // Rust handler function name
    pub input: Vec<ParamDecl>,
    pub output: String,     // Rust return type
    pub description: Option<String>,
}

/// A tool dispatch group — maps tool names to handler modules.
#[derive(Debug, Clone)]
pub struct DispatchGroupDecl {
    pub module_name: String,   // Rust module name (e.g., "task", "pty", "kb")
    pub tools: Vec<String>,    // Exact tool names routed to this module
    pub prefix: Option<String>, // Optional prefix routing (e.g., "mission_kb_")
}

/// An error code constant.
#[derive(Debug, Clone)]
pub struct ErrorCodeDecl {
    pub name: String,   // Rust const name (e.g., "UNKNOWN_TOOL")
    pub value: String,  // String value (e.g., "UNKNOWN_TOOL")
}

/// Top-level RPC gateway declaration.
#[derive(Debug, Clone)]
pub struct RpcGatewayDecl {
    pub name: String,
    pub target: Option<String>,
    pub transports: Vec<Transport>,
    pub methods: Vec<MethodDecl>,
    pub dispatch_groups: Vec<DispatchGroupDecl>,
    pub error_codes: Vec<ErrorCodeDecl>,
}

// ---------------------------------------------------------------------------
// S-EXP → IR lowering
// ---------------------------------------------------------------------------

impl RpcGatewayDecl {
    pub fn from_sexp(node: &SexpNode) -> Result<Self, IrError> {
        let children = expect_list(node)?;
        if children.len() < 2 {
            return Err(err("rpc-gateway", "missing gateway name", &["name"], ""));
        }

        expect_tag(node, "rpc-gateway")?;
        let name = expect_atom(&children[1])?.to_string();
        let target = node.keyword(":target").map(String::from);

        let transports = parse_transports(node)?;
        let methods = parse_methods(node)?;
        let dispatch_groups = parse_dispatch_groups(node)?;
        let error_codes = parse_error_codes(node);

        if transports.is_empty() {
            return Err(err(
                "transports",
                "must declare at least one transport",
                &["(transports stdio ipc ws)"],
                &name,
            ));
        }

        Ok(RpcGatewayDecl {
            name,
            target,
            transports,
            methods,
            dispatch_groups,
            error_codes,
        })
    }
}

/// Parse all `(rpc-gateway ...)` declarations from a top-level S-EXP list.
pub fn parse_rpc_gateways(nodes: &[SexpNode]) -> Result<Vec<RpcGatewayDecl>, IrError> {
    nodes
        .iter()
        .filter(|n| n.tag() == Some("rpc-gateway"))
        .map(RpcGatewayDecl::from_sexp)
        .collect()
}

// ---------------------------------------------------------------------------
// Sub-parsers
// ---------------------------------------------------------------------------

fn parse_transports(parent: &SexpNode) -> Result<Vec<Transport>, IrError> {
    let children = match parent {
        SexpNode::List(c) => c,
        _ => return Ok(vec![]),
    };
    for child in children {
        if child.tag() == Some("transports") {
            if let SexpNode::List(items) = child {
                return items
                    .iter()
                    .skip(1)
                    .map(|item| {
                        let s = expect_atom(item)?;
                        Transport::from_str(s)
                    })
                    .collect();
            }
        }
    }
    Ok(vec![])
}

fn parse_methods(parent: &SexpNode) -> Result<Vec<MethodDecl>, IrError> {
    let children = match parent {
        SexpNode::List(c) => c,
        _ => return Ok(vec![]),
    };
    let mut methods = Vec::new();
    for child in children {
        if child.tag() == Some("methods") {
            if let SexpNode::List(items) = child {
                for item in items.iter().skip(1) {
                    if item.tag() == Some("method") {
                        methods.push(parse_one_method(item)?);
                    }
                }
            }
        }
    }
    Ok(methods)
}

fn parse_one_method(node: &SexpNode) -> Result<MethodDecl, IrError> {
    let children = expect_list(node)?;
    if children.len() < 2 {
        return Err(err("method", "missing method name", &["method_name"], ""));
    }

    let name = expect_atom(&children[1])?.to_string();
    let handler = node
        .keyword(":handler")
        .map(String::from)
        .unwrap_or_else(|| name.replace('/', "_").replace('.', "_"));
    let output = keyword_or(node, ":output", "Value");
    let description = node.keyword(":description").map(String::from);
    let input = parse_params(node);

    Ok(MethodDecl {
        name,
        handler,
        input,
        output,
        description,
    })
}

fn parse_params(parent: &SexpNode) -> Vec<ParamDecl> {
    let node = match parent.keyword_node(":input") {
        Some(n) => n,
        None => return vec![],
    };
    let items = match node {
        SexpNode::List(items) => items,
        _ => return vec![],
    };
    items
        .iter()
        .filter_map(|item| {
            if let SexpNode::List(parts) = item {
                if parts.len() >= 2 {
                    let name = expect_atom(&parts[0]).ok()?.to_string();
                    let rust_type = expect_atom(&parts[1]).ok()?.to_string();
                    let required = item.keyword(":required").is_some()
                        || item
                            .keyword(":required")
                            .map(|v| v == "true")
                            .unwrap_or(false);
                    // Check if :required appears as a bare keyword (no value)
                    let required = required || has_bare_keyword(item, ":required");
                    let rename = item.keyword(":rename").map(String::from);
                    let description = item.keyword(":description").map(String::from);
                    return Some(ParamDecl {
                        name,
                        rust_type,
                        required,
                        rename,
                        description,
                    });
                }
            }
            None
        })
        .collect()
}

fn has_bare_keyword(node: &SexpNode, key: &str) -> bool {
    if let SexpNode::List(items) = node {
        items.iter().any(|item| {
            if let SexpNode::Atom(s) = item {
                s == key
            } else {
                false
            }
        })
    } else {
        false
    }
}

fn parse_dispatch_groups(parent: &SexpNode) -> Result<Vec<DispatchGroupDecl>, IrError> {
    let children = match parent {
        SexpNode::List(c) => c,
        _ => return Ok(vec![]),
    };
    let mut groups = Vec::new();
    for child in children {
        if child.tag() == Some("dispatch") {
            if let SexpNode::List(items) = child {
                for item in items.iter().skip(1) {
                    if item.tag() == Some("group") {
                        groups.push(parse_one_group(item)?);
                    }
                }
            }
        }
    }
    Ok(groups)
}

fn parse_one_group(node: &SexpNode) -> Result<DispatchGroupDecl, IrError> {
    let children = expect_list(node)?;
    if children.len() < 2 {
        return Err(err("group", "missing group module name", &["module_name"], ""));
    }

    let module_name = expect_atom(&children[1])?.to_string();
    let prefix = node.keyword(":prefix").map(String::from);

    let mut tools = Vec::new();
    for child in children.iter().skip(2) {
        if child.tag() == Some("tools") {
            if let SexpNode::List(items) = child {
                for item in items.iter().skip(1) {
                    if let SexpNode::Atom(s) = item {
                        if !s.starts_with(':') {
                            tools.push(s.clone());
                        }
                    }
                }
            }
        }
    }

    Ok(DispatchGroupDecl {
        module_name,
        tools,
        prefix,
    })
}

fn parse_error_codes(parent: &SexpNode) -> Vec<ErrorCodeDecl> {
    let children = match parent {
        SexpNode::List(c) => c,
        _ => return vec![],
    };
    for child in children {
        if child.tag() == Some("error-codes") {
            if let SexpNode::List(items) = child {
                return items
                    .iter()
                    .skip(1)
                    .filter_map(|item| {
                        let name = expect_atom(item).ok()?.to_string();
                        Some(ErrorCodeDecl {
                            value: name.clone(),
                            name,
                        })
                    })
                    .collect();
            }
        }
    }
    vec![]
}

// ---------------------------------------------------------------------------
// S-EXP utility helpers
// ---------------------------------------------------------------------------

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

fn keyword_or(node: &SexpNode, key: &str, default: &str) -> String {
    node.keyword(key)
        .map(String::from)
        .unwrap_or_else(|| default.to_string())
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
    fn parse_basic_gateway() {
        let sexp = parse(
            r#"(rpc-gateway missiond-mcp
              :target "missiond-mcp/src/gen_server.rs"
              (transports stdio ipc)
              (methods
                (method "initialize"
                  :handler handle_initialize
                  :output InitializeResult
                  :description "MCP initialization handshake")
                (method "tools/call"
                  :handler handle_tools_call
                  :input ((name String :required) (arguments Value))
                  :output ToolResult
                  :description "Execute a tool by name")
                (method "ping"
                  :handler handle_ping
                  :output Value))
              (dispatch
                (group task
                  (tools mission_task_submit mission_task_query mission_task_cancel))
                (group pty
                  (tools mission_pty_spawn mission_pty_send mission_pty_read
                        mission_pty_status mission_pty_screenshot))
                (group kb :prefix "mission_kb_"))
              (error-codes
                UNKNOWN_TOOL MISSING_PARAM INVALID_PARAM NOT_FOUND
                PERMISSION_DENIED IPC_TIMEOUT DB_ERROR))"#,
        );
        let decl = RpcGatewayDecl::from_sexp(&sexp).unwrap();

        assert_eq!(decl.name, "missiond-mcp");
        assert_eq!(decl.transports, vec![Transport::Stdio, Transport::Ipc]);

        // Methods
        assert_eq!(decl.methods.len(), 3);
        assert_eq!(decl.methods[0].name, "initialize");
        assert_eq!(decl.methods[0].handler, "handle_initialize");
        assert_eq!(decl.methods[0].output, "InitializeResult");
        assert_eq!(decl.methods[1].name, "tools/call");
        assert_eq!(decl.methods[1].input.len(), 2);
        assert_eq!(decl.methods[1].input[0].name, "name");
        assert!(decl.methods[1].input[0].required);
        assert_eq!(decl.methods[2].name, "ping");
        assert_eq!(decl.methods[2].output, "Value");

        // Dispatch groups
        assert_eq!(decl.dispatch_groups.len(), 3);
        assert_eq!(decl.dispatch_groups[0].module_name, "task");
        assert_eq!(decl.dispatch_groups[0].tools.len(), 3);
        assert_eq!(decl.dispatch_groups[1].module_name, "pty");
        assert_eq!(decl.dispatch_groups[1].tools.len(), 5);
        assert_eq!(decl.dispatch_groups[2].module_name, "kb");
        assert_eq!(decl.dispatch_groups[2].prefix, Some("mission_kb_".into()));

        // Error codes
        assert_eq!(decl.error_codes.len(), 7);
        assert_eq!(decl.error_codes[0].name, "UNKNOWN_TOOL");
    }

    #[test]
    fn reject_missing_transports() {
        let sexp = parse(r#"(rpc-gateway empty)"#);
        let err = RpcGatewayDecl::from_sexp(&sexp).unwrap_err();
        assert_eq!(err.field, "transports");
    }

    #[test]
    fn reject_invalid_transport() {
        let sexp = parse(r#"(rpc-gateway bad (transports grpc))"#);
        let err = RpcGatewayDecl::from_sexp(&sexp).unwrap_err();
        assert_eq!(err.field, "transport");
        assert!(err.got.contains("grpc"));
    }

    #[test]
    fn parse_websocket_transport() {
        let sexp = parse(r#"(rpc-gateway ws-gw (transports ws stdio))"#);
        let decl = RpcGatewayDecl::from_sexp(&sexp).unwrap();
        assert_eq!(decl.transports, vec![Transport::WebSocket, Transport::Stdio]);
    }

    #[test]
    fn method_handler_defaults_from_name() {
        let sexp = parse(
            r#"(rpc-gateway auto-handler
              (transports stdio)
              (methods
                (method "tools/call" :output Value)
                (method "notifications.initialized" :output Value)))"#,
        );
        let decl = RpcGatewayDecl::from_sexp(&sexp).unwrap();
        assert_eq!(decl.methods[0].handler, "tools_call");
        assert_eq!(decl.methods[1].handler, "notifications_initialized");
    }

    #[test]
    fn prefix_routing_group() {
        let sexp = parse(
            r#"(rpc-gateway prefix-test
              (transports stdio)
              (dispatch
                (group board :prefix "mission_board_"
                  (tools mission_board_create mission_board_update))
                (group external :prefix "xjp_")))"#,
        );
        let decl = RpcGatewayDecl::from_sexp(&sexp).unwrap();
        assert_eq!(decl.dispatch_groups.len(), 2);
        assert_eq!(decl.dispatch_groups[0].prefix, Some("mission_board_".into()));
        assert_eq!(decl.dispatch_groups[0].tools.len(), 2);
        assert_eq!(decl.dispatch_groups[1].prefix, Some("xjp_".into()));
        assert_eq!(decl.dispatch_groups[1].tools.len(), 0);
    }
}
