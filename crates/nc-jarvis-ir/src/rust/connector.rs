//! Connector IR — typed representation of `(connector ...)` declarations.
//!
//! The "connector" pattern generates LLM provider connector boilerplate for
//! xjp-router. Each connector:
//! - Wraps an HTTP client with timeout, auth, and secret loading
//! - Declares capabilities (text, vision, tools, stream, etc.)
//! - Uses Generation Gap: generated shell + hand-written `invoke_impl()`
//!
//! ```lisp
//! (connector acme
//!   :base-url "http://example.com"
//!   :auth api-key
//!   :timeout 120
//!   :capabilities (text vision tools stream)
//!   :secret-namespace "acme-shared"
//!   :secret-keys (("base-url" "providers/acme/base-url")
//!                 ("api-key" "providers/acme/api-key")))
//! ```

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
            "Connector IR error in `{}`: {} (expected {:?}, got {:?})",
            self.field, self.message, self.expected, self.got
        )
    }
}

impl std::error::Error for IrError {}

// ---------------------------------------------------------------------------
// Auth scheme
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum AuthScheme {
    ApiKey,
    BearerToken,
    ServiceAccount,
    None,
}

impl AuthScheme {
    pub fn from_str(s: &str) -> Result<Self, IrError> {
        match s {
            "api-key" => Ok(Self::ApiKey),
            "bearer-token" => Ok(Self::BearerToken),
            "service-account" => Ok(Self::ServiceAccount),
            "none" => Ok(Self::None),
            other => Err(err(
                ":auth",
                "unknown auth scheme",
                &["api-key", "bearer-token", "service-account", "none"],
                other,
            )),
        }
    }
}

// ---------------------------------------------------------------------------
// Capabilities
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct Capabilities {
    pub text: bool,
    pub vision: bool,
    pub video: bool,
    pub tools: bool,
    pub stream: bool,
    pub image_generation: bool,
}

// ---------------------------------------------------------------------------
// Secret key mapping
// ---------------------------------------------------------------------------

/// Maps a field name to a secret path, e.g. ("api-key", "providers/acme/api-key").
#[derive(Debug, Clone)]
pub struct SecretKeyMapping {
    pub field_name: String,
    pub secret_path: String,
}

// ---------------------------------------------------------------------------
// Top-level declaration
// ---------------------------------------------------------------------------

/// Top-level connector declaration.
#[derive(Debug, Clone)]
pub struct ConnectorDecl {
    pub name: String,
    pub base_url: Option<String>,
    pub auth: AuthScheme,
    pub timeout_secs: u64,
    pub capabilities: Capabilities,
    pub secret_namespace: Option<String>,
    pub secret_keys: Vec<SecretKeyMapping>,
}

// ---------------------------------------------------------------------------
// S-EXP → IR lowering
// ---------------------------------------------------------------------------

impl ConnectorDecl {
    pub fn from_sexp(node: &SexpNode) -> Result<Self, IrError> {
        let children = expect_list(node)?;
        if children.len() < 2 {
            return Err(err(
                "connector",
                "missing connector name",
                &["connector_name"],
                "",
            ));
        }

        expect_tag(node, "connector")?;
        let name = expect_atom(&children[1])?.to_string();

        let base_url = node.keyword(":base-url").map(String::from);

        let auth = match node.keyword(":auth") {
            Some(s) => AuthScheme::from_str(s)?,
            None => AuthScheme::None,
        };

        let timeout_secs = match node.keyword(":timeout") {
            Some(s) => s.parse().map_err(|_| {
                err(":timeout", "invalid timeout seconds", &["120", "30"], s)
            })?,
            None => 120,
        };

        let capabilities = parse_capabilities(node);
        let secret_namespace = node.keyword(":secret-namespace").map(String::from);
        let secret_keys = parse_secret_keys(node);

        Ok(ConnectorDecl {
            name,
            base_url,
            auth,
            timeout_secs,
            capabilities,
            secret_namespace,
            secret_keys,
        })
    }
}

/// Parse all `(connector ...)` declarations from a top-level S-EXP list.
pub fn parse_connectors(nodes: &[SexpNode]) -> Result<Vec<ConnectorDecl>, IrError> {
    nodes
        .iter()
        .filter(|n| n.tag() == Some("connector"))
        .map(ConnectorDecl::from_sexp)
        .collect()
}

// ---------------------------------------------------------------------------
// Sub-parsers
// ---------------------------------------------------------------------------

