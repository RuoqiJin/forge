//! Atom: impl-block
//!
//! Generates `impl TypeName { ... }` blocks.
//! Methods are provided as pre-formatted code fragments.
//!
//! ## What it generates
//!
//! ```text
//! impl TypeName {
//!     /// doc
//!     pub fn as_str(&self) -> &'static str {
//!         body
//!     }
//! }
//! ```
//!
//! Or with a trait:
//!
//! ```text
//! impl Default for TypeName {
//!     fn default() -> Self { ... }
//! }
//! ```

use super::w;

/// A single method in an impl block.
#[derive(Debug, Clone)]
pub struct MethodDef {
    /// Full method signature, e.g., `pub fn as_str(&self) -> &'static str`
    pub signature: String,
    /// Pre-formatted method body (lines joined with \n).
    pub body: String,
    /// Optional doc comment for the method.
    pub doc: Option<String>,
    /// Method-level attributes, e.g., `#[instrument(skip(self))]`.
    /// Each string is a complete attribute line.
    pub attributes: Vec<String>,
}

/// Input for generating an impl block.
pub struct ImplBlockInput {
    /// The type being implemented, e.g., `SessionState`.
    pub type_name: String,
    /// If Some, generates `impl TraitName for TypeName {`.
    pub trait_name: Option<String>,
    /// Methods in the impl block.
    pub methods: Vec<MethodDef>,
}

