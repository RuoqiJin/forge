//! Atom: trait-contract
//!
//! Generates Rust trait definitions for the Generation Gap pattern.
//! The generated trait is the contract between generated code and custom.rs.
//!
//! ## What it generates
//!
//! ```text
//! /// Implement in custom.rs
//! #[async_trait]
//! pub trait BoardModuleLogic {
//!     /// Handle board creation.
//!     async fn on_create(&self, name: &str) -> Result<()>;
//!
//!     /// Default implementation provided.
//!     async fn on_delete(&self) -> Result<()> {
//!         todo!("implement on_delete in custom.rs")
//!     }
//! }
//! ```

use super::w;

/// A single method signature in a trait.
#[derive(Debug, Clone)]
pub struct TraitMethodDef {
    /// Method name, e.g., `on_create`.
    pub name: String,
    /// Parameter pairs (name, type), excluding `&self`.
    pub params: Vec<(String, String)>,
    /// Return type, e.g., `Result<()>` or `Result<Value>`.
    pub return_type: String,
    /// If true, prefixes the method with `async`.
    pub is_async: bool,
    /// Optional doc comment for the method.
    pub doc: Option<String>,
    /// If true, generates `{ todo!("implement ...") }` instead of `;`.
    pub has_default: bool,
    /// If true, prepends `&self` as the first parameter (default behavior).
    /// Set to false for pure/static functions.
    pub has_self: bool,
}

/// Input for trait generation.
pub struct TraitContractInput {
    /// Trait name, e.g., `BoardModuleLogic`.
    pub name: String,
    /// Optional doc comment for the trait.
    pub doc: Option<String>,
    /// Methods in the trait.
    pub methods: Vec<TraitMethodDef>,
    /// If true, adds `#[async_trait]` attribute before the trait.
    pub is_async_trait: bool,
}

