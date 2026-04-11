//! S3 Response Builder IR — typed representation of `(s3-response-builder ...)` declarations.
//!
//! The "builder catalog" pattern:
//! - Declares builder functions that construct S3 DTO response objects
//! - Categorizes builders by complexity: Default, Const, Wrap, Custom
//! - Optionally tracks source module for each builder
//!
//! Forge generates:
//!   - `generated_builders.rs` — Default/Const/Wrap builders (fully generated)
//!   - `custom_builders.rs` — Custom trampolines + trait for complex builders

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
            "S3ResponseBuilder IR error in `{}`: {} (expected {:?}, got {:?})",
            self.field, self.message, self.expected, self.got
        )
    }
}

impl std::error::Error for IrError {}

// ---------------------------------------------------------------------------
// Declaration types
// ---------------------------------------------------------------------------

/// Builder kind — determines how the code is generated.
#[derive(Debug, Clone, PartialEq)]
pub enum BuilderKind {
    /// Returns `Output::default()`
    Default,
    /// Returns a hardcoded constant expression (`:body` field)
    Const,
    /// Maps params to output fields via `..Default::default()` fill
    Wrap,
    /// Complex logic — trait dispatch to custom_builders.rs
    Custom,
}

impl BuilderKind {
    pub fn from_str(s: &str) -> Result<Self, IrError> {
        match s {
            "default" => Ok(Self::Default),
            "const" => Ok(Self::Const),
            "wrap" => Ok(Self::Wrap),
            "custom" => Ok(Self::Custom),
            other => Err(IrError {
                field: ":kind".into(),
                message: "unknown builder kind".into(),
                expected: vec![
                    "default".into(),
                    "const".into(),
                    "wrap".into(),
                    "custom".into(),
                ],
                got: other.into(),
            }),
        }
    }
}

/// A single builder function declaration.
#[derive(Debug, Clone)]
pub struct BuilderDecl {
    pub name: String,
    pub kind: BuilderKind,
    pub output_type: String,
    pub params: Vec<(String, String)>,
    pub fields: Vec<(String, String)>,
    pub body: Option<String>,
    pub source: Option<String>,
    pub generic: Option<String>,
}

/// Top-level s3-response-builder declaration.
#[derive(Debug, Clone)]
pub struct S3ResponseBuilderDecl {
    pub name: String,
    pub target: Option<String>,
    pub imports: Vec<String>,
    pub builders: Vec<BuilderDecl>,
}

impl S3ResponseBuilderDecl {
    /// Count builders by kind.
    pub fn count_by_kind(&self, kind: &BuilderKind) -> usize {
        self.builders.iter().filter(|b| b.kind == *kind).count()
    }

    /// Get all unique source modules referenced.
    pub fn source_modules(&self) -> Vec<String> {
        let mut seen = std::collections::HashSet::new();
        let mut result = Vec::new();
        for b in &self.builders {
            if let Some(ref src) = b.source {
                if seen.insert(src.clone()) {
                    result.push(src.clone());
                }
            }
        }
        result
    }
}

// ---------------------------------------------------------------------------
// S-EXP -> IR lowering
// ---------------------------------------------------------------------------

impl S3ResponseBuilderDecl {
    pub fn from_sexp(node: &SexpNode) -> Result<Self, IrError> {
        let children = expect_list(node)?;
        if children.len() < 2 {
            return Err(err(
                "s3-response-builder",
                "missing builder name",
                &["name"],
                "",
            ));
        }

        expect_tag(node, "s3-response-builder")?;
        let name = expect_atom(&children[1])?.to_string();
        let target = node.keyword(":target").map(String::from);
        let imports = parse_imports(node);

        let mut builders = Vec::new();
        for child in children.iter().skip(2) {
            if child.tag() == Some("builder") {
                builders.push(BuilderDecl::from_sexp(child)?);
            }
        }

        Ok(S3ResponseBuilderDecl {
            name,
            target,
            imports,
            builders,
        })
    }
}

