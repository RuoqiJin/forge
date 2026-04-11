//! S3 Dispatcher IR — typed representation of `(s3-dispatcher ...)` declarations.
//!
//! The "delegate + trampoline" pattern:
//! - Declares S3 methods that are fully delegated (input → aws_sdk call → output)
//! - Declares custom methods that generate a trampoline + trait signature
//! - Optionally instruments methods with tracing spans
//! - Optionally records S3 operations for audit/metrics
//!
//! Forge generates:
//!   - Delegate method impls that forward to the inner S3 client
//!   - Trampoline stubs + `pub trait S3DispatcherLogic` for custom methods
//!   - `#[instrument]` attributes from InstrumentDecl
//!   - `record_s3_op(...)` calls from record_s3_op fields

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
            "S3Dispatcher IR error in `{}`: {} (expected {:?}, got {:?})",
            self.field, self.message, self.expected, self.got
        )
    }
}

impl std::error::Error for IrError {}

// ---------------------------------------------------------------------------
// Declaration types
// ---------------------------------------------------------------------------

/// Usecase kind — determines which S3 client trait the delegate targets.
#[derive(Debug, Clone, PartialEq)]
pub enum UsecaseKind {
    Bucket,
    Object,
    Multipart,
}

impl UsecaseKind {
    pub fn from_str(s: &str) -> Result<Self, IrError> {
        match s {
            "bucket" => Ok(Self::Bucket),
            "object" => Ok(Self::Object),
            "multipart" => Ok(Self::Multipart),
            other => Err(IrError {
                field: ":usecase".into(),
                message: "unknown usecase kind".into(),
                expected: vec!["bucket".into(), "object".into(), "multipart".into()],
                got: other.into(),
            }),
        }
    }
}

/// Instrument configuration — maps to `#[instrument(...)]` attribute.
#[derive(Debug, Clone)]
pub struct InstrumentDecl {
    pub level: String,
    pub skip: Vec<String>,
    pub fields: Vec<(String, String)>, // e.g. [("start_time", "?time::OffsetDateTime::now_utc()")]
}

/// A delegate method — fully generated, forwards to inner S3 client.
#[derive(Debug, Clone)]
pub struct S3DelegateDecl {
    pub method: String,
    pub input_type: String,
    pub output_type: String,
    pub usecase: UsecaseKind,
    pub instrument: Option<InstrumentDecl>,
    pub record_s3_op: Option<String>,
    pub record_s3_op_bucket: Option<String>, // "*" override for list_buckets
    pub pass_self: bool,
}

/// A custom method — generates trampoline + trait signature.
#[derive(Debug, Clone)]
pub struct S3CustomDecl {
    pub method: String,
    pub input_type: String,
    pub output_type: String,
}

/// Top-level s3-dispatcher declaration.
#[derive(Debug, Clone)]
pub struct S3DispatcherDecl {
    pub name: String,
    pub struct_name: String,
    pub async_trait: Option<String>, // e.g. "async_trait::async_trait"
    pub imports: Vec<String>,
    pub delegates: Vec<S3DelegateDecl>,
    pub customs: Vec<S3CustomDecl>,
}

// ---------------------------------------------------------------------------
// S-EXP -> IR lowering
// ---------------------------------------------------------------------------

impl S3DispatcherDecl {
    pub fn from_sexp(node: &SexpNode) -> Result<Self, IrError> {
        let children = expect_list(node)?;
        if children.len() < 2 {
            return Err(err(
                "s3-dispatcher",
                "missing dispatcher name",
                &["name"],
                "",
            ));
        }

        expect_tag(node, "s3-dispatcher")?;
        let name = expect_atom(&children[1])?.to_string();
        let struct_name = keyword_required(node, ":struct")?;
        let async_trait = node.keyword(":async-trait").map(String::from);
        let imports = parse_imports(node);

        let mut delegates = Vec::new();
        let mut customs = Vec::new();

        for child in children.iter().skip(2) {
            match child.tag() {
                Some("delegate") => delegates.push(S3DelegateDecl::from_sexp(child)?),
                Some("custom") => customs.push(S3CustomDecl::from_sexp(child)?),
                _ => {} // skip keywords and other atoms
            }
        }

        Ok(S3DispatcherDecl {
            name,
            struct_name,
            async_trait,
            imports,
            delegates,
            customs,
        })
    }
}

impl S3DelegateDecl {
    fn from_sexp(node: &SexpNode) -> Result<Self, IrError> {
        let children = expect_list(node)?;
        if children.len() < 2 {
            return Err(err(
                "delegate",
                "missing method name",
                &["method_name"],
                "",
            ));
        }

        let method = expect_atom(&children[1])?.to_string();
        let input_type = keyword_required(node, ":input")?;
        let output_type = keyword_required(node, ":output")?;
        let usecase_str = keyword_required(node, ":usecase")?;
        let usecase = UsecaseKind::from_str(&usecase_str)?;
        let instrument = parse_instrument(node)?;
        let record_s3_op = node.keyword(":record-s3-op").map(String::from);
        let record_s3_op_bucket = node.keyword(":record-s3-op-bucket").map(String::from);
        let pass_self = node.keyword(":pass-self").map(|v| v == "true").unwrap_or(false);

        Ok(S3DelegateDecl {
            method,
            input_type,
            output_type,
            usecase,
            instrument,
            record_s3_op,
            record_s3_op_bucket,
            pass_self,
        })
    }
}

