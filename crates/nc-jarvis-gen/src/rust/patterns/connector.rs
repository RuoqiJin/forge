//! Connector code generator.
//!
//! Generates LLM provider connector boilerplate for xjp-router:
//! - Struct with HTTP client, base URL, and auth credentials
//! - Constructor with secret loading (from SecretProvider + env fallback)
//! - `Connector` trait impl with capabilities declaration
//! - `invoke()` delegates to hand-written `invoke_impl()` (Generation Gap)
//!
//! ## Generation Gap
//!
//! The generated file provides the scaffold; the custom file implements
//! `invoke_impl()` with provider-specific request/response translation.

use nc_jarvis_ir::connector::*;

use crate::common::{w, blank, forge_header};

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub fn generate_connector(decl: &ConnectorDecl) -> String {
    let mut out = String::with_capacity(2048);

    gen_header(&mut out, decl);
    gen_trait_impl_imports(&mut out, decl);
    blank(&mut out);
    gen_connector_impl(&mut out, decl);

    out
}

// ---------------------------------------------------------------------------
// Header
// ---------------------------------------------------------------------------

fn gen_header(out: &mut String, decl: &ConnectorDecl) {
    forge_header(out, "rust", &format!("intent.lisp (connector {})", decl.name));
    blank(out);
}

// ---------------------------------------------------------------------------
// Imports
// ---------------------------------------------------------------------------

fn gen_imports(out: &mut String, _decl: &ConnectorDecl) {
    w(out, 0, "use std::collections::HashMap;");
    w(out, 0, "use std::sync::Arc;");
    w(out, 0, "use std::time::Duration;");
    w(out, 0, "use reqwest::Client;");
    w(out, 0, "use crate::connectors::{Connector, ConnectorCapabilities, ConnectorError, ConnectorResponse};");
    w(out, 0, "use crate::core::entities::{UnifiedRequest, UnifiedChunk};");
    w(out, 0, "use crate::registry::EgressRoute;");
    w(out, 0, "use crate::secret_store::SecretProvider;");
}

/// Slim imports for trait-impl-only generated file (no struct/constructor).
fn gen_trait_impl_imports(out: &mut String, decl: &ConnectorDecl) {
    let struct_name = to_pascal_case(&decl.name);
    let mod_name = decl.name.replace('-', "_");
    w(out, 0, &format!("use super::{mod_name}_impl::{struct_name}Connector;"));
    w(out, 0, "use crate::connectors::{Connector, ConnectorCapabilities, ConnectorError, ConnectorResponse};");
    w(out, 0, "use crate::core::entities::UnifiedRequest;");
    w(out, 0, "use crate::registry::EgressRoute;");
}

// ---------------------------------------------------------------------------
// Struct definition
// ---------------------------------------------------------------------------

fn gen_struct(out: &mut String, decl: &ConnectorDecl) {
    let struct_name = to_pascal_case(&decl.name);
    w(out, 0, &format!("pub struct {struct_name}Connector {{"));
    w(out, 1, "pub(crate) client: Client,");
    w(out, 1, "pub(crate) base: String,");

    match decl.auth {
        AuthScheme::ApiKey => w(out, 1, "pub(crate) api_key: Option<String>,"),
        AuthScheme::BearerToken => w(out, 1, "pub(crate) bearer_token: Option<String>,"),
        AuthScheme::ServiceAccount => w(out, 1, "pub(crate) service_account: Option<String>,"),
        AuthScheme::None => {}
    }

    w(out, 0, "}");
}

// ---------------------------------------------------------------------------
// Constructor (new)
// ---------------------------------------------------------------------------