/// Generate an `impl` block with the given methods.
///
/// Returns the generated code as a String fragment.
pub fn gen_impl_block(input: &ImplBlockInput) -> String {
    let mut out = String::with_capacity(512);

    // Opening line
    match input.trait_name {
        Some(ref trait_name) => {
            w(
                &mut out,
                0,
                &format!("impl {trait_name} for {} {{", input.type_name),
            );
        }
        None => {
            w(&mut out, 0, &format!("impl {} {{", input.type_name));
        }
    }

    for (i, method) in input.methods.iter().enumerate() {
        // Blank line between methods
        if i > 0 {
            out.push('\n');
        }

        // Method-level attributes
        for attr in &method.attributes {
            w(&mut out, 1, attr);
        }

        // Doc comment
        if let Some(ref doc) = method.doc {
            w(&mut out, 1, &format!("/// {doc}"));
        }

        // Signature
        w(&mut out, 1, &format!("{} {{", method.signature));

        // Body — indent each line by 2 levels
        for line in method.body.lines() {
            if line.is_empty() {
                out.push('\n');
            } else {
                w(&mut out, 2, line);
            }
        }

        // Close method
        w(&mut out, 1, "}");
    }

    w(&mut out, 0, "}");
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_inherent_impl() {
        let input = ImplBlockInput {
            type_name: "SessionState".into(),
            trait_name: None,
            methods: vec![MethodDef {
                signature: "pub fn as_str(&self) -> &'static str".into(),
                body: "match self {\n    Self::Pending => \"pending\",\n    Self::Active => \"active\",\n}".into(),
                doc: Some("Returns the string representation.".into()),
                attributes: vec![],
            }],
        };

        let code = gen_impl_block(&input);

        assert!(code.contains("impl SessionState {"));
        assert!(code.contains("/// Returns the string representation."));
        assert!(code.contains("pub fn as_str(&self) -> &'static str {"));
        assert!(code.contains("match self {"));
        assert!(code.contains("Self::Pending => \"pending\","));
        assert!(code.contains("Self::Active => \"active\","));

        // Brace balance
        let open = code.matches('{').count();
        let close = code.matches('}').count();
        assert_eq!(open, close, "unbalanced: {open} vs {close}");
    }

    #[test]
    fn trait_impl() {
        let input = ImplBlockInput {
            type_name: "SessionState".into(),
            trait_name: Some("Default".into()),
            methods: vec![MethodDef {
                signature: "fn default() -> Self".into(),
                body: "Self::Pending".into(),
                doc: None,
                attributes: vec![],
            }],
        };

        let code = gen_impl_block(&input);

        assert!(code.contains("impl Default for SessionState {"));
        assert!(code.contains("fn default() -> Self {"));
        assert!(code.contains("Self::Pending"));

        let open = code.matches('{').count();
        let close = code.matches('}').count();
        assert_eq!(open, close);
    }

    #[test]
    fn multiple_methods() {
        let input = ImplBlockInput {
            type_name: "ErrorCode".into(),
            trait_name: None,
            methods: vec![
                MethodDef {
                    signature: "pub fn code(&self) -> u32".into(),
                    body: "42".into(),
                    doc: Some("Returns numeric code.".into()),
                    attributes: vec![],
                },
                MethodDef {
                    signature: "pub fn message(&self) -> &'static str".into(),
                    body: "\"error\"".into(),
                    doc: Some("Returns error message.".into()),
                    attributes: vec![],
                },
            ],
        };

        let code = gen_impl_block(&input);

        assert!(code.contains("/// Returns numeric code."));
        assert!(code.contains("pub fn code(&self) -> u32 {"));
        assert!(code.contains("/// Returns error message."));
        assert!(code.contains("pub fn message(&self) -> &'static str {"));

        // Should have a blank line between methods
        assert!(code.contains("}\n\n"));
    }

    #[test]
    fn empty_methods() {
        let input = ImplBlockInput {
            type_name: "Unit".into(),
            trait_name: None,
            methods: vec![],
        };

        let code = gen_impl_block(&input);

        assert!(code.contains("impl Unit {"));
        assert!(code.contains("}"));

        let open = code.matches('{').count();
        let close = code.matches('}').count();
        assert_eq!(open, close);
    }

    #[test]
    fn display_trait_impl() {
        let input = ImplBlockInput {
            type_name: "MyError".into(),
            trait_name: Some("std::fmt::Display".into()),
            methods: vec![MethodDef {
                signature: "fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result".into(),
                body: "write!(f, \"error: {}\", self.message())".into(),
                doc: None,
                attributes: vec![],
            }],
        };

        let code = gen_impl_block(&input);

        assert!(code.contains("impl std::fmt::Display for MyError {"));
        assert!(code.contains("fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {"));
        assert!(code.contains("write!(f, \"error: {}\", self.message())"));
    }

    #[test]
    fn method_with_attributes() {
        let input = ImplBlockInput {
            type_name: "Service".into(),
            trait_name: None,
            methods: vec![MethodDef {
                signature: "pub async fn handle(&self, req: Request) -> Result<Response>".into(),
                body: "todo!()".into(),
                doc: Some("Handle a request.".into()),
                attributes: vec![
                    "#[instrument(skip(self))]".into(),
                    "#[tracing::instrument(level = \"debug\")]".into(),
                ],
            }],
        };

        let code = gen_impl_block(&input);

        assert!(code.contains("/// Handle a request."));
        assert!(code.contains("#[instrument(skip(self))]"));
        assert!(code.contains("#[tracing::instrument(level = \"debug\")]"));
        assert!(code.contains("pub async fn handle(&self, req: Request) -> Result<Response> {"));

        // Attributes should appear before the doc comment and signature
        let attr_pos = code.find("#[instrument(skip(self))]").unwrap();
        let doc_pos = code.find("/// Handle a request.").unwrap();
        let sig_pos = code.find("pub async fn handle").unwrap();
        assert!(attr_pos < doc_pos, "attributes must come before doc comment");
        assert!(doc_pos < sig_pos, "doc comment must come before signature");

        let open = code.matches('{').count();
        let close = code.matches('}').count();
        assert_eq!(open, close, "unbalanced: {open} vs {close}");
    }

    #[test]
    fn multiline_body_indentation() {
        let input = ImplBlockInput {
            type_name: "Config".into(),
            trait_name: None,
            methods: vec![MethodDef {
                signature: "pub fn validate(&self) -> Result<()>".into(),
                body: "if self.name.is_empty() {\n    anyhow::bail!(\"name required\");\n}\nOk(())".into(),
                doc: None,
                attributes: vec![],
            }],
        };

        let code = gen_impl_block(&input);

        // Each body line should be at indent level 2 (8 spaces)
        assert!(code.contains("        if self.name.is_empty() {"));
        assert!(code.contains("            anyhow::bail!(\"name required\");"));
        assert!(code.contains("        }"));
        assert!(code.contains("        Ok(())"));
    }
}
