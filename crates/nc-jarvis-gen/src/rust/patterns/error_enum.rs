//! Error Enum code generator.
//!
//! Generates complete error enum source files:
//! - Enum definition with `#[derive(thiserror::Error, Debug)]`
//! - `to_u32()` / `from_u32()` code mapping methods
//! - Manual `Clone` implementation
//! - `PartialEq` / `Eq` via `to_u32()` (with special `Io` handling)
//! - Optional `Hash` via `to_u32()`
//! - Helper methods (`is_not_found`, etc.) and free functions
//! - `From` conversion impls between error types
//!
//! Entry point: `generate_error_enum(decl) -> String`

use nc_jarvis_ir::error_enum::*;

use crate::common::{blank, forge_header, w};
use crate::rust::atoms::impl_block::{gen_impl_block, ImplBlockInput, MethodDef};
use crate::rust::atoms::match_arm::{gen_match_expr, MatchArmDef, MatchExprInput};

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Generate the complete error enum source file.
pub fn generate_error_enum(decl: &ErrorEnumDecl) -> String {
    let mut out = String::with_capacity(16384);

    let source = decl
        .target
        .as_deref()
        .unwrap_or("intent-error-enums.lisp");
    forge_header(&mut out, "rust", source);
    w(
        &mut out,
        0,
        &format!(
            "// Pattern: error-enum ({}, {} variants)",
            decl.name,
            decl.all_variants().len()
        ),
    );
    blank(&mut out);

    gen_enum_def(&mut out, decl);
    blank(&mut out);

    if decl.has_feature(&ErrorFeature::U32Codes) {
        gen_u32_codes(&mut out, decl);
        blank(&mut out);
    }

    // Helper methods (inside impl block)
    let method_helpers: Vec<&HelperDecl> = decl.helpers.iter().filter(|h| h.is_method).collect();
    if !method_helpers.is_empty() {
        gen_method_helpers(&mut out, decl, &method_helpers);
        blank(&mut out);
    }

    // Free helper functions
    let free_helpers: Vec<&HelperDecl> = decl.helpers.iter().filter(|h| h.is_free).collect();
    if !free_helpers.is_empty() {
        gen_free_helpers(&mut out, decl, &free_helpers);
        blank(&mut out);
    }

    // Static helper methods
    let static_helpers: Vec<&HelperDecl> = decl.helpers.iter().filter(|h| h.is_static).collect();
    if !static_helpers.is_empty() {
        gen_static_helpers(&mut out, decl, &static_helpers);
        blank(&mut out);
    }

    // From conversions
    for fc in &decl.from_conversions {
        gen_from_conversion(&mut out, decl, fc);
        blank(&mut out);
    }

    if decl.has_feature(&ErrorFeature::PartialEq) {
        gen_partial_eq(&mut out, decl);
        blank(&mut out);
    }

    if decl.has_feature(&ErrorFeature::Clone) {
        gen_clone(&mut out, decl);
        blank(&mut out);
    }

    if decl.has_feature(&ErrorFeature::Hash) {
        gen_hash(&mut out, decl);
        blank(&mut out);
    }

    out
}

// ---------------------------------------------------------------------------
// Enum definition
// ---------------------------------------------------------------------------

fn gen_enum_def(out: &mut String, decl: &ErrorEnumDecl) {
    // Derive attributes
    let derive_str = decl.derives.join(", ");
    w(out, 0, &format!("#[derive({derive_str})]"));
    w(out, 0, &format!("pub enum {} {{", decl.name));

    for cat in &decl.categories {
        w(out, 1, &format!("// --- {} ---", cat.name));
        for v in &cat.variants {
            // Error message attribute
            let msg_attr = format_error_attr(&v.message, &v.params);
            w(out, 1, &msg_attr);

            // From attribute for special params
            if v.from_attr {
                w(out, 1, &format!("{}(#[from] {}),", v.name, v.params[0]));
            } else if v.params.is_empty() {
                w(out, 1, &format!("{},", v.name));
            } else {
                let params_str = v.params.join(", ");
                w(out, 1, &format!("{}({}),", v.name, params_str));
            }
        }
        blank(out);
    }

    w(out, 0, "}");
}

