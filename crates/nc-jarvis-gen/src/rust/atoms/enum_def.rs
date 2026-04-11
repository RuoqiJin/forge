//! Atom: enum-def
//!
//! Generates a Rust enum with optional Display + FromStr implementations.
//! Used for status enums, error types, domain_types, and whitelist values.
//!
//! ## Two styles
//!
//! **Style A — Trait impls** (`derive_display` / `derive_from_str`):
//! ```text
//! impl std::fmt::Display for Status { ... }
//! impl std::str::FromStr for Status { ... }
//! ```
//!
//! **Style B — Method impls** (`gen_as_str_method` / `gen_from_str_method`):
//! ```text
//! impl Status {
//!     pub fn as_str(&self) -> &'static str { ... }
//!     pub fn from_str(s: &str) -> Option<Self> { ... }
//! }
//! ```

use super::w;
use crate::quarks::derive_attr::DeriveSet;
use crate::quarks::doc_comment::{DocLang, doc_line};

/// A single variant in an enum.
#[derive(Debug, Clone)]
pub struct VariantDef {
    pub name: String,
    pub doc: Option<String>,
    /// String representation for Display/FromStr (default: snake_case of name).
    pub str_value: Option<String>,
}

/// Input to the enum-def atom.
pub struct EnumDefInput {
    pub name: String,
    pub variants: Vec<VariantDef>,
    /// Derive traits for the enum. Replaces the old hardcoded derive line.
    pub derives: DeriveSet,
    pub derive_display: bool,
    pub derive_from_str: bool,
    pub doc: Option<String>,
    /// When set, generates `#[serde(rename_all = "...")]` after the derive line.
    pub serde_rename_all: Option<String>,
    /// Arbitrary extra enum-level attributes (e.g. `#[non_exhaustive]`).
    pub extra_enum_attrs: Vec<String>,
    /// Style B: generate `pub fn as_str(&self) -> &'static str` method.
    pub gen_as_str_method: bool,
    /// Style B: generate `pub fn from_str(s: &str) -> Option<Self>` method.
    pub gen_from_str_method: bool,
    /// Default variant name for the from_str catch-all arm.
    /// When set, returns `Some(Self::Variant)` / `Ok(Self::Variant)` instead of `None` / `Err`.
    pub from_str_default: Option<String>,
}

/// Generate a Rust enum definition with optional Display + FromStr.
pub fn gen_enum_def(input: &EnumDefInput) -> String {
    let mut out = String::with_capacity(512);

    // Doc comment
    if let Some(ref doc) = input.doc {
        w(&mut out, 0, &doc_line(DocLang::Rust, doc));
    }

    // Derive
    let derive_line = input.derives.to_derive_line();
    if !derive_line.is_empty() {
        w(&mut out, 0, &derive_line);
    }

    // serde rename_all
    if let Some(ref rename) = input.serde_rename_all {
        w(
            &mut out,
            0,
            &format!("#[serde(rename_all = \"{}\")]", rename),
        );
    }

    // Extra enum-level attributes
    for attr in &input.extra_enum_attrs {
        w(&mut out, 0, attr);
    }

    // Enum definition
    w(&mut out, 0, &format!("pub enum {} {{", input.name));
    for v in &input.variants {
        if let Some(ref doc) = v.doc {
            w(&mut out, 1, &doc_line(DocLang::Rust, doc));
        }
        w(&mut out, 1, &format!("{},", v.name));
    }
    w(&mut out, 0, "}");
    w(&mut out, 0, "");

    // Display impl (Style A)
    if input.derive_display {
        w(
            &mut out,
            0,
            &format!("impl std::fmt::Display for {} {{", input.name),
        );
        w(
            &mut out,
            1,
            "fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {",
        );
        w(&mut out, 2, "match self {");
        for v in &input.variants {
            let str_val = variant_str_value(v);
            w(
                &mut out,
                3,
                &format!("Self::{} => write!(f, \"{}\"),", v.name, str_val),
            );
        }
        w(&mut out, 2, "}");
        w(&mut out, 1, "}");
        w(&mut out, 0, "}");
        w(&mut out, 0, "");
    }

    // FromStr impl (Style A)
    if input.derive_from_str {
        w(
            &mut out,
            0,
            &format!("impl std::str::FromStr for {} {{", input.name),
        );
        w(&mut out, 1, "type Err = String;");
        w(&mut out, 0, "");
        w(
            &mut out,
            1,
            "fn from_str(s: &str) -> Result<Self, Self::Err> {",
        );
        w(&mut out, 2, "match s {");
        for v in &input.variants {
            let str_val = variant_str_value(v);
            w(
                &mut out,
                3,
                &format!("\"{}\" => Ok(Self::{}),", str_val, v.name),
            );
        }
        // Catch-all arm
        if let Some(ref default) = input.from_str_default {
            w(
                &mut out,
                3,
                &format!("_ => Ok(Self::{}),", default),
            );
        } else {
            w(
                &mut out,
                3,
                &format!(
                    "other => Err(format!(\"unknown {}: {{other}}\",)),",
                    to_snake_case(&input.name)
                ),
            );
        }
        w(&mut out, 2, "}");
        w(&mut out, 1, "}");
        w(&mut out, 0, "}");
        w(&mut out, 0, "");
    }

    // Style B: as_str + from_str methods in a single impl block
    if input.gen_as_str_method || input.gen_from_str_method {
        w(&mut out, 0, &format!("impl {} {{", input.name));

        // as_str method
        if input.gen_as_str_method {
            w(
                &mut out,
                1,
                "pub fn as_str(&self) -> &'static str {",
            );
            w(&mut out, 2, "match self {");
            for v in &input.variants {
                let str_val = variant_str_value(v);
                w(
                    &mut out,
                    3,
                    &format!("Self::{} => \"{}\",", v.name, str_val),
                );
            }
            w(&mut out, 2, "}");
            w(&mut out, 1, "}");

            // Add blank line between methods when both are present
            if input.gen_from_str_method {
                w(&mut out, 0, "");
            }
        }

        // from_str method (returns Option<Self>)
        if input.gen_from_str_method {
            w(
                &mut out,
                1,
                "pub fn from_str(s: &str) -> Option<Self> {",
            );
            w(&mut out, 2, "match s {");
            for v in &input.variants {
                let str_val = variant_str_value(v);
                w(
                    &mut out,
                    3,
                    &format!("\"{}\" => Some(Self::{}),", str_val, v.name),
                );
            }
            // Catch-all arm
            if let Some(ref default) = input.from_str_default {
                w(
                    &mut out,
                    3,
                    &format!("_ => Some(Self::{}),", default),
                );
            } else {
                w(&mut out, 3, "_ => None,");
            }
            w(&mut out, 2, "}");
            w(&mut out, 1, "}");
        }

        w(&mut out, 0, "}");
        w(&mut out, 0, "");
    }

    out
}

