//! Atom: struct-def
//!
//! Generates a Rust struct definition with typed fields and optional derive macros.
//! This is the most fundamental atom — almost every pattern needs struct definitions.
//!
//! ## What it generates
//!
//! ```text
//! #[derive(Debug, Clone, Deserialize)]
//! pub struct MyArgs {
//!     pub name: String,
//!     #[serde(default)]
//!     pub limit: Option<i32>,
//! }
//! ```
//!
//! ## What it does NOT generate
//!
//! - No impl blocks (that's other atoms or custom.rs)
//! - No trait definitions (that's domain-engine or pure-utility)
//! - No methods — just the struct shape

use super::w;
use crate::quarks::doc_comment::{DocLang, doc_line};

// DeriveSet is now a quark — re-export for backward compatibility
pub use crate::quarks::derive_attr::DeriveSet;

/// A single field in a struct definition.
#[derive(Debug, Clone)]
pub struct FieldDef {
    pub name: String,
    pub rust_type: String,
    pub visibility: Visibility,
    pub doc: Option<String>,
    pub serde_rename: Option<String>,
    pub serde_default: bool,
    pub serde_skip_serializing_if: Option<String>,
    /// When Some("fn_name"), generates `#[serde(default = "fn_name")]`.
    /// Mutually exclusive with `serde_default` (if both set, `serde_default_fn` wins).
    pub serde_default_fn: Option<String>,
    /// Arbitrary extra field-level attributes, each a complete line like `#[sqlx(rename_all = "snake_case")]`.
    pub extra_field_attrs: Vec<String>,
}

/// Field visibility level.
#[derive(Debug, Clone, PartialEq)]
pub enum Visibility {
    Pub,
    PubCrate,
    Private,
}

/// Input to the struct-def atom.
pub struct StructDefInput {
    pub name: String,
    pub fields: Vec<FieldDef>,
    pub derives: DeriveSet,
    pub doc: Option<String>,
    /// When Some, generates `#[serde(rename_all = "...")]` on the struct.
    pub serde_rename_all: Option<String>,
    /// Arbitrary extra struct-level attributes. Each string is a complete attribute line
    /// like `#[sqlx(rename_all = "snake_case")]`.
    pub extra_struct_attrs: Vec<String>,
    /// When true, generates a `Default` trait impl block after the struct definition.
    pub default_impl: bool,
}