impl BuilderDecl {
    fn from_sexp(node: &SexpNode) -> Result<Self, IrError> {
        let children = expect_list(node)?;
        if children.len() < 2 {
            return Err(err(
                "builder",
                "missing builder function name",
                &["fn_name"],
                "",
            ));
        }

        let name = expect_atom(&children[1])?.to_string();
        let kind_str = keyword_required(node, ":kind")?;
        let kind = BuilderKind::from_str(&kind_str)?;
        let output_type = keyword_required(node, ":output")?;
        let body = node.keyword(":body").map(String::from);
        let source = node.keyword(":source").map(String::from);
        let generic = node.keyword(":generic").map(String::from);
        let params = parse_param_pairs(node);
        let fields = parse_field_pairs(node);

        // Validate kind-specific requirements
        match kind {
            BuilderKind::Const => {
                if body.is_none() {
                    return Err(err(
                        &name,
                        "const builder requires :body",
                        &[":body expression"],
                        "none",
                    ));
                }
            }
            BuilderKind::Wrap => {
                if fields.is_empty() {
                    return Err(err(
                        &name,
                        "wrap builder requires :fields",
                        &[":fields ((field expr) ...)"],
                        "empty",
                    ));
                }
            }
            _ => {}
        }

        Ok(BuilderDecl {
            name,
            kind,
            output_type,
            params,
            fields,
            body,
            source,
            generic,
        })
    }
}

/// Parse all `(s3-response-builder ...)` declarations from a top-level S-EXP list.
pub fn parse_s3_response_builders(
    nodes: &[SexpNode],
) -> Result<Vec<S3ResponseBuilderDecl>, IrError> {
    nodes
        .iter()
        .filter(|n| n.tag() == Some("s3-response-builder"))
        .map(S3ResponseBuilderDecl::from_sexp)
        .collect()
}

// ---------------------------------------------------------------------------
// Sub-parsers
// ---------------------------------------------------------------------------

fn parse_imports(parent: &SexpNode) -> Vec<String> {
    let node = match parent.keyword_node(":imports") {
        Some(n) => n,
        None => return vec![],
    };
    match node {
        SexpNode::List(items) => items
            .iter()
            .filter_map(|item| {
                if let SexpNode::Atom(s) = item {
                    Some(s.clone())
                } else {
                    None
                }
            })
            .collect(),
        _ => vec![],
    }
}

/// Parse `:params ((name "Type") (name2 "Type2"))` into vec of (name, type) pairs.
fn parse_param_pairs(parent: &SexpNode) -> Vec<(String, String)> {
    let node = match parent.keyword_node(":params") {
        Some(n) => n,
        None => return vec![],
    };
    parse_pairs(node)
}

/// Parse `:fields ((field_name "expr") ...)` into vec of (field, expr) pairs.
fn parse_field_pairs(parent: &SexpNode) -> Vec<(String, String)> {
    let node = match parent.keyword_node(":fields") {
        Some(n) => n,
        None => return vec![],
    };
    parse_pairs(node)
}

fn parse_pairs(node: &SexpNode) -> Vec<(String, String)> {
    match node {
        SexpNode::List(items) => items
            .iter()
            .filter_map(|item| {
                if let SexpNode::List(parts) = item {
                    if parts.len() >= 2 {
                        let name = expect_atom(&parts[0]).ok()?.to_string();
                        let value = expect_atom(&parts[1]).ok()?.to_string();
                        return Some((name, value));
                    }
                }
                None
            })
            .collect(),
        _ => vec![],
    }
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
        SexpNode::List(_) => Err(err("node", "expected atom", &["atom"], "(...)")),
    }
}