/// Resolve the string value for a variant: explicit `str_value` or snake_case fallback.
fn variant_str_value(v: &VariantDef) -> String {
    v.str_value
        .as_deref()
        .map(|s| s.to_string())
        .unwrap_or_else(|| to_snake_case(&v.name))
}

use crate::quarks::ident::to_snake_case;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: build a DeriveSet matching the old hardcoded derive line
    /// `#[derive(Debug, Clone, Copy, PartialEq, Eq)]`.
    fn enum_default_derives() -> DeriveSet {
        DeriveSet {
            debug: true,
            clone: true,
            partial_eq: true,
            custom: vec!["Copy".into(), "Eq".into()],
            ..Default::default()
        }
    }

    #[test]
    fn basic_enum() {
        let input = EnumDefInput {
            name: "DeployStatus".into(),
            variants: vec![
                VariantDef { name: "Pending".into(), doc: None, str_value: None },
                VariantDef { name: "Running".into(), doc: None, str_value: None },
                VariantDef { name: "Success".into(), doc: None, str_value: None },
                VariantDef { name: "Failed".into(), doc: None, str_value: None },
            ],
            derives: enum_default_derives(),
            derive_display: true,
            derive_from_str: true,
            doc: Some("Deployment execution status".into()),
            serde_rename_all: None,
            extra_enum_attrs: vec![],
            gen_as_str_method: false,
            gen_from_str_method: false,
            from_str_default: None,
        };

        let code = gen_enum_def(&input);

        assert!(code.contains("/// Deployment execution status"));
        assert!(code.contains("#[derive(Debug, Clone, PartialEq, Copy, Eq)]"));
        assert!(code.contains("pub enum DeployStatus {"));
        assert!(code.contains("Pending,"));
        assert!(code.contains("Running,"));

        // Display
        assert!(code.contains("impl std::fmt::Display for DeployStatus"));
        assert!(code.contains("Self::Pending => write!(f, \"pending\")"));
        assert!(code.contains("Self::Running => write!(f, \"running\")"));

        // FromStr
        assert!(code.contains("impl std::str::FromStr for DeployStatus"));
        assert!(code.contains("\"pending\" => Ok(Self::Pending)"));
        assert!(code.contains("\"failed\" => Ok(Self::Failed)"));
    }

    #[test]
    fn custom_str_values() {
        let input = EnumDefInput {
            name: "Priority".into(),
            variants: vec![
                VariantDef { name: "Low".into(), doc: None, str_value: Some("-1".into()) },
                VariantDef { name: "Normal".into(), doc: None, str_value: Some("0".into()) },
                VariantDef { name: "High".into(), doc: None, str_value: Some("1".into()) },
            ],
            derives: enum_default_derives(),
            derive_display: true,
            derive_from_str: true,
            doc: None,
            serde_rename_all: None,
            extra_enum_attrs: vec![],
            gen_as_str_method: false,
            gen_from_str_method: false,
            from_str_default: None,
        };

        let code = gen_enum_def(&input);
        assert!(code.contains("Self::Low => write!(f, \"-1\")"));
        assert!(code.contains("\"-1\" => Ok(Self::Low)"));
        assert!(code.contains("\"1\" => Ok(Self::High)"));
    }

    #[test]
    fn no_display_no_fromstr() {
        let input = EnumDefInput {
            name: "Simple".into(),
            variants: vec![
                VariantDef { name: "A".into(), doc: None, str_value: None },
                VariantDef { name: "B".into(), doc: None, str_value: None },
            ],
            derives: enum_default_derives(),
            derive_display: false,
            derive_from_str: false,
            doc: None,
            serde_rename_all: None,
            extra_enum_attrs: vec![],
            gen_as_str_method: false,
            gen_from_str_method: false,
            from_str_default: None,
        };

        let code = gen_enum_def(&input);
        assert!(code.contains("pub enum Simple {"));
        assert!(!code.contains("impl std::fmt::Display"));
        assert!(!code.contains("impl std::str::FromStr"));
    }

    #[test]
    fn snake_case_conversion() {
        assert_eq!(to_snake_case("DeployStatus"), "deploy_status");
        assert_eq!(to_snake_case("Pending"), "pending");
        assert_eq!(to_snake_case("LostContact"), "lost_contact");
        assert_eq!(to_snake_case("SSEStream"), "s_s_e_stream");
    }

    // --- New capability tests ---

    #[test]
    fn serde_rename_all() {
        let input = EnumDefInput {
            name: "Color".into(),
            variants: vec![
                VariantDef { name: "Red".into(), doc: None, str_value: None },
                VariantDef { name: "Blue".into(), doc: None, str_value: None },
            ],
            derives: DeriveSet {
                debug: true,
                clone: true,
                serialize: true,
                deserialize: true,
                partial_eq: false,
                custom: vec![],
            },
            derive_display: false,
            derive_from_str: false,
            doc: None,
            serde_rename_all: Some("lowercase".into()),
            extra_enum_attrs: vec![],
            gen_as_str_method: false,
            gen_from_str_method: false,
            from_str_default: None,
        };

        let code = gen_enum_def(&input);
        assert!(code.contains("#[derive(Debug, Clone, Serialize, Deserialize)]"));
        assert!(code.contains("#[serde(rename_all = \"lowercase\")]"));
        assert!(code.contains("pub enum Color {"));
    }

    #[test]
    fn extra_enum_attrs() {
        let input = EnumDefInput {
            name: "Visibility".into(),
            variants: vec![
                VariantDef { name: "Public".into(), doc: None, str_value: None },
                VariantDef { name: "Private".into(), doc: None, str_value: None },
            ],
            derives: enum_default_derives(),
            derive_display: false,
            derive_from_str: false,
            doc: None,
            serde_rename_all: None,
            extra_enum_attrs: vec!["#[non_exhaustive]".into()],
            gen_as_str_method: false,
            gen_from_str_method: false,
            from_str_default: None,
        };

        let code = gen_enum_def(&input);
        assert!(code.contains("#[non_exhaustive]"));
        assert!(code.contains("pub enum Visibility {"));
    }

    #[test]
    fn as_str_method() {
        let input = EnumDefInput {
            name: "Lang".into(),
            variants: vec![
                VariantDef { name: "Rust".into(), doc: None, str_value: Some("rust".into()) },
                VariantDef { name: "Python".into(), doc: None, str_value: Some("python".into()) },
                VariantDef { name: "TypeScript".into(), doc: None, str_value: Some("typescript".into()) },
            ],
            derives: enum_default_derives(),
            derive_display: false,
            derive_from_str: false,
            doc: None,
            serde_rename_all: None,
            extra_enum_attrs: vec![],
            gen_as_str_method: true,
            gen_from_str_method: false,
            from_str_default: None,
        };

        let code = gen_enum_def(&input);
        assert!(code.contains("impl Lang {"));
        assert!(code.contains("pub fn as_str(&self) -> &'static str {"));
        assert!(code.contains("Self::Rust => \"rust\","));
        assert!(code.contains("Self::Python => \"python\","));
        assert!(code.contains("Self::TypeScript => \"typescript\","));
        // Should NOT generate Display trait
        assert!(!code.contains("impl std::fmt::Display"));
    }

    #[test]
    fn from_str_method() {
        let input = EnumDefInput {
            name: "Lang".into(),
            variants: vec![
                VariantDef { name: "Rust".into(), doc: None, str_value: Some("rust".into()) },
                VariantDef { name: "Python".into(), doc: None, str_value: Some("python".into()) },
            ],
            derives: enum_default_derives(),
            derive_display: false,
            derive_from_str: false,
            doc: None,
            serde_rename_all: None,
            extra_enum_attrs: vec![],
            gen_as_str_method: false,
            gen_from_str_method: true,
            from_str_default: None,
        };

        let code = gen_enum_def(&input);
        assert!(code.contains("impl Lang {"));
        assert!(code.contains("pub fn from_str(s: &str) -> Option<Self> {"));
        assert!(code.contains("\"rust\" => Some(Self::Rust),"));
        assert!(code.contains("\"python\" => Some(Self::Python),"));
        assert!(code.contains("_ => None,"));
        // Should NOT generate FromStr trait
        assert!(!code.contains("impl std::str::FromStr"));
    }

    #[test]
    fn as_str_and_from_str_methods_combined() {
        let input = EnumDefInput {
            name: "Role".into(),
            variants: vec![
                VariantDef { name: "Admin".into(), doc: None, str_value: Some("admin".into()) },
                VariantDef { name: "User".into(), doc: None, str_value: Some("user".into()) },
                VariantDef { name: "Guest".into(), doc: None, str_value: Some("guest".into()) },
            ],
            derives: enum_default_derives(),
            derive_display: false,
            derive_from_str: false,
            doc: None,
            serde_rename_all: None,
            extra_enum_attrs: vec![],
            gen_as_str_method: true,
            gen_from_str_method: true,
            from_str_default: None,
        };

        let code = gen_enum_def(&input);
        // Both methods in a single impl block
        assert!(code.contains("impl Role {"));
        assert!(code.contains("pub fn as_str(&self) -> &'static str {"));
        assert!(code.contains("pub fn from_str(s: &str) -> Option<Self> {"));
        // Only one impl block (no "impl Role {" appearing twice)
        assert_eq!(code.matches("impl Role {").count(), 1);
    }

    #[test]
    fn from_str_default_with_trait() {
        let input = EnumDefInput {
            name: "Level".into(),
            variants: vec![
                VariantDef { name: "Info".into(), doc: None, str_value: Some("info".into()) },
                VariantDef { name: "Warn".into(), doc: None, str_value: Some("warn".into()) },
                VariantDef { name: "Error".into(), doc: None, str_value: Some("error".into()) },
            ],
            derives: enum_default_derives(),
            derive_display: false,
            derive_from_str: true,
            doc: None,
            serde_rename_all: None,
            extra_enum_attrs: vec![],
            gen_as_str_method: false,
            gen_from_str_method: false,
            from_str_default: Some("Info".into()),
        };

        let code = gen_enum_def(&input);
        assert!(code.contains("impl std::str::FromStr for Level"));
        assert!(code.contains("_ => Ok(Self::Info),"));
        // Should NOT contain the error format arm
        assert!(!code.contains("Err(format!"));
    }

    #[test]
    fn from_str_default_with_method() {
        let input = EnumDefInput {
            name: "Level".into(),
            variants: vec![
                VariantDef { name: "Info".into(), doc: None, str_value: Some("info".into()) },
                VariantDef { name: "Warn".into(), doc: None, str_value: Some("warn".into()) },
            ],
            derives: enum_default_derives(),
            derive_display: false,
            derive_from_str: false,
            doc: None,
            serde_rename_all: None,
            extra_enum_attrs: vec![],
            gen_as_str_method: false,
            gen_from_str_method: true,
            from_str_default: Some("Info".into()),
        };

        let code = gen_enum_def(&input);
        assert!(code.contains("pub fn from_str(s: &str) -> Option<Self> {"));
        assert!(code.contains("_ => Some(Self::Info),"));
        assert!(!code.contains("_ => None,"));
    }

    #[test]
    fn as_str_snake_case_fallback() {
        let input = EnumDefInput {
            name: "TaskState".into(),
            variants: vec![
                VariantDef { name: "InProgress".into(), doc: None, str_value: None },
                VariantDef { name: "Done".into(), doc: None, str_value: None },
            ],
            derives: enum_default_derives(),
            derive_display: false,
            derive_from_str: false,
            doc: None,
            serde_rename_all: None,
            extra_enum_attrs: vec![],
            gen_as_str_method: true,
            gen_from_str_method: true,
            from_str_default: None,
        };

        let code = gen_enum_def(&input);
        // Fallback to snake_case when str_value is None
        assert!(code.contains("Self::InProgress => \"in_progress\","));
        assert!(code.contains("Self::Done => \"done\","));
        assert!(code.contains("\"in_progress\" => Some(Self::InProgress),"));
    }
}