/// Format `#[error("...")]` attribute, converting `{0}` style to proper thiserror syntax.
fn format_error_attr(message: &str, _params: &[String]) -> String {
    format!("#[error(\"{message}\")]")
}

// ---------------------------------------------------------------------------
// u32 codes
// ---------------------------------------------------------------------------

fn gen_u32_codes(out: &mut String, decl: &ErrorEnumDecl) {
    let name = &decl.name;

    // Build to_u32 match arms
    let to_u32_arms: Vec<MatchArmDef> = decl
        .all_variants()
        .iter()
        .filter_map(|v| {
            v.code.map(|code| MatchArmDef {
                pattern: variant_match_pattern(name, v),
                body: format!("{code:#04x}"),
                doc: None,
            })
        })
        .collect();

    let to_u32_match = gen_match_expr(&MatchExprInput {
        subject: "self".into(),
        arms: to_u32_arms,
        fallback: None,
        indent: 0,
    });

    // Build from_u32 match arms
    let from_u32_arms: Vec<MatchArmDef> = decl
        .all_variants()
        .iter()
        .filter_map(|v| {
            v.code.map(|code| MatchArmDef {
                pattern: format!("{code:#04x}"),
                body: format!("Some({})", variant_default_constructor(name, v)),
                doc: None,
            })
        })
        .collect();

    let from_u32_match = gen_match_expr(&MatchExprInput {
        subject: "error".into(),
        arms: from_u32_arms,
        fallback: Some("None".into()),
        indent: 0,
    });

    let block = gen_impl_block(&ImplBlockInput {
        type_name: name.clone(),
        trait_name: None,
        methods: vec![
            MethodDef {
                signature: "pub fn to_u32(&self) -> u32".into(),
                body: to_u32_match.trim_end().to_string(),
                doc: None,
                attributes: vec![],
            },
            MethodDef {
                signature: "pub fn from_u32(error: u32) -> Option<Self>".into(),
                body: from_u32_match.trim_end().to_string(),
                doc: None,
                attributes: vec![],
            },
        ],
    });

    out.push_str(&block);
}

// ---------------------------------------------------------------------------
// Helper methods (&self)
// ---------------------------------------------------------------------------

fn gen_method_helpers(out: &mut String, decl: &ErrorEnumDecl, helpers: &[&HelperDecl]) {
    let name = &decl.name;

    let methods: Vec<MethodDef> = helpers
        .iter()
        .map(|h| {
            let arms = build_matches_arms(name, &h.matches, decl);
            MethodDef {
                signature: format!("pub fn {}(&self) -> bool", h.name),
                body: format!("matches!(self, {arms})"),
                doc: None,
                attributes: vec![],
            }
        })
        .collect();

    let block = gen_impl_block(&ImplBlockInput {
        type_name: name.clone(),
        trait_name: None,
        methods,
    });

    out.push_str(&block);
}

// ---------------------------------------------------------------------------
// Free helper functions
// ---------------------------------------------------------------------------

fn gen_free_helpers(out: &mut String, decl: &ErrorEnumDecl, helpers: &[&HelperDecl]) {
    let name = &decl.name;
    for (i, h) in helpers.iter().enumerate() {
        if i > 0 {
            blank(out);
        }
        w(
            out,
            0,
            &format!("pub fn {}(err: &{name}) -> bool {{", h.name),
        );
        let arms = build_matches_arms(name, &h.matches, decl);
        w(out, 1, &format!("matches!(err, {arms})"));
        w(out, 0, "}");
    }
}

// ---------------------------------------------------------------------------
// Static helper methods
// ---------------------------------------------------------------------------