fn gen_constructor(out: &mut String, decl: &ConnectorDecl) {
    let struct_name = to_pascal_case(&decl.name);
    let ns = decl
        .secret_namespace
        .as_deref()
        .unwrap_or(&decl.name);

    w(out, 0, &format!("impl {struct_name}Connector {{"));
    w(out, 1, "pub fn new(");
    w(out, 2, "_secret_provider: Arc<dyn SecretProvider>,");
    w(out, 2, "preloaded_secrets: &HashMap<String, String>,");
    w(out, 1, ") -> anyhow::Result<Self> {");

    // Client builder
    w(
        out,
        2,
        &format!(
            "let client = Client::builder().timeout(Duration::from_secs({})).build()?;",
            decl.timeout_secs
        ),
    );
    blank(out);

    // Base URL loading
    let base_secret = decl
        .secret_keys
        .iter()
        .find(|sk| sk.field_name == "base-url");

    let default_url = decl
        .base_url
        .as_deref()
        .unwrap_or("http://localhost:8080");
    let env_base = format!("{}_BASE_URL", to_screaming_snake(&decl.name));

    if let Some(sk) = base_secret {
        w(out, 2, &format!(
            "let base = preloaded_secrets.get(\"{ns}::{}\").cloned()",
            sk.secret_path
        ));
        w(out, 3, &format!(
            ".or_else(|| std::env::var(\"{env_base}\").ok())"
        ));
        w(out, 3, &format!(
            ".unwrap_or_else(|| \"{default_url}\".to_string());"
        ));
    } else {
        w(out, 2, &format!(
            "let base = std::env::var(\"{env_base}\").unwrap_or_else(|_| \"{default_url}\".to_string());"
        ));
    }

    // Auth credential loading
    match decl.auth {
        AuthScheme::ApiKey => {
            gen_auth_field(out, decl, ns, "api-key", "api_key", "API_KEY");
        }
        AuthScheme::BearerToken => {
            gen_auth_field(out, decl, ns, "bearer-token", "bearer_token", "BEARER_TOKEN");
        }
        AuthScheme::ServiceAccount => {
            gen_auth_field(out, decl, ns, "service-account", "service_account", "SERVICE_ACCOUNT");
        }
        AuthScheme::None => {}
    }

    blank(out);

    // Construct Self
    w(out, 2, "Ok(Self {");
    w(out, 3, "client,");
    w(out, 3, "base,");
    match decl.auth {
        AuthScheme::ApiKey => w(out, 3, "api_key,"),
        AuthScheme::BearerToken => w(out, 3, "bearer_token,"),
        AuthScheme::ServiceAccount => w(out, 3, "service_account,"),
        AuthScheme::None => {}
    }
    w(out, 2, "})");

    w(out, 1, "}");
    w(out, 0, "}");
}

fn gen_auth_field(
    out: &mut String,
    decl: &ConnectorDecl,
    ns: &str,
    secret_field: &str,
    rust_field: &str,
    env_suffix: &str,
) {
    let secret = decl
        .secret_keys
        .iter()
        .find(|sk| sk.field_name == secret_field);
    let env_var = format!("{}_{}", to_screaming_snake(&decl.name), env_suffix);

    blank(out);
    if let Some(sk) = secret {
        w(out, 2, &format!(
            "let {rust_field} = preloaded_secrets.get(\"{ns}::{}\").cloned()",
            sk.secret_path
        ));
        w(out, 3, &format!(
            ".or_else(|| std::env::var(\"{env_var}\").ok());"
        ));
    } else {
        w(out, 2, &format!(
            "let {rust_field} = std::env::var(\"{env_var}\").ok();"
        ));
    }
}

// ---------------------------------------------------------------------------
// Connector trait impl
// ---------------------------------------------------------------------------

fn gen_connector_impl(out: &mut String, decl: &ConnectorDecl) {
    let struct_name = to_pascal_case(&decl.name);
    let cap = &decl.capabilities;

    w(out, 0, "#[async_trait::async_trait]");
    w(out, 0, &format!("impl Connector for {struct_name}Connector {{"));

    // name()
    w(out, 1, "fn name(&self) -> &'static str {");
    w(out, 2, &format!("\"{}\"", decl.name));
    w(out, 1, "}");
    blank(out);

    // capabilities()
    w(out, 1, "fn capabilities(&self) -> ConnectorCapabilities {");
    w(out, 2, "ConnectorCapabilities {");
    w(out, 3, &format!("text: {},", cap.text));
    w(out, 3, &format!("vision: {},", cap.vision));
    w(out, 3, &format!("video: {},", cap.video));
    w(out, 3, &format!("tools: {},", cap.tools));
    w(out, 3, &format!("stream: {},", cap.stream));
    w(out, 3, &format!("image_generation: {},", cap.image_generation));
    w(out, 2, "}");
    w(out, 1, "}");
    blank(out);

    // invoke() — delegates to hand-written invoke_impl()
    w(out, 1, "async fn invoke(");
    w(out, 2, "&self,");
    w(out, 2, "route: &EgressRoute,");
    w(out, 2, "req: UnifiedRequest,");
    w(out, 1, ") -> Result<ConnectorResponse, ConnectorError> {");
    w(out, 2, "// Delegate to hand-written implementation (Generation Gap)");
    w(out, 2, "self.invoke_impl(route, req).await");
    w(out, 1, "}");

    w(out, 0, "}");
}