impl S3CustomDecl {
    fn from_sexp(node: &SexpNode) -> Result<Self, IrError> {
        let children = expect_list(node)?;
        if children.len() < 2 {
            return Err(err("custom", "missing method name", &["method_name"], ""));
        }

        let method = expect_atom(&children[1])?.to_string();

        // Reject :usecase on custom methods — they don't delegate
        if node.keyword(":usecase").is_some() {
            return Err(err(
                "custom",
                "custom methods must not have :usecase (they don't delegate)",
                &[":input", ":output"],
                ":usecase",
            ));
        }

        let input_type = keyword_required(node, ":input")?;
        let output_type = keyword_required(node, ":output")?;

        Ok(S3CustomDecl {
            method,
            input_type,
            output_type,
        })
    }
}

/// Parse all `(s3-dispatcher ...)` declarations from a top-level S-EXP list.
pub fn parse_s3_dispatchers(nodes: &[SexpNode]) -> Result<Vec<S3DispatcherDecl>, IrError> {
    nodes
        .iter()
        .filter(|n| n.tag() == Some("s3-dispatcher"))
        .map(S3DispatcherDecl::from_sexp)
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

fn parse_instrument(parent: &SexpNode) -> Result<Option<InstrumentDecl>, IrError> {
    let node = match parent.keyword_node(":instrument") {
        Some(n) => n,
        None => return Ok(None),
    };

    let level = node
        .keyword(":level")
        .or_else(|| node.keyword("level"))
        .map(String::from)
        .unwrap_or_else(|| "debug".to_string());

    let skip = parse_instrument_skip(node);
    let fields = parse_instrument_fields(node);

    Ok(Some(InstrumentDecl {
        level,
        skip,
        fields,
    }))
}

fn parse_instrument_skip(parent: &SexpNode) -> Vec<String> {
    // Try keyword-style first: :skip (self req)
    let node = parent
        .keyword_node(":skip")
        .or_else(|| parent.keyword_node("skip"));
    match node {
        Some(SexpNode::List(items)) => items
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

fn parse_instrument_fields(parent: &SexpNode) -> Vec<(String, String)> {
    // Try keyword-style: :fields ((name "expr") (name2 "expr2"))
    let node = parent
        .keyword_node(":fields")
        .or_else(|| parent.keyword_node("fields"));
    match node {
        Some(SexpNode::List(items)) => items
            .iter()
            .filter_map(|item| {
                if let SexpNode::List(parts) = item {
                    if parts.len() >= 2 {
                        let name = expect_atom(&parts[0]).ok()?.to_string();
                        let expr = expect_atom(&parts[1]).ok()?.to_string();
                        return Some((name, expr));
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
    fn parse_minimal_dispatcher() {
        let sexp = parse(
            r#"(s3-dispatcher fs
              :struct FS
              :async-trait "async_trait::async_trait"
              :imports ("use aws_sdk_s3::Client" "use crate::error::S3Error")

              (delegate list_buckets
                :input ListBucketsInput
                :output ListBucketsOutput
                :usecase bucket
                :record-s3-op ListBuckets
                :record-s3-op-bucket "*")

              (custom get_object_stream
                :input GetObjectInput
                :output StreamingBody))"#,
        );
        let decl = S3DispatcherDecl::from_sexp(&sexp).unwrap();

        assert_eq!(decl.name, "fs");
        assert_eq!(decl.struct_name, "FS");
        assert_eq!(decl.async_trait, Some("async_trait::async_trait".into()));
        assert_eq!(decl.imports.len(), 2);
        assert_eq!(decl.imports[0], "use aws_sdk_s3::Client");

        // Delegates
        assert_eq!(decl.delegates.len(), 1);
        let d = &decl.delegates[0];
        assert_eq!(d.method, "list_buckets");
        assert_eq!(d.input_type, "ListBucketsInput");
        assert_eq!(d.output_type, "ListBucketsOutput");
        assert_eq!(d.usecase, UsecaseKind::Bucket);
        assert_eq!(d.record_s3_op, Some("ListBuckets".into()));
        assert_eq!(d.record_s3_op_bucket, Some("*".into()));
        assert!(!d.pass_self);

        // Customs
        assert_eq!(decl.customs.len(), 1);
        let c = &decl.customs[0];
        assert_eq!(c.method, "get_object_stream");
        assert_eq!(c.input_type, "GetObjectInput");
        assert_eq!(c.output_type, "StreamingBody");
    }

    #[test]
    fn reject_unknown_usecase() {
        let sexp = parse(
            r#"(s3-dispatcher bad
              :struct Bad
              (delegate put_item
                :input PutItemInput
                :output PutItemOutput
                :usecase storage))"#,
        );
        let e = S3DispatcherDecl::from_sexp(&sexp).unwrap_err();
        assert_eq!(e.field, ":usecase");
        assert!(e.got.contains("storage"));
    }

    #[test]
    fn reject_missing_input() {
        let sexp = parse(
            r#"(s3-dispatcher bad
              :struct Bad
              (delegate no_input
                :output SomeOutput
                :usecase object))"#,
        );
        let e = S3DispatcherDecl::from_sexp(&sexp).unwrap_err();
        assert_eq!(e.field, ":input");
        assert!(e.message.contains("missing required"));
    }

    #[test]
    fn reject_custom_with_usecase() {
        let sexp = parse(
            r#"(s3-dispatcher bad
              :struct Bad
              (custom hybrid_method
                :input SomeInput
                :output SomeOutput
                :usecase object))"#,
        );
        let e = S3DispatcherDecl::from_sexp(&sexp).unwrap_err();
        assert_eq!(e.field, "custom");
        assert!(e.message.contains("must not have :usecase"));
    }

    #[test]
    fn delegate_defaults() {
        let sexp = parse(
            r#"(s3-dispatcher defaults
              :struct Defaults
              (delegate head_object
                :input HeadObjectInput
                :output HeadObjectOutput
                :usecase object))"#,
        );
        let decl = S3DispatcherDecl::from_sexp(&sexp).unwrap();
        let d = &decl.delegates[0];

        assert!(!d.pass_self);
        assert!(d.instrument.is_none());
        assert!(d.record_s3_op.is_none());
        assert!(d.record_s3_op_bucket.is_none());
        assert!(decl.async_trait.is_none());
        assert!(decl.imports.is_empty());
    }

    #[test]
    fn parse_instrument_with_fields() {
        let sexp = parse(
            r#"(s3-dispatcher instrumented
              :struct Instrumented
              (delegate put_object
                :input PutObjectInput
                :output PutObjectOutput
                :usecase object
                :instrument (:level "debug" :skip (self req) :fields ((start_time "?time::OffsetDateTime::now_utc()")))
                :record-s3-op PutObject
                :pass-self true))"#,
        );
        let decl = S3DispatcherDecl::from_sexp(&sexp).unwrap();
        let d = &decl.delegates[0];

        assert!(d.pass_self);
        assert_eq!(d.record_s3_op, Some("PutObject".into()));

        let inst = d.instrument.as_ref().expect("instrument should be present");
        assert_eq!(inst.level, "debug");
        assert_eq!(inst.skip, vec!["self", "req"]);
        assert_eq!(inst.fields.len(), 1);
        assert_eq!(inst.fields[0].0, "start_time");
        assert_eq!(inst.fields[0].1, "?time::OffsetDateTime::now_utc()");
    }

    #[test]
    fn parse_s3_dispatchers_filters_correctly() {
        let input = r#"
            (pure-utility unrelated :target "foo.rs"
              (fn dummy :returns bool))

            (s3-dispatcher alpha
              :struct Alpha
              (delegate get_object
                :input GetObjectInput
                :output GetObjectOutput
                :usecase object))

            (s3-dispatcher beta
              :struct Beta
              (delegate delete_object
                :input DeleteObjectInput
                :output DeleteObjectOutput
                :usecase object))
        "#;
        let nodes = SexpNode::parse(input).unwrap();
        let dispatchers = parse_s3_dispatchers(&nodes).unwrap();

        assert_eq!(dispatchers.len(), 2);
        assert_eq!(dispatchers[0].name, "alpha");
        assert_eq!(dispatchers[0].struct_name, "Alpha");
        assert_eq!(dispatchers[1].name, "beta");
        assert_eq!(dispatchers[1].struct_name, "Beta");
    }

    #[test]
    fn reject_missing_struct() {
        let sexp = parse(
            r#"(s3-dispatcher no-struct
              (delegate foo
                :input FooInput
                :output FooOutput
                :usecase bucket))"#,
        );
        let e = S3DispatcherDecl::from_sexp(&sexp).unwrap_err();
        assert_eq!(e.field, ":struct");
        assert!(e.message.contains("missing required"));
    }

    #[test]
    fn multipart_usecase() {
        let sexp = parse(
            r#"(s3-dispatcher mp
              :struct Mp
              (delegate create_multipart_upload
                :input CreateMultipartUploadInput
                :output CreateMultipartUploadOutput
                :usecase multipart))"#,
        );
        let decl = S3DispatcherDecl::from_sexp(&sexp).unwrap();
        assert_eq!(decl.delegates[0].usecase, UsecaseKind::Multipart);
    }
}