fn gen_static_helpers(out: &mut String, decl: &ErrorEnumDecl, helpers: &[&HelperDecl]) {
    let name = &decl.name;

    let methods: Vec<MethodDef> = helpers
        .iter()
        .map(|h| {
            let arms = build_matches_arms(name, &h.matches, decl);
            MethodDef {
                signature: format!("pub fn {}(err: &{name}) -> bool", h.name),
                body: format!("matches!(err, {arms})"),
                doc: None,
                attributes: vec![],
            }
        })
        .collect();

    let block = gen_impl_block(&ImplBlockInput {
        type_name: name.clone(),
        trait_name: None,
        methods,
    });

    out.push_str(&block);
}

// ---------------------------------------------------------------------------
// From conversions
// ---------------------------------------------------------------------------

fn gen_from_conversion(out: &mut String, decl: &ErrorEnumDecl, fc: &FromConversionDecl) {
    let target = &decl.name;
    let source = &fc.source;

    if fc.is_reverse {
        // From<Self> for Source
        let arms: Vec<MatchArmDef> = fc
            .mappings
            .iter()
            .map(|m| {
                if m.via_into {
                    MatchArmDef {
                        pattern: format!("{target}::{}(e)", m.source_variant),
                        body: "e.into()".into(),
                        doc: None,
                    }
                } else {
                    MatchArmDef {
                        pattern: format!("{target}::{}", m.source_variant),
                        body: format!("{source}::{}", m.target_variant),
                        doc: None,
                    }
                }
            })
            .collect();

        let match_body = gen_match_expr(&MatchExprInput {
            subject: "val".into(),
            arms,
            fallback: Some(format!("{source}::other(val)")),
            indent: 0,
        });

        let block = gen_impl_block(&ImplBlockInput {
            type_name: source.clone(),
            trait_name: Some(format!("From<{target}>")),
            methods: vec![MethodDef {
                signature: format!("fn from(val: {target}) -> Self"),
                body: match_body.trim_end().to_string(),
                doc: None,
                attributes: vec![],
            }],
        });

        out.push_str(&block);
    } else if fc.via_downcast.is_some() {
        // Via downcast pattern (e.g., From<std::io::Error>)
        let downcast_type = fc.via_downcast.as_ref().unwrap();

        let block = gen_impl_block(&ImplBlockInput {
            type_name: target.clone(),
            trait_name: Some(format!("From<{source}>")),
            methods: vec![MethodDef {
                signature: format!("fn from(e: {source}) -> Self"),
                body: format!(
                    "e.downcast::<{downcast_type}>().unwrap_or_else({target}::other)"
                ),
                doc: None,
                attributes: vec![],
            }],
        });

        out.push_str(&block);
    } else {
        // Standard From<Source> for Self
        let arms: Vec<MatchArmDef> = fc
            .mappings
            .iter()
            .map(|m| {
                if m.via_into {
                    MatchArmDef {
                        pattern: format!("{source}::{}(e)", m.source_variant),
                        body: "e.into()".into(),
                        doc: None,
                    }
                } else if m.via_other {
                    MatchArmDef {
                        pattern: format!("{source}::{}(e)", m.source_variant),
                        body: format!("{target}::other(e)"),
                        doc: None,
                    }
                } else if m.source_variant == "Io" && m.target_variant == "Io" {
                    MatchArmDef {
                        pattern: format!("{source}::Io(io_error)"),
                        body: format!("{target}::Io(io_error)"),
                        doc: None,
                    }
                } else {
                    MatchArmDef {
                        pattern: format!("{source}::{}", m.source_variant),
                        body: format!("{target}::{}", m.target_variant),
                        doc: None,
                    }
                }
            })
            .collect();

        let match_body = gen_match_expr(&MatchExprInput {
            subject: "e".into(),
            arms,
            fallback: Some(format!("{target}::Io(std::io::Error::other(e))")),
            indent: 0,
        });

        let block = gen_impl_block(&ImplBlockInput {
            type_name: target.clone(),
            trait_name: Some(format!("From<{source}>")),
            methods: vec![MethodDef {
                signature: format!("fn from(e: {source}) -> Self"),
                body: match_body.trim_end().to_string(),
                doc: None,
                attributes: vec![],
            }],
        });

        out.push_str(&block);
    }
}