// ---------------------------------------------------------------------------
// Utilities
// ---------------------------------------------------------------------------

/// Convert kebab-case to PascalCase: "acme" -> "Acme", "local-llm" -> "LocalLlm"
fn to_pascal_case(s: &str) -> String {
    s.split('-')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(c) => c.to_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

/// Convert kebab-case to SCREAMING_SNAKE_CASE: "acme" -> "ACME", "local-llm" -> "LOCAL_LLM"
fn to_screaming_snake(s: &str) -> String {
    s.replace('-', "_").to_uppercase()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use nc_parser::SexpNode;

    fn parse_and_gen(input: &str) -> String {
        let sexp = SexpNode::parse_one(input).unwrap();
        let decl = ConnectorDecl::from_sexp(&sexp).unwrap();
        generate_connector(&decl)
    }

    #[test]
    fn generates_full_connector() {
        let output = parse_and_gen(
            r#"(connector acme
              :base-url "http://example.com"
              :auth api-key
              :timeout 120
              :capabilities (text vision tools stream)
              :secret-namespace "acme-shared"
              :secret-keys (("base-url" "providers/acme/base-url")
                            ("api-key" "providers/acme/api-key")))"#,
        );

        // The generated file holds only the `Connector` trait impl. The struct,
        // its constructor and secret loading live in the hand-written
        // `<name>_impl` module (Generation Gap), so they must not appear here.
        assert!(output.contains("GENERATED BY FORGE"));
        assert!(output.contains("use super::acme_impl::AcmeConnector;"));
        assert!(!output.contains("pub struct AcmeConnector"));
        assert!(!output.contains("ACME_API_KEY"));
        assert!(output.contains("impl Connector for AcmeConnector"));
        assert!(output.contains("fn name(&self)"));
        assert!(output.contains("\"acme\""));
        assert!(output.contains("text: true"));
        assert!(output.contains("vision: true"));
        assert!(output.contains("video: false"));
        assert!(output.contains("tools: true"));
        assert!(output.contains("stream: true"));
        assert!(output.contains("invoke_impl(route, req)"));

        // Brace balance
        let open = output.matches('{').count();
        let close = output.matches('}').count();
        assert_eq!(open, close, "unbalanced: {open} vs {close}");
    }

    #[test]
    fn generates_minimal_connector() {
        let output = parse_and_gen(
            r#"(connector local-llm
              :base-url "http://localhost:8080"
              :auth none
              :timeout 30
              :capabilities (text stream))"#,
        );

        assert!(output.contains("use super::local_llm_impl::LocalLlmConnector;"));
        assert!(!output.contains("pub struct LocalLlmConnector"));
        assert!(!output.contains("api_key"));
        assert!(!output.contains("bearer_token"));
        assert!(output.contains("text: true"));
        assert!(output.contains("stream: true"));
        assert!(output.contains("vision: false"));
        assert!(output.contains("impl Connector for LocalLlmConnector"));

        let open = output.matches('{').count();
        let close = output.matches('}').count();
        assert_eq!(open, close, "unbalanced: {open} vs {close}");
    }

    #[test]
    fn generates_bearer_token_connector() {
        let output = parse_and_gen(
            r#"(connector openai
              :auth bearer-token
              :capabilities (text vision tools stream)
              :secret-keys (("bearer-token" "providers/openai/token")))"#,
        );

        // Credentials are handled in the hand-written `openai_impl` module; the
        // generated trait impl is the same whatever the auth scheme.
        assert!(output.contains("use super::openai_impl::OpenaiConnector;"));
        assert!(output.contains("impl Connector for OpenaiConnector"));
        assert!(output.contains("tools: true"));
        assert!(!output.contains("bearer_token"));
        assert!(!output.contains("api_key"));
    }
}