/// Generate a trait definition.
///
/// Returns the generated code as a String fragment.
pub fn gen_trait_contract(input: &TraitContractInput) -> String {
    let mut out = String::with_capacity(512);

    // Doc comment
    if let Some(ref doc) = input.doc {
        w(&mut out, 0, &format!("/// {doc}"));
    }

    // #[async_trait] attribute
    if input.is_async_trait {
        w(&mut out, 0, "#[async_trait]");
    }

    // Trait opening
    w(&mut out, 0, &format!("pub trait {} {{", input.name));

    for (i, method) in input.methods.iter().enumerate() {
        // Blank line between methods
        if i > 0 {
            out.push('\n');
        }

        // Method doc comment
        if let Some(ref doc) = method.doc {
            w(&mut out, 1, &format!("/// {doc}"));
        }

        // Build parameter list: optionally starts with &self
        let params: Vec<String> = if method.has_self {
            std::iter::once("&self".to_string())
                .chain(method.params.iter().map(|(n, t)| format!("{n}: {t}")))
                .collect()
        } else {
            method.params.iter().map(|(n, t)| format!("{n}: {t}")).collect()
        };

        let async_kw = if method.is_async { "async " } else { "" };

        if method.has_default {
            // Method with default body
            w(
                &mut out,
                1,
                &format!(
                    "{async_kw}fn {}({}) -> {} {{",
                    method.name,
                    params.join(", "),
                    method.return_type,
                ),
            );
            w(
                &mut out,
                2,
                &format!("todo!(\"implement {} in custom.rs\")", method.name),
            );
            w(&mut out, 1, "}");
        } else {
            // Abstract method (semicolon)
            w(
                &mut out,
                1,
                &format!(
                    "{async_kw}fn {}({}) -> {};",
                    method.name,
                    params.join(", "),
                    method.return_type,
                ),
            );
        }
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
    fn basic_async_trait() {
        let input = TraitContractInput {
            name: "BoardLogic".into(),
            doc: Some("Implement in custom.rs".into()),
            is_async_trait: true,
            methods: vec![
                TraitMethodDef {
                    name: "on_create".into(),
                    params: vec![("name".into(), "&str".into())],
                    return_type: "Result<()>".into(),
                    is_async: true,
                    doc: Some("Handle board creation.".into()),
                    has_default: false,
                    has_self: true,
                },
                TraitMethodDef {
                    name: "on_delete".into(),
                    params: vec![("id".into(), "i64".into())],
                    return_type: "Result<()>".into(),
                    is_async: true,
                    doc: None,
                    has_default: false,
                    has_self: true,
                },
            ],
        };

        let code = gen_trait_contract(&input);

        assert!(code.contains("/// Implement in custom.rs"));
        assert!(code.contains("#[async_trait]"));
        assert!(code.contains("pub trait BoardLogic {"));
        assert!(code.contains("/// Handle board creation."));
        assert!(code.contains("async fn on_create(&self, name: &str) -> Result<()>;"));
        assert!(code.contains("async fn on_delete(&self, id: i64) -> Result<()>;"));

        let open = code.matches('{').count();
        let close = code.matches('}').count();
        assert_eq!(open, close, "unbalanced: {open} vs {close}");
    }

    #[test]
    fn sync_trait_no_async_attribute() {
        let input = TraitContractInput {
            name: "Validator".into(),
            doc: None,
            is_async_trait: false,
            methods: vec![TraitMethodDef {
                name: "validate".into(),
                params: vec![("input".into(), "&str".into())],
                return_type: "bool".into(),
                is_async: false,
                doc: None,
                has_default: false,
                has_self: true,
            }],
        };

        let code = gen_trait_contract(&input);

        assert!(!code.contains("#[async_trait]"));
        assert!(code.contains("pub trait Validator {"));
        assert!(code.contains("fn validate(&self, input: &str) -> bool;"));
        // Should NOT have async keyword
        assert!(!code.contains("async fn validate"));
    }

    #[test]
    fn method_with_default() {
        let input = TraitContractInput {
            name: "WorkerLogic".into(),
            doc: None,
            is_async_trait: true,
            methods: vec![
                TraitMethodDef {
                    name: "on_tick".into(),
                    params: vec![],
                    return_type: "Result<()>".into(),
                    is_async: true,
                    doc: Some("Called every interval.".into()),
                    has_default: false,
                    has_self: true,
                },
                TraitMethodDef {
                    name: "on_shutdown".into(),
                    params: vec![],
                    return_type: "Result<()>".into(),
                    is_async: true,
                    doc: Some("Optional cleanup.".into()),
                    has_default: true,
                    has_self: true,
                },
            ],
        };

        let code = gen_trait_contract(&input);

        // on_tick: abstract (semicolon)
        assert!(code.contains("async fn on_tick(&self) -> Result<()>;"));

        // on_shutdown: default body
        assert!(code.contains("async fn on_shutdown(&self) -> Result<()> {"));
        assert!(code.contains("todo!(\"implement on_shutdown in custom.rs\")"));

        let open = code.matches('{').count();
        let close = code.matches('}').count();
        assert_eq!(open, close, "unbalanced: {open} vs {close}");
    }

    #[test]
    fn empty_methods() {
        let input = TraitContractInput {
            name: "EmptyTrait".into(),
            doc: None,
            is_async_trait: false,
            methods: vec![],
        };

        let code = gen_trait_contract(&input);

        assert!(code.contains("pub trait EmptyTrait {"));
        assert!(code.contains("}"));

        let open = code.matches('{').count();
        let close = code.matches('}').count();
        assert_eq!(open, close);
    }

    #[test]
    fn mixed_async_sync_methods() {
        let input = TraitContractInput {
            name: "GatewayLogic".into(),
            doc: None,
            is_async_trait: true,
            methods: vec![
                TraitMethodDef {
                    name: "route".into(),
                    params: vec![("req".into(), "&Request".into())],
                    return_type: "Result<Response>".into(),
                    is_async: true,
                    doc: None,
                    has_default: false,
                    has_self: true,
                },
                TraitMethodDef {
                    name: "estimate_cost".into(),
                    params: vec![("tokens".into(), "usize".into())],
                    return_type: "f64".into(),
                    is_async: false,
                    doc: None,
                    has_default: false,
                    has_self: true,
                },
            ],
        };

        let code = gen_trait_contract(&input);

        assert!(code.contains("async fn route(&self, req: &Request) -> Result<Response>;"));
        assert!(code.contains("fn estimate_cost(&self, tokens: usize) -> f64;"));
        // estimate_cost should NOT be async
        assert!(!code.contains("async fn estimate_cost"));
    }

    #[test]
    fn pure_function_no_self() {
        let input = TraitContractInput {
            name: "MathOps".into(),
            doc: None,
            is_async_trait: false,
            methods: vec![
                TraitMethodDef {
                    name: "add".into(),
                    params: vec![("a".into(), "i32".into()), ("b".into(), "i32".into())],
                    return_type: "i32".into(),
                    is_async: false,
                    doc: Some("Add two numbers.".into()),
                    has_default: false,
                    has_self: false,
                },
                TraitMethodDef {
                    name: "negate".into(),
                    params: vec![("x".into(), "i32".into())],
                    return_type: "i32".into(),
                    is_async: false,
                    doc: None,
                    has_default: false,
                    has_self: false,
                },
            ],
        };

        let code = gen_trait_contract(&input);

        // Pure functions must NOT have &self
        assert!(!code.contains("&self"));
        assert!(code.contains("fn add(a: i32, b: i32) -> i32;"));
        assert!(code.contains("fn negate(x: i32) -> i32;"));
        assert!(code.contains("/// Add two numbers."));
    }

    #[test]
    fn no_params_method() {
        let input = TraitContractInput {
            name: "Lifecycle".into(),
            doc: None,
            is_async_trait: false,
            methods: vec![TraitMethodDef {
                name: "init".into(),
                params: vec![],
                return_type: "Result<()>".into(),
                is_async: false,
                doc: None,
                has_default: true,
                has_self: true,
            }],
        };

        let code = gen_trait_contract(&input);

        assert!(code.contains("fn init(&self) -> Result<()> {"));
        assert!(code.contains("todo!(\"implement init in custom.rs\")"));
    }
}