// ---------------------------------------------------------------------------
// PartialEq
// ---------------------------------------------------------------------------

fn gen_partial_eq(out: &mut String, decl: &ErrorEnumDecl) {
    let name = &decl.name;
    let has_io = decl.all_variants().iter().any(|v| v.name == "Io");

    let eq_body = if has_io {
        let match_body = gen_match_expr(&MatchExprInput {
            subject: "(self, other)".into(),
            arms: vec![
                MatchArmDef {
                    pattern: format!("({name}::Io(e1), {name}::Io(e2))"),
                    body: "e1.kind() == e2.kind() && e1.to_string() == e2.to_string()".into(),
                    doc: None,
                },
                MatchArmDef {
                    pattern: "(e1, e2)".into(),
                    body: "e1.to_u32() == e2.to_u32()".into(),
                    doc: None,
                },
            ],
            fallback: None,
            indent: 0,
        });
        match_body.trim_end().to_string()
    } else {
        "self.to_u32() == other.to_u32()".to_string()
    };

    let block = gen_impl_block(&ImplBlockInput {
        type_name: name.clone(),
        trait_name: Some("PartialEq".into()),
        methods: vec![MethodDef {
            signature: "fn eq(&self, other: &Self) -> bool".into(),
            body: eq_body,
            doc: None,
            attributes: vec![],
        }],
    });

    out.push_str(&block);
    blank(out);
    w(out, 0, &format!("impl Eq for {name} {{}}"));
}

// ---------------------------------------------------------------------------
// Clone
// ---------------------------------------------------------------------------

fn gen_clone(out: &mut String, decl: &ErrorEnumDecl) {
    let name = &decl.name;

    let arms: Vec<MatchArmDef> = decl
        .all_variants()
        .iter()
        .map(|v| {
            if v.name == "Io" {
                MatchArmDef {
                    pattern: format!("{name}::Io(e)"),
                    body: format!("{name}::Io(std::io::Error::new(e.kind(), e.to_string()))"),
                    doc: None,
                }
            } else if v.params.is_empty() {
                MatchArmDef {
                    pattern: format!("{name}::{}", v.name),
                    body: format!("{name}::{}", v.name),
                    doc: None,
                }
            } else {
                // gen_clone_body returns "Name::V(p0, p1) => Name::V(p0.clone(), p1.clone()),"
                // We need to split it into pattern and body for MatchArmDef
                let clone_line = gen_clone_body(name, v);
                // The line format is: "pattern => body,"
                // Strip the trailing comma
                let line = clone_line.trim_end_matches(',');
                let (pat, bod) = line.split_once(" => ").unwrap();
                MatchArmDef {
                    pattern: pat.to_string(),
                    body: bod.to_string(),
                    doc: None,
                }
            }
        })
        .collect();

    let match_body = gen_match_expr(&MatchExprInput {
        subject: "self".into(),
        arms,
        fallback: None,
        indent: 0,
    });

    let block = gen_impl_block(&ImplBlockInput {
        type_name: name.clone(),
        trait_name: Some("Clone".into()),
        methods: vec![MethodDef {
            signature: "fn clone(&self) -> Self".into(),
            body: match_body.trim_end().to_string(),
            doc: None,
            attributes: vec![],
        }],
    });

    out.push_str(&block);
}