fn parse_capabilities(parent: &SexpNode) -> Capabilities {
    let node = match parent.keyword_node(":capabilities") {
        Some(n) => n,
        None => return Capabilities::default(),
    };
    let atoms = collect_atoms(node);
    Capabilities {
        text: atoms.iter().any(|a| a == "text"),
        vision: atoms.iter().any(|a| a == "vision"),
        video: atoms.iter().any(|a| a == "video"),
        tools: atoms.iter().any(|a| a == "tools"),
        stream: atoms.iter().any(|a| a == "stream"),
        image_generation: atoms.iter().any(|a| a == "image-generation"),
    }
}

fn parse_secret_keys(parent: &SexpNode) -> Vec<SecretKeyMapping> {
    let node = match parent.keyword_node(":secret-keys") {
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
                    let field_name = expect_atom(&parts[0]).ok()?.to_string();
                    let secret_path = expect_atom(&parts[1]).ok()?.to_string();
                    return Some(SecretKeyMapping {
                        field_name,
                        secret_path,
                    });
                }
            }
            None
        })
        .collect()
}

// ---------------------------------------------------------------------------
// S-EXP utility helpers (same pattern as worker.rs)
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
        SexpNode::List(_) => Err(err("node", "expected atom", &["atom"], "(...)")),
    }
}

fn collect_atoms(node: &SexpNode) -> Vec<String> {
    match node {
        SexpNode::List(items) => items
            .iter()
            .filter_map(|i| {
                if let SexpNode::Atom(s) = i {
                    if !s.starts_with(':') {
                        return Some(s.clone());
                    }
                }
                None
            })
            .collect(),
        SexpNode::Atom(s) => vec![s.clone()],
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
    fn parse_basic_connector() {
        let sexp = parse(
            r#"(connector acme
              :base-url "http://example.com"
              :auth api-key
              :timeout 120
              :capabilities (text vision tools stream)
              :secret-namespace "acme-shared"
              :secret-keys (("base-url" "providers/acme/base-url")
                            ("api-key" "providers/acme/api-key")))"#,
        );
        let c = ConnectorDecl::from_sexp(&sexp).unwrap();
        assert_eq!(c.name, "acme");
        assert_eq!(c.base_url.as_deref(), Some("http://example.com"));
        assert_eq!(c.auth, AuthScheme::ApiKey);
        assert_eq!(c.timeout_secs, 120);
        assert!(c.capabilities.text);
        assert!(c.capabilities.vision);
        assert!(c.capabilities.tools);
        assert!(c.capabilities.stream);
        assert!(!c.capabilities.video);
        assert!(!c.capabilities.image_generation);
        assert_eq!(c.secret_namespace.as_deref(), Some("acme-shared"));
        assert_eq!(c.secret_keys.len(), 2);
        assert_eq!(c.secret_keys[0].field_name, "base-url");
        assert_eq!(c.secret_keys[0].secret_path, "providers/acme/base-url");
        assert_eq!(c.secret_keys[1].field_name, "api-key");
        assert_eq!(c.secret_keys[1].secret_path, "providers/acme/api-key");
    }

    #[test]
    fn parse_minimal_connector() {
        let sexp = parse(r#"(connector openai :auth bearer-token)"#);
        let c = ConnectorDecl::from_sexp(&sexp).unwrap();
        assert_eq!(c.name, "openai");
        assert_eq!(c.auth, AuthScheme::BearerToken);
        assert_eq!(c.timeout_secs, 120); // default
        assert!(c.base_url.is_none());
        assert!(c.secret_namespace.is_none());
        assert!(c.secret_keys.is_empty());
    }

    #[test]
    fn parse_no_auth_connector() {
        let sexp = parse(
            r#"(connector local-llm
              :base-url "http://localhost:8080"
              :auth none
              :timeout 30
              :capabilities (text stream))"#,
        );
        let c = ConnectorDecl::from_sexp(&sexp).unwrap();
        assert_eq!(c.name, "local-llm");
        assert_eq!(c.auth, AuthScheme::None);
        assert_eq!(c.timeout_secs, 30);
        assert!(c.capabilities.text);
        assert!(c.capabilities.stream);
        assert!(!c.capabilities.vision);
    }

    #[test]
    fn reject_invalid_auth() {
        let sexp = parse(r#"(connector x :auth magic-token)"#);
        let err = ConnectorDecl::from_sexp(&sexp).unwrap_err();
        assert_eq!(err.field, ":auth");
        assert!(err.got.contains("magic-token"));
    }
}