/// Generate a Rust struct definition.
///
/// Returns the generated code as a String fragment (no imports — caller manages those).
pub fn gen_struct_def(input: &StructDefInput) -> String {
    let mut out = String::with_capacity(512);

    // Doc comment
    if let Some(ref doc) = input.doc {
        w(&mut out, 0, &doc_line(DocLang::Rust, doc));
    }

    // Derive line
    let derive_line = input.derives.to_derive_line();
    if !derive_line.is_empty() {
        w(&mut out, 0, &derive_line);
    }

    // serde rename_all (struct-level)
    if let Some(ref rename) = input.serde_rename_all {
        w(&mut out, 0, &format!("#[serde(rename_all = \"{rename}\")]"));
    }

    // Extra struct-level attributes
    for attr in &input.extra_struct_attrs {
        w(&mut out, 0, attr);
    }

    // Struct opening
    w(&mut out, 0, &format!("pub struct {} {{", input.name));

    // Fields
    for field in &input.fields {
        // Doc comment
        if let Some(ref doc) = field.doc {
            w(&mut out, 1, &doc_line(DocLang::Rust, doc));
        }

        // Serde attributes
        if let Some(ref rename) = field.serde_rename {
            w(&mut out, 1, &format!("#[serde(rename = \"{rename}\")]"));
        }
        if let Some(ref default_fn) = field.serde_default_fn {
            w(&mut out, 1, &format!("#[serde(default = \"{default_fn}\")]"));
        } else if field.serde_default {
            w(&mut out, 1, "#[serde(default)]");
        }
        if let Some(ref skip) = field.serde_skip_serializing_if {
            w(
                &mut out,
                1,
                &format!("#[serde(skip_serializing_if = \"{skip}\")]"),
            );
        }

        // Extra field-level attributes
        for attr in &field.extra_field_attrs {
            w(&mut out, 1, attr);
        }

        // Field declaration
        let vis = match field.visibility {
            Visibility::Pub => "pub ",
            Visibility::PubCrate => "pub(crate) ",
            Visibility::Private => "",
        };
        w(
            &mut out,
            1,
            &format!("{vis}{}: {},", field.name, field.rust_type),
        );
    }

    w(&mut out, 0, "}");

    // Default impl
    if input.default_impl {
        w(&mut out, 0, "");
        w(&mut out, 0, &format!("impl Default for {} {{", input.name));
        w(&mut out, 1, "fn default() -> Self {");
        w(&mut out, 2, "Self {");
        for field in &input.fields {
            let default_val = if let Some(ref dfn) = field.serde_default_fn {
                format!("{}()", dfn)
            } else if field.rust_type.starts_with("Option<") {
                "None".to_string()
            } else if field.rust_type == "String" {
                "String::new()".to_string()
            } else if field.rust_type == "bool" {
                "false".to_string()
            } else if field.rust_type == "i32"
                || field.rust_type == "i64"
                || field.rust_type == "u32"
                || field.rust_type == "u64"
            {
                "0".to_string()
            } else if field.rust_type == "f64" {
                "0.0".to_string()
            } else {
                "Default::default()".to_string()
            };
            w(&mut out, 3, &format!("{}: {},", field.name, default_val));
        }
        w(&mut out, 2, "}");
        w(&mut out, 1, "}");
        w(&mut out, 0, "}");
    }

    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: create a FieldDef with only the required fields set, new optional fields defaulted.
    fn field(name: &str, rust_type: &str) -> FieldDef {
        FieldDef {
            name: name.into(),
            rust_type: rust_type.into(),
            visibility: Visibility::Pub,
            doc: None,
            serde_rename: None,
            serde_default: false,
            serde_skip_serializing_if: None,
            serde_default_fn: None,
            extra_field_attrs: vec![],
        }
    }

    #[test]
    fn basic_struct() {
        let input = StructDefInput {
            name: "UserArgs".into(),
            fields: vec![
                FieldDef {
                    name: "name".into(),
                    rust_type: "String".into(),
                    visibility: Visibility::Pub,
                    doc: Some("User display name".into()),
                    serde_rename: None,
                    serde_default: false,
                    serde_skip_serializing_if: None,
                    serde_default_fn: None,
                    extra_field_attrs: vec![],
                },
                FieldDef {
                    name: "age".into(),
                    rust_type: "Option<i32>".into(),
                    visibility: Visibility::Pub,
                    doc: None,
                    serde_rename: None,
                    serde_default: true,
                    serde_skip_serializing_if: Some("Option::is_none".into()),
                    serde_default_fn: None,
                    extra_field_attrs: vec![],
                },
            ],
            derives: DeriveSet::deserialize_input(),
            doc: Some("Arguments for user creation".into()),
            serde_rename_all: None,
            extra_struct_attrs: vec![],
            default_impl: false,
        };

        let code = gen_struct_def(&input);

        assert!(code.contains("/// Arguments for user creation"));
        assert!(code.contains("#[derive(Debug, Clone, Deserialize)]"));
        assert!(code.contains("pub struct UserArgs {"));
        assert!(code.contains("/// User display name"));
        assert!(code.contains("pub name: String,"));
        assert!(code.contains("#[serde(default)]"));
        assert!(code.contains("#[serde(skip_serializing_if = \"Option::is_none\")]"));
        assert!(code.contains("pub age: Option<i32>,"));

        // Brace balance
        let open = code.matches('{').count();
        let close = code.matches('}').count();
        assert_eq!(open, close);
    }

    #[test]
    fn pub_crate_fields() {
        let input = StructDefInput {
            name: "AppState".into(),
            fields: vec![FieldDef {
                name: "db".into(),
                rust_type: "PgPool".into(),
                visibility: Visibility::PubCrate,
                doc: None,
                serde_rename: None,
                serde_default: false,
                serde_skip_serializing_if: None,
                serde_default_fn: None,
                extra_field_attrs: vec![],
            }],
            derives: DeriveSet::default(),
            doc: None,
            serde_rename_all: None,
            extra_struct_attrs: vec![],
            default_impl: false,
        };

        let code = gen_struct_def(&input);
        assert!(code.contains("pub(crate) db: PgPool,"));
    }

    #[test]
    fn serde_rename() {
        let input = StructDefInput {
            name: "ApiResponse".into(),
            fields: vec![FieldDef {
                name: "slot_id".into(),
                rust_type: "String".into(),
                visibility: Visibility::Pub,
                doc: None,
                serde_rename: Some("slotId".into()),
                serde_default: false,
                serde_skip_serializing_if: None,
                serde_default_fn: None,
                extra_field_attrs: vec![],
            }],
            derives: DeriveSet::serde_both(),
            doc: None,
            serde_rename_all: None,
            extra_struct_attrs: vec![],
            default_impl: false,
        };

        let code = gen_struct_def(&input);
        assert!(code.contains("#[serde(rename = \"slotId\")]"));
        assert!(code.contains("#[derive(Debug, Clone, Serialize, Deserialize)]"));
    }

    #[test]
    fn empty_struct() {
        let input = StructDefInput {
            name: "Unit".into(),
            fields: vec![],
            derives: DeriveSet::default(),
            doc: None,
            serde_rename_all: None,
            extra_struct_attrs: vec![],
            default_impl: false,
        };

        let code = gen_struct_def(&input);
        assert!(code.contains("pub struct Unit {"));
        assert!(code.contains("}"));
    }

    // -----------------------------------------------------------------------
    // New capability tests
    // -----------------------------------------------------------------------

    #[test]
    fn serde_rename_all_struct_level() {
        let input = StructDefInput {
            name: "ApiPayload".into(),
            fields: vec![field("user_name", "String")],
            derives: DeriveSet::serde_both(),
            doc: None,
            serde_rename_all: Some("camelCase".into()),
            extra_struct_attrs: vec![],
            default_impl: false,
        };

        let code = gen_struct_def(&input);
        assert!(
            code.contains("#[serde(rename_all = \"camelCase\")]"),
            "missing serde rename_all"
        );
        // rename_all must appear after derive, before struct opening
        let derive_pos = code.find("#[derive(").unwrap();
        let rename_pos = code.find("#[serde(rename_all").unwrap();
        let struct_pos = code.find("pub struct").unwrap();
        assert!(derive_pos < rename_pos, "rename_all must come after derive");
        assert!(rename_pos < struct_pos, "rename_all must come before struct");
    }

    #[test]
    fn extra_struct_attrs() {
        let input = StructDefInput {
            name: "DbRow".into(),
            fields: vec![field("id", "i64")],
            derives: DeriveSet::default(),
            doc: None,
            serde_rename_all: None,
            extra_struct_attrs: vec![
                "#[sqlx(rename_all = \"snake_case\")]".into(),
                "#[table_name = \"users\"]".into(),
            ],
            default_impl: false,
        };

        let code = gen_struct_def(&input);
        assert!(code.contains("#[sqlx(rename_all = \"snake_case\")]"));
        assert!(code.contains("#[table_name = \"users\"]"));
        // attrs must come before the struct opening
        let attr_pos = code.find("#[sqlx(").unwrap();
        let struct_pos = code.find("pub struct").unwrap();
        assert!(attr_pos < struct_pos);
    }

    #[test]
    fn serde_default_fn_field() {
        let input = StructDefInput {
            name: "Config".into(),
            fields: vec![
                FieldDef {
                    name: "timeout".into(),
                    rust_type: "u64".into(),
                    visibility: Visibility::Pub,
                    doc: None,
                    serde_rename: None,
                    serde_default: false,
                    serde_skip_serializing_if: None,
                    serde_default_fn: Some("default_timeout".into()),
                    extra_field_attrs: vec![],
                },
            ],
            derives: DeriveSet::deserialize_input(),
            doc: None,
            serde_rename_all: None,
            extra_struct_attrs: vec![],
            default_impl: false,
        };

        let code = gen_struct_def(&input);
        assert!(
            code.contains("#[serde(default = \"default_timeout\")]"),
            "missing serde default fn"
        );
        // Must NOT also generate #[serde(default)]
        assert!(
            !code.contains("#[serde(default)]"),
            "serde_default_fn should suppress plain serde_default"
        );
    }

    #[test]
    fn serde_default_fn_wins_over_serde_default() {
        // When both serde_default=true and serde_default_fn=Some(...), only the fn form appears
        let input = StructDefInput {
            name: "Opts".into(),
            fields: vec![
                FieldDef {
                    name: "retries".into(),
                    rust_type: "u32".into(),
                    visibility: Visibility::Pub,
                    doc: None,
                    serde_rename: None,
                    serde_default: true,
                    serde_skip_serializing_if: None,
                    serde_default_fn: Some("default_retries".into()),
                    extra_field_attrs: vec![],
                },
            ],
            derives: DeriveSet::deserialize_input(),
            doc: None,
            serde_rename_all: None,
            extra_struct_attrs: vec![],
            default_impl: false,
        };

        let code = gen_struct_def(&input);
        assert!(code.contains("#[serde(default = \"default_retries\")]"));
        assert!(!code.contains("#[serde(default)]"));
    }

    #[test]
    fn extra_field_attrs() {
        let input = StructDefInput {
            name: "Row".into(),
            fields: vec![
                FieldDef {
                    name: "created_at".into(),
                    rust_type: "DateTime<Utc>".into(),
                    visibility: Visibility::Pub,
                    doc: None,
                    serde_rename: None,
                    serde_default: false,
                    serde_skip_serializing_if: None,
                    serde_default_fn: None,
                    extra_field_attrs: vec![
                        "#[sqlx(rename = \"created\")]".into(),
                        "#[validate(required)]".into(),
                    ],
                },
            ],
            derives: DeriveSet::default(),
            doc: None,
            serde_rename_all: None,
            extra_struct_attrs: vec![],
            default_impl: false,
        };

        let code = gen_struct_def(&input);
        assert!(code.contains("#[sqlx(rename = \"created\")]"));
        assert!(code.contains("#[validate(required)]"));
        // extra attrs must come before the field declaration
        let attr_pos = code.find("#[sqlx(rename").unwrap();
        let field_pos = code.find("pub created_at:").unwrap();
        assert!(attr_pos < field_pos);
    }

    #[test]
    fn default_impl_basic_types() {
        let input = StructDefInput {
            name: "Settings".into(),
            fields: vec![
                field("name", "String"),
                field("count", "i32"),
                field("big_count", "i64"),
                field("small", "u32"),
                field("big_small", "u64"),
                field("ratio", "f64"),
                field("enabled", "bool"),
                field("tag", "Option<String>"),
                field("custom", "Vec<String>"),
            ],
            derives: DeriveSet::serde_both(),
            doc: None,
            serde_rename_all: None,
            extra_struct_attrs: vec![],
            default_impl: true,
        };

        let code = gen_struct_def(&input);
        eprintln!("=== DEFAULT IMPL OUTPUT ===\n{code}\n=== END ===");

        // Must contain the impl block
        assert!(code.contains("impl Default for Settings {"));
        assert!(code.contains("fn default() -> Self {"));
        assert!(code.contains("name: String::new(),"));
        assert!(code.contains("count: 0,"));
        assert!(code.contains("big_count: 0,"));
        assert!(code.contains("small: 0,"));
        assert!(code.contains("big_small: 0,"));
        assert!(code.contains("ratio: 0.0,"));
        assert!(code.contains("enabled: false,"));
        assert!(code.contains("tag: None,"));
        assert!(code.contains("custom: Default::default(),"));

        // Brace balance
        let open = code.matches('{').count();
        let close = code.matches('}').count();
        assert_eq!(open, close, "unbalanced braces: {open} open, {close} close");
    }

    #[test]
    fn default_impl_with_serde_default_fn() {
        let input = StructDefInput {
            name: "Cfg".into(),
            fields: vec![
                field("name", "String"),
                FieldDef {
                    name: "timeout".into(),
                    rust_type: "u64".into(),
                    visibility: Visibility::Pub,
                    doc: None,
                    serde_rename: None,
                    serde_default: false,
                    serde_skip_serializing_if: None,
                    serde_default_fn: Some("default_timeout".into()),
                    extra_field_attrs: vec![],
                },
            ],
            derives: DeriveSet::deserialize_input(),
            doc: None,
            serde_rename_all: None,
            extra_struct_attrs: vec![],
            default_impl: true,
        };

        let code = gen_struct_def(&input);
        // The serde_default_fn should be used as the default value function call
        assert!(
            code.contains("timeout: default_timeout(),"),
            "serde_default_fn should generate fn call in Default impl"
        );
        assert!(code.contains("name: String::new(),"));
    }

    #[test]
    fn default_impl_false_no_impl_block() {
        let input = StructDefInput {
            name: "Simple".into(),
            fields: vec![field("x", "i32")],
            derives: DeriveSet::default(),
            doc: None,
            serde_rename_all: None,
            extra_struct_attrs: vec![],
            default_impl: false,
        };

        let code = gen_struct_def(&input);
        assert!(!code.contains("impl Default for"), "default_impl=false should not produce impl block");
    }

    #[test]
    fn full_featured_struct() {
        // Tests all new capabilities together
        let input = StructDefInput {
            name: "TaskConfig".into(),
            fields: vec![
                FieldDef {
                    name: "id".into(),
                    rust_type: "String".into(),
                    visibility: Visibility::Pub,
                    doc: Some("Task identifier".into()),
                    serde_rename: None,
                    serde_default: false,
                    serde_skip_serializing_if: None,
                    serde_default_fn: None,
                    extra_field_attrs: vec![],
                },
                FieldDef {
                    name: "max_retries".into(),
                    rust_type: "u32".into(),
                    visibility: Visibility::Pub,
                    doc: None,
                    serde_rename: Some("maxRetries".into()),
                    serde_default: false,
                    serde_skip_serializing_if: None,
                    serde_default_fn: Some("default_max_retries".into()),
                    extra_field_attrs: vec!["#[validate(range(min = 0, max = 10))]".into()],
                },
                FieldDef {
                    name: "label".into(),
                    rust_type: "Option<String>".into(),
                    visibility: Visibility::Pub,
                    doc: None,
                    serde_rename: None,
                    serde_default: false,
                    serde_skip_serializing_if: Some("Option::is_none".into()),
                    serde_default_fn: None,
                    extra_field_attrs: vec![],
                },
            ],
            derives: DeriveSet::serde_both(),
            doc: Some("Configuration for a background task".into()),
            serde_rename_all: Some("camelCase".into()),
            extra_struct_attrs: vec!["#[sqlx(rename_all = \"snake_case\")]".into()],
            default_impl: true,
        };

        let code = gen_struct_def(&input);
        eprintln!("=== FULL FEATURED OUTPUT ===\n{code}\n=== END ===");

        // Struct-level
        assert!(code.contains("/// Configuration for a background task"));
        assert!(code.contains("#[derive(Debug, Clone, Serialize, Deserialize)]"));
        assert!(code.contains("#[serde(rename_all = \"camelCase\")]"));
        assert!(code.contains("#[sqlx(rename_all = \"snake_case\")]"));
        assert!(code.contains("pub struct TaskConfig {"));

        // Field-level
        assert!(code.contains("/// Task identifier"));
        assert!(code.contains("pub id: String,"));
        assert!(code.contains("#[serde(rename = \"maxRetries\")]"));
        assert!(code.contains("#[serde(default = \"default_max_retries\")]"));
        assert!(code.contains("#[validate(range(min = 0, max = 10))]"));
        assert!(code.contains("pub max_retries: u32,"));
        assert!(code.contains("#[serde(skip_serializing_if = \"Option::is_none\")]"));
        assert!(code.contains("pub label: Option<String>,"));

        // Default impl
        assert!(code.contains("impl Default for TaskConfig {"));
        assert!(code.contains("id: String::new(),"));
        assert!(code.contains("max_retries: default_max_retries(),"));
        assert!(code.contains("label: None,"));

        // Brace balance
        let open = code.matches('{').count();
        let close = code.matches('}').count();
        assert_eq!(open, close, "unbalanced braces: {open} open, {close} close");
    }
}