fn gen_clone_body(name: &str, v: &VariantDecl) -> String {
    let n = v.params.len();
    let bindings: Vec<String> = (0..n).map(|i| format!("p{i}")).collect();
    let bind_str = bindings.join(", ");

    let clones: Vec<String> = v
        .params
        .iter()
        .enumerate()
        .map(|(i, ty)| {
            let b = &bindings[i];
            if is_copy_type(ty) {
                format!("*{b}")
            } else if ty == "std::io::Error" {
                format!("std::io::Error::new({b}.kind(), {b}.to_string())")
            } else {
                format!("{b}.clone()")
            }
        })
        .collect();
    let clone_str = clones.join(", ");

    format!(
        "{name}::{}({bind_str}) => {name}::{}({clone_str}),",
        v.name, v.name
    )
}

fn is_copy_type(ty: &str) -> bool {
    matches!(
        ty,
        "u8" | "u16"
            | "u32"
            | "u64"
            | "usize"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "isize"
            | "f32"
            | "f64"
            | "bool"
            | "char"
    )
}

// ---------------------------------------------------------------------------
// Hash
// ---------------------------------------------------------------------------

fn gen_hash(out: &mut String, decl: &ErrorEnumDecl) {
    let name = &decl.name;

    let block = gen_impl_block(&ImplBlockInput {
        type_name: name.clone(),
        trait_name: Some("std::hash::Hash".into()),
        methods: vec![MethodDef {
            signature: "fn hash<H: std::hash::Hasher>(&self, state: &mut H)".into(),
            body: "self.to_u32().hash(state);".into(),
            doc: None,
            attributes: vec![],
        }],
    });

    out.push_str(&block);
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a `matches!` arm list: `Name::A | Name::B(..) | Name::C(_, _)`
fn build_matches_arms(enum_name: &str, matches: &[String], decl: &ErrorEnumDecl) -> String {
    let all_variants = decl.all_variants();
    matches
        .iter()
        .map(|m| {
            let v = all_variants.iter().find(|v| v.name == *m);
            match v {
                Some(v) if v.params.is_empty() => format!("{enum_name}::{m}"),
                Some(v) => {
                    let wildcards: Vec<&str> = v.params.iter().map(|_| "_").collect();
                    format!("{enum_name}::{}({})", m, wildcards.join(", "))
                }
                None => format!("{enum_name}::{m}"), // fallback
            }
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

/// Pattern for matching a variant (wildcard params).
fn variant_match_pattern(enum_name: &str, v: &VariantDecl) -> String {
    if v.params.is_empty() {
        format!("{enum_name}::{}", v.name)
    } else {
        let wildcards: Vec<&str> = v.params.iter().map(|_| "_").collect();
        format!("{enum_name}::{}({})", v.name, wildcards.join(", "))
    }
}

/// Default constructor for from_u32 (uses Default::default() for params).
fn variant_default_constructor(enum_name: &str, v: &VariantDecl) -> String {
    if v.params.is_empty() {
        format!("{enum_name}::{}", v.name)
    } else if v.name == "Io" {
        format!(
            "{enum_name}::Io(std::io::Error::other(\"Io error\"))"
        )
    } else if v.name == "Lock" {
        format!(
            "{enum_name}::Lock(rustfs_lock::LockError::internal(\"Generic lock error\".to_string()))"
        )
    } else {
        let defaults: Vec<String> = v
            .params
            .iter()
            .map(|ty| {
                if is_copy_type(ty) {
                    "Default::default()".to_string()
                } else {
                    "Default::default()".to_string()
                }
            })
            .collect();
        format!(
            "{enum_name}::{}({})",
            v.name,
            defaults.join(", ")
        )
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use nc_parser::SexpNode;

    fn parse_decl(input: &str) -> ErrorEnumDecl {
        let node = SexpNode::parse_one(input).expect("valid S-EXP");
        ErrorEnumDecl::from_sexp(&node).unwrap()
    }

    #[test]
    fn generate_minimal_enum() {
        let decl = parse_decl(
            r#"(error-enum SimpleError
              :target "src/error.rs"
              :derive (thiserror::Error Debug)
              :features (u32-codes partial-eq)

              (category basic
                (variant NotFound :code 0x01 :message "not found")
                (variant AlreadyExists :code 0x02 :message "already exists")))"#,
        );

        let output = generate_error_enum(&decl);

        // Check header
        assert!(output.contains("GENERATED BY FORGE"));
        assert!(output.contains("error-enum (SimpleError, 2 variants)"));

        // Check enum def
        assert!(output.contains("#[derive(thiserror::Error, Debug)]"));
        assert!(output.contains("pub enum SimpleError {"));
        assert!(output.contains("#[error(\"not found\")]"));
        assert!(output.contains("NotFound,"));
        assert!(output.contains("#[error(\"already exists\")]"));
        assert!(output.contains("AlreadyExists,"));

        // Check u32 codes
        assert!(output.contains("pub fn to_u32(&self) -> u32 {"));
        assert!(output.contains("SimpleError::NotFound => 0x01,"));
        assert!(output.contains("SimpleError::AlreadyExists => 0x02,"));
        assert!(output.contains("pub fn from_u32(error: u32) -> Option<Self> {"));
        assert!(output.contains("0x01 => Some(SimpleError::NotFound),"));

        // Check PartialEq (no Io variant, so simple)
        assert!(output.contains("impl PartialEq for SimpleError {"));
        assert!(output.contains("self.to_u32() == other.to_u32()"));
        assert!(output.contains("impl Eq for SimpleError {}"));
    }

    #[test]
    fn generate_with_io_and_params() {
        let decl = parse_decl(
            r#"(error-enum TestError
              :target "src/error.rs"
              :derive (thiserror::Error Debug)
              :features (u32-codes clone partial-eq)

              (category main
                (variant NotFound :code 0x01 :params (String) :message "not found: {0}")
                (variant Io       :code 0x02 :params (std::io::Error) :message "io: {0}")))"#,
        );

        let output = generate_error_enum(&decl);

        // Parameterized variant
        assert!(output.contains("NotFound(String),"));
        assert!(output.contains("Io(std::io::Error),"));

        // Clone with special Io handling
        assert!(output.contains("impl Clone for TestError {"));
        assert!(output.contains("TestError::Io(e) => TestError::Io(std::io::Error::new(e.kind(), e.to_string())),"));
        assert!(output.contains("TestError::NotFound(p0) => TestError::NotFound(p0.clone()),"));

        // PartialEq with Io special case
        assert!(output.contains("(TestError::Io(e1), TestError::Io(e2)) => e1.kind() == e2.kind() && e1.to_string() == e2.to_string(),"));
    }

    #[test]
    fn generate_helpers() {
        let decl = parse_decl(
            r#"(error-enum E
              :derive (thiserror::Error Debug)
              :features (u32-codes)

              (category a
                (variant A :code 0x01 :message "a")
                (variant B :code 0x02 :params (String) :message "b: {0}")
                (variant C :code 0x03 :message "c"))

              (helper is_ab :method true :matches (A B))
              (helper is_err_a :free true :matches (A)))"#,
        );

        let output = generate_error_enum(&decl);

        // Method helper
        assert!(output.contains("pub fn is_ab(&self) -> bool {"));
        assert!(output.contains("matches!(self, E::A | E::B(_))"));

        // Free helper
        assert!(output.contains("pub fn is_err_a(err: &E) -> bool {"));
        assert!(output.contains("matches!(err, E::A)"));
    }

    #[test]
    fn generate_from_conversion() {
        let decl = parse_decl(
            r#"(error-enum Target
              :derive (thiserror::Error Debug)
              :features (u32-codes)

              (category a
                (variant X :code 0x01 :message "x")
                (variant Y :code 0x02 :message "y"))

              (from Source
                (map SrcX X)
                (map SrcY Y)))"#,
        );

        let output = generate_error_enum(&decl);

        assert!(output.contains("impl From<Source> for Target {"));
        assert!(output.contains("Source::SrcX => Target::X,"));
        assert!(output.contains("Source::SrcY => Target::Y,"));
        assert!(output.contains("_ => Target::Io(std::io::Error::other(e)),"));
    }

    #[test]
    fn generate_from_reverse() {
        let decl = parse_decl(
            r#"(error-enum E
              :derive (thiserror::Error Debug)
              :features (u32-codes)

              (category a
                (variant X :code 0x01 :message "x"))

              (from-reverse Other
                (map X OtherX)))"#,
        );

        let output = generate_error_enum(&decl);

        assert!(output.contains("impl From<E> for Other {"));
        assert!(output.contains("fn from(val: E) -> Self {"));
        assert!(output.contains("E::X => Other::OtherX,"));
        assert!(output.contains("_ => Other::other(val),"));
    }

    #[test]
    fn generate_hash() {
        let decl = parse_decl(
            r#"(error-enum E
              :derive (thiserror::Error Debug)
              :features (u32-codes hash)

              (category a
                (variant X :code 0x01 :message "x")))"#,
        );

        let output = generate_error_enum(&decl);

        assert!(output.contains("impl std::hash::Hash for E {"));
        assert!(output.contains("self.to_u32().hash(state);"));
    }

    #[test]
    fn generate_multi_param_clone() {
        let decl = parse_decl(
            r#"(error-enum E
              :derive (thiserror::Error Debug)
              :features (u32-codes clone)

              (category a
                (variant Triple :code 0x01 :params (String String String) :message "{0}/{1}-{2}")
                (variant Mixed  :code 0x02 :params (usize i64 i64) :message "{0} {1} {2}")))"#,
        );

        let output = generate_error_enum(&decl);

        assert!(output.contains("E::Triple(p0, p1, p2) => E::Triple(p0.clone(), p1.clone(), p2.clone()),"));
        assert!(output.contains("E::Mixed(p0, p1, p2) => E::Mixed(*p0, *p1, *p2),"));
    }

    #[test]
    fn integration_stamp_real_lisp() {
        let lisp_path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../patterns/xjpfs-error-enums.lisp"
        );
        if !std::path::Path::new(lisp_path).exists() {
            eprintln!("Skipping integration test: {lisp_path} not found");
            return;
        }

        let content = std::fs::read_to_string(lisp_path).unwrap();
        let nodes = SexpNode::parse(&content).unwrap();
        let decls = nc_jarvis_ir::error_enum::parse_error_enums(&nodes).unwrap();

        assert_eq!(decls.len(), 3, "expected 3 error-enum declarations");

        // Verify names
        assert_eq!(decls[0].name, "StorageError");
        assert_eq!(decls[1].name, "DiskError");
        assert_eq!(decls[2].name, "BucketMetadataError");

        // Verify variant counts
        assert_eq!(
            decls[0].all_variants().len(),
            63,
            "StorageError should have 63 variants"
        );
        assert_eq!(
            decls[1].all_variants().len(),
            42,
            "DiskError should have 42 variants"
        );
        assert_eq!(
            decls[2].all_variants().len(),
            9,
            "BucketMetadataError should have 9 variants"
        );

        // Stamp output to .forge-output/errors/
        let output_dir = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../.forge-output/errors"
        );
        std::fs::create_dir_all(output_dir).unwrap();

        for decl in &decls {
            let output = generate_error_enum(decl);
            assert!(output.contains("GENERATED BY FORGE"));

            let filename = format!(
                "{}/{}.generated.rs",
                output_dir,
                decl.name.to_lowercase().replace("error", "_error").trim_start_matches('_')
            );
            std::fs::write(&filename, &output).unwrap();
            eprintln!("Stamped: {filename} ({} bytes)", output.len());
        }
    }
}