fn keyword_required(node: &SexpNode, key: &str) -> Result<String, IrError> {
    node.keyword(key)
        .map(String::from)
        .ok_or_else(|| err(key, &format!("missing required {key}"), &["value"], ""))
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
    fn parse_minimal_builder_catalog() {
        let sexp = parse(
            r#"(s3-response-builder s3-api
              :target "rustfs/src/storage/s3_api"
              :imports (
                "s3s::dto::*"
                "crate::storage::s3_api::common::rustfs_owner"
              )

              (builder build_put_bucket_encryption_output
                :kind default
                :output PutBucketEncryptionOutput
                :source encryption)

              (builder build_get_bucket_acl_output
                :kind const
                :output GetBucketAclOutput
                :source acl
                :body "GetBucketAclOutput { grants: Some(full_control_grants()), owner: Some(rustfs_owner()) }")

              (builder build_get_bucket_encryption_output
                :kind wrap
                :output GetBucketEncryptionOutput
                :source encryption
                :params ((sse "Option<ServerSideEncryptionConfiguration>"))
                :fields ((server_side_encryption_configuration "sse")))

              (builder build_list_objects_v2_output
                :kind custom
                :output ListObjectsV2Output
                :source bucket
                :params ((objects "Vec<ObjectInfo>") (result "ListObjectsV2Info") (encoded "bool"))))"#,
        );
        let decl = S3ResponseBuilderDecl::from_sexp(&sexp).unwrap();

        assert_eq!(decl.name, "s3-api");
        assert_eq!(
            decl.target,
            Some("rustfs/src/storage/s3_api".into())
        );
        assert_eq!(decl.imports.len(), 2);
        assert_eq!(decl.builders.len(), 4);

        // Default
        let b0 = &decl.builders[0];
        assert_eq!(b0.name, "build_put_bucket_encryption_output");
        assert_eq!(b0.kind, BuilderKind::Default);
        assert_eq!(b0.output_type, "PutBucketEncryptionOutput");
        assert_eq!(b0.source, Some("encryption".into()));
        assert!(b0.params.is_empty());
        assert!(b0.fields.is_empty());
        assert!(b0.body.is_none());

        // Const
        let b1 = &decl.builders[1];
        assert_eq!(b1.name, "build_get_bucket_acl_output");
        assert_eq!(b1.kind, BuilderKind::Const);
        assert!(b1.body.is_some());
        assert!(b1.body.as_ref().unwrap().contains("full_control_grants()"));

        // Wrap
        let b2 = &decl.builders[2];
        assert_eq!(b2.name, "build_get_bucket_encryption_output");
        assert_eq!(b2.kind, BuilderKind::Wrap);
        assert_eq!(b2.params.len(), 1);
        assert_eq!(b2.params[0].0, "sse");
        assert_eq!(b2.params[0].1, "Option<ServerSideEncryptionConfiguration>");
        assert_eq!(b2.fields.len(), 1);
        assert_eq!(b2.fields[0].0, "server_side_encryption_configuration");
        assert_eq!(b2.fields[0].1, "sse");

        // Custom
        let b3 = &decl.builders[3];
        assert_eq!(b3.name, "build_list_objects_v2_output");
        assert_eq!(b3.kind, BuilderKind::Custom);
        assert_eq!(b3.params.len(), 3);
        assert_eq!(b3.source, Some("bucket".into()));
    }

    #[test]
    fn reject_unknown_kind() {
        let sexp = parse(
            r#"(s3-response-builder bad
              (builder broken
                :kind magic
                :output SomeOutput))"#,
        );
        let e = S3ResponseBuilderDecl::from_sexp(&sexp).unwrap_err();
        assert_eq!(e.field, ":kind");
        assert!(e.got.contains("magic"));
    }

    #[test]
    fn reject_const_without_body() {
        let sexp = parse(
            r#"(s3-response-builder bad
              (builder broken
                :kind const
                :output SomeOutput
                :source test))"#,
        );
        let e = S3ResponseBuilderDecl::from_sexp(&sexp).unwrap_err();
        assert_eq!(e.field, "broken");
        assert!(e.message.contains("requires :body"));
    }

    #[test]
    fn reject_wrap_without_fields() {
        let sexp = parse(
            r#"(s3-response-builder bad
              (builder broken
                :kind wrap
                :output SomeOutput
                :source test
                :params ((x "i32"))))"#,
        );
        let e = S3ResponseBuilderDecl::from_sexp(&sexp).unwrap_err();
        assert_eq!(e.field, "broken");
        assert!(e.message.contains("requires :fields"));
    }

    #[test]
    fn count_by_kind() {
        let sexp = parse(
            r#"(s3-response-builder test
              (builder a :kind default :output A)
              (builder b :kind default :output B)
              (builder c :kind const :output C :body "C{}")
              (builder d :kind wrap :output D :params ((x "i32")) :fields ((x "x")))
              (builder e :kind custom :output E))"#,
        );
        let decl = S3ResponseBuilderDecl::from_sexp(&sexp).unwrap();

        assert_eq!(decl.count_by_kind(&BuilderKind::Default), 2);
        assert_eq!(decl.count_by_kind(&BuilderKind::Const), 1);
        assert_eq!(decl.count_by_kind(&BuilderKind::Wrap), 1);
        assert_eq!(decl.count_by_kind(&BuilderKind::Custom), 1);
    }

    #[test]
    fn source_modules_dedup() {
        let sexp = parse(
            r#"(s3-response-builder test
              (builder a :kind default :output A :source encryption)
              (builder b :kind default :output B :source encryption)
              (builder c :kind default :output C :source tagging)
              (builder d :kind default :output D))"#,
        );
        let decl = S3ResponseBuilderDecl::from_sexp(&sexp).unwrap();
        let modules = decl.source_modules();
        assert_eq!(modules, vec!["encryption", "tagging"]);
    }

    #[test]
    fn parse_s3_response_builders_filters_correctly() {
        let input = r#"
            (pure-utility unrelated :target "foo.rs"
              (fn dummy :returns bool))

            (s3-response-builder alpha
              (builder a :kind default :output A))

            (s3-response-builder beta
              (builder b :kind default :output B))
        "#;
        let nodes = SexpNode::parse(input).unwrap();
        let decls = parse_s3_response_builders(&nodes).unwrap();

        assert_eq!(decls.len(), 2);
        assert_eq!(decls[0].name, "alpha");
        assert_eq!(decls[1].name, "beta");
    }

    #[test]
    fn parse_builder_with_generic() {
        let sexp = parse(
            r#"(s3-response-builder test
              (builder s3_response
                :kind custom
                :output "S3Response<T>"
                :params ((output "T"))
                :generic "T"))"#,
        );
        let decl = S3ResponseBuilderDecl::from_sexp(&sexp).unwrap();
        let b = &decl.builders[0];
        assert_eq!(b.name, "s3_response");
        assert_eq!(b.generic, Some("T".into()));
        assert_eq!(b.output_type, "S3Response<T>");
    }

    #[test]
    fn parse_real_lisp_file() {
        let lisp_path = "fixtures/xjpfs/.missiond/intent-s3-response-builders.lisp";
        let lisp_src = match std::fs::read_to_string(lisp_path) {
            Ok(s) => s,
            Err(_) => return, // skip if file not available
        };
        let node = SexpNode::parse_one(&lisp_src).expect("Failed to parse S-EXP");
        let decl = S3ResponseBuilderDecl::from_sexp(&node).expect("Failed to lower to IR");

        assert_eq!(decl.name, "s3-api");
        assert_eq!(
            decl.target,
            Some("rustfs/src/storage/s3_api".into())
        );

        // Verify counts
        let total = decl.builders.len();
        let defaults = decl.count_by_kind(&BuilderKind::Default);
        let consts = decl.count_by_kind(&BuilderKind::Const);
        let wraps = decl.count_by_kind(&BuilderKind::Wrap);
        let customs = decl.count_by_kind(&BuilderKind::Custom);

        assert_eq!(defaults, 4, "expected 4 default builders");
        assert_eq!(consts, 4, "expected 4 const builders");
        assert_eq!(wraps, 9, "expected 9 wrap builders");
        assert_eq!(customs, 17, "expected 17 custom builders");
        assert_eq!(total, 34, "expected 34 total builders, got {}", total);

        // Verify specific builders exist
        let names: Vec<&str> = decl.builders.iter().map(|b| b.name.as_str()).collect();
        assert!(names.contains(&"build_put_bucket_encryption_output"));
        assert!(names.contains(&"build_get_bucket_acl_output"));
        assert!(names.contains(&"build_get_bucket_encryption_output"));
        assert!(names.contains(&"build_list_objects_v2_output"));
        assert!(names.contains(&"parse_list_object_versions_params"));
        assert!(names.contains(&"s3_response"));
        assert!(names.contains(&"validate_object_tag_set"));

        // Verify source modules
        let modules = decl.source_modules();
        assert!(modules.contains(&"encryption".to_string()));
        assert!(modules.contains(&"acl".to_string()));
        assert!(modules.contains(&"bucket".to_string()));
        assert!(modules.contains(&"tagging".to_string()));
        assert!(modules.contains(&"multipart".to_string()));
        assert!(modules.contains(&"object_lock".to_string()));
        assert!(modules.contains(&"response".to_string()));
        assert!(modules.contains(&"restore".to_string()));
        assert!(modules.contains(&"select".to_string()));

        eprintln!(
            "=== IR VALIDATION PASS === defaults={}, consts={}, wraps={}, customs={}, total={}",
            defaults, consts, wraps, customs, total
        );
    }
}
