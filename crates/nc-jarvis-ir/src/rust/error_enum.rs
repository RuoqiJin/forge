//! Error Enum IR — typed representation of `(error-enum ...)` declarations.
//!
//! The "error enum" pattern generates:
//! - Enum definition with `#[derive(thiserror::Error, Debug)]` and `#[error("...")]`
//! - `to_u32()` / `from_u32()` code mapping
//! - Manual `Clone` implementation
//! - `PartialEq` via `to_u32()` comparison (with special Io handling)
//! - Helper methods and free functions (`is_not_found`, `is_err_object_not_found`, etc.)
//! - `From` conversion impls between error types
//!
//! Complex functions (context transformation, string pattern analysis) stay custom.

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
            "ErrorEnum IR error in `{}`: {} (expected {:?}, got {:?})",
            self.field, self.message, self.expected, self.got
        )
    }
}

impl std::error::Error for IrError {}

// ---------------------------------------------------------------------------
// Declaration types
// ---------------------------------------------------------------------------

/// Feature flags controlling what code is generated.
#[derive(Debug, Clone, PartialEq)]
pub enum ErrorFeature {
    U32Codes,
    Clone,
    PartialEq,
    Hash,
}

impl ErrorFeature {
    pub fn from_str(s: &str) -> Result<Self, IrError> {
        match s {
            "u32-codes" => Ok(Self::U32Codes),
            "clone" => Ok(Self::Clone),
            "partial-eq" => Ok(Self::PartialEq),
            "hash" => Ok(Self::Hash),
            other => Err(err(
                ":features",
                "unknown feature",
                &["u32-codes", "clone", "partial-eq", "hash"],
                other,
            )),
        }
    }
}

/// A category grouping variants together (for comments/organization).
#[derive(Debug, Clone)]
pub struct CategoryDecl {
    pub name: String,
    pub variants: Vec<VariantDecl>,
}

/// A single error variant.
#[derive(Debug, Clone)]
pub struct VariantDecl {
    pub name: String,
    pub code: Option<u32>,
    pub params: Vec<String>,
    pub message: String,
    pub from_attr: bool,
}

/// A helper function or method.
#[derive(Debug, Clone)]
pub struct HelperDecl {
    pub name: String,
    pub is_method: bool,
    pub is_free: bool,
    pub is_static: bool,
    pub matches: Vec<String>,
}

/// `From<Source> for Target` conversion declaration.
#[derive(Debug, Clone)]
pub struct FromConversionDecl {
    pub source: String,
    pub mappings: Vec<FromMapping>,
    pub is_reverse: bool,
    pub via_downcast: Option<String>,
}

/// A single mapping in a From conversion.
#[derive(Debug, Clone)]
pub struct FromMapping {
    pub source_variant: String,
    pub target_variant: String,
    pub via_into: bool,
    pub via_other: bool,
}

/// Top-level error-enum declaration.
#[derive(Debug, Clone)]
pub struct ErrorEnumDecl {
    pub name: String,
    pub target: Option<String>,
    pub derives: Vec<String>,
    pub features: Vec<ErrorFeature>,
    pub categories: Vec<CategoryDecl>,
    pub helpers: Vec<HelperDecl>,
    pub from_conversions: Vec<FromConversionDecl>,
    pub custom_fns: Vec<String>,
}

impl ErrorEnumDecl {
    /// Flatten all variants across categories.
    pub fn all_variants(&self) -> Vec<&VariantDecl> {
        self.categories.iter().flat_map(|c| &c.variants).collect()
    }

    pub fn has_feature(&self, f: &ErrorFeature) -> bool {
        self.features.contains(f)
    }
}

// ---------------------------------------------------------------------------
// S-EXP -> IR lowering
// ---------------------------------------------------------------------------

impl ErrorEnumDecl {
    pub fn from_sexp(node: &SexpNode) -> Result<Self, IrError> {
        let children = expect_list(node)?;
        if children.len() < 2 {
            return Err(err(
                "error-enum",
                "missing enum name",
                &["name"],
                "",
            ));
        }

        expect_tag(node, "error-enum")?;
        let name = expect_atom(&children[1])?.to_string();
        let target = node.keyword(":target").map(String::from);

        // Parse :derive list
        let derives = parse_atom_list(node, ":derive");

        // Parse :features list
        let features = parse_atom_list(node, ":features")
            .into_iter()
            .map(|s| ErrorFeature::from_str(&s))
            .collect::<Result<Vec<_>, _>>()?;

        let mut categories = Vec::new();
        let mut helpers = Vec::new();
        let mut from_conversions = Vec::new();
        let mut custom_fns = Vec::new();

        for child in children.iter().skip(2) {
            match child.tag() {
                Some("category") => categories.push(CategoryDecl::from_sexp(child)?),
                Some("helper") => helpers.push(HelperDecl::from_sexp(child)?),
                Some("from") => from_conversions.push(FromConversionDecl::from_sexp(child, false)?),
                Some("from-reverse") => from_conversions.push(FromConversionDecl::from_sexp(child, true)?),
                Some("custom") => {
                    let inner = expect_list(child)?;
                    if inner.len() >= 2 {
                        custom_fns.push(expect_atom(&inner[1])?.to_string());
                    }
                }
                _ => {} // skip keywords and other atoms
            }
        }

        Ok(ErrorEnumDecl {
            name,
            target,
            derives,
            features,
            categories,
            helpers,
            from_conversions,
            custom_fns,
        })
    }
}

impl CategoryDecl {
    fn from_sexp(node: &SexpNode) -> Result<Self, IrError> {
        let children = expect_list(node)?;
        if children.len() < 2 {
            return Err(err("category", "missing category name", &["name"], ""));
        }

        let name = expect_atom(&children[1])?.to_string();
        let mut variants = Vec::new();

        for child in children.iter().skip(2) {
            if child.tag() == Some("variant") {
                variants.push(VariantDecl::from_sexp(child)?);
            }
        }

        Ok(CategoryDecl { name, variants })
    }
}

impl VariantDecl {
    fn from_sexp(node: &SexpNode) -> Result<Self, IrError> {
        let children = expect_list(node)?;
        if children.len() < 2 {
            return Err(err("variant", "missing variant name", &["name"], ""));
        }

        let name = expect_atom(&children[1])?.to_string();
        let code = node.keyword(":code").map(|s| parse_hex_or_dec(s)).transpose()?;
        let params = parse_atom_list(node, ":params");
        let message = node
            .keyword(":message")
            .map(String::from)
            .unwrap_or_default();
        let from_attr = node.keyword(":from-attr").map(|v| v == "true").unwrap_or(false);

        Ok(VariantDecl {
            name,
            code,
            params,
            message,
            from_attr,
        })
    }
}

impl HelperDecl {
    fn from_sexp(node: &SexpNode) -> Result<Self, IrError> {
        let children = expect_list(node)?;
        if children.len() < 2 {
            return Err(err("helper", "missing helper name", &["name"], ""));
        }

        let name = expect_atom(&children[1])?.to_string();
        let is_method = node.keyword(":method").map(|v| v == "true").unwrap_or(false);
        let is_free = node.keyword(":free").map(|v| v == "true").unwrap_or(false);
        let is_static = node.keyword(":static").map(|v| v == "true").unwrap_or(false);
        let matches = parse_atom_list(node, ":matches");

        Ok(HelperDecl {
            name,
            is_method,
            is_free,
            is_static,
            matches,
        })
    }
}

impl FromConversionDecl {
    fn from_sexp(node: &SexpNode, is_reverse: bool) -> Result<Self, IrError> {
        let children = expect_list(node)?;
        let tag = if is_reverse { "from-reverse" } else { "from" };
        if children.len() < 2 {
            return Err(err(tag, "missing source type", &["source_type"], ""));
        }

        let source = expect_atom(&children[1])?.to_string();
        let via_downcast = node.keyword(":via-downcast").map(String::from);

        let mut mappings = Vec::new();
        for child in children.iter().skip(2) {
            if child.tag() == Some("map") {
                mappings.push(FromMapping::from_sexp(child)?);
            }
        }

        Ok(FromConversionDecl {
            source,
            mappings,
            is_reverse,
            via_downcast,
        })
    }
}

impl FromMapping {
    fn from_sexp(node: &SexpNode) -> Result<Self, IrError> {
        let children = expect_list(node)?;
        if children.len() < 3 {
            return Err(err(
                "map",
                "missing source/target variant",
                &["source_variant", "target_variant"],
                "",
            ));
        }

        let source_variant = expect_atom(&children[1])?.to_string();
        let target_variant = expect_atom(&children[2])?.to_string();
        let via_into = node.keyword(":via-into").map(|v| v == "true").unwrap_or(false);
        let via_other = node.keyword(":via-other").map(|v| v == "true").unwrap_or(false);

        Ok(FromMapping {
            source_variant,
            target_variant,
            via_into,
            via_other,
        })
    }
}

/// Parse all `(error-enum ...)` declarations from a top-level S-EXP list.
pub fn parse_error_enums(nodes: &[SexpNode]) -> Result<Vec<ErrorEnumDecl>, IrError> {
    nodes
        .iter()
        .filter(|n| n.tag() == Some("error-enum"))
        .map(ErrorEnumDecl::from_sexp)
        .collect()
}

// ---------------------------------------------------------------------------
// Sub-parsers
// ---------------------------------------------------------------------------

fn parse_atom_list(parent: &SexpNode, key: &str) -> Vec<String> {
    let node = match parent.keyword_node(key) {
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

fn parse_hex_or_dec(s: &str) -> Result<u32, IrError> {
    if let Some(hex) = s.strip_prefix("0x") {
        u32::from_str_radix(hex, 16).map_err(|_| err(":code", "invalid hex", &["0xNN"], s))
    } else {
        s.parse::<u32>()
            .map_err(|_| err(":code", "invalid number", &["number"], s))
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
    fn parse_minimal_error_enum() {
        let sexp = parse(
            r#"(error-enum TestError
              :target "src/error.rs"
              :derive (thiserror::Error Debug)
              :features (u32-codes clone)

              (category basic
                (variant NotFound :code 0x01 :message "not found")
                (variant IoErr    :code 0x02 :params (std::io::Error) :message "io: {0}")))"#,
        );
        let decl = ErrorEnumDecl::from_sexp(&sexp).unwrap();

        assert_eq!(decl.name, "TestError");
        assert_eq!(decl.target, Some("src/error.rs".into()));
        assert_eq!(decl.derives, vec!["thiserror::Error", "Debug"]);
        assert_eq!(decl.features.len(), 2);
        assert!(decl.has_feature(&ErrorFeature::U32Codes));
        assert!(decl.has_feature(&ErrorFeature::Clone));
        assert!(!decl.has_feature(&ErrorFeature::PartialEq));

        assert_eq!(decl.categories.len(), 1);
        let cat = &decl.categories[0];
        assert_eq!(cat.name, "basic");
        assert_eq!(cat.variants.len(), 2);

        let v0 = &cat.variants[0];
        assert_eq!(v0.name, "NotFound");
        assert_eq!(v0.code, Some(0x01));
        assert!(v0.params.is_empty());
        assert_eq!(v0.message, "not found");

        let v1 = &cat.variants[1];
        assert_eq!(v1.name, "IoErr");
        assert_eq!(v1.code, Some(0x02));
        assert_eq!(v1.params, vec!["std::io::Error"]);
        assert_eq!(v1.message, "io: {0}");
    }

    #[test]
    fn parse_helpers() {
        let sexp = parse(
            r#"(error-enum E
              :derive (Debug)
              :features ()

              (category a
                (variant X :code 0x01 :message "x")
                (variant Y :code 0x02 :message "y"))

              (helper is_xy :method true :matches (X Y))
              (helper is_err_x :free true :matches (X)))"#,
        );
        let decl = ErrorEnumDecl::from_sexp(&sexp).unwrap();

        assert_eq!(decl.helpers.len(), 2);

        let h0 = &decl.helpers[0];
        assert_eq!(h0.name, "is_xy");
        assert!(h0.is_method);
        assert!(!h0.is_free);
        assert_eq!(h0.matches, vec!["X", "Y"]);

        let h1 = &decl.helpers[1];
        assert_eq!(h1.name, "is_err_x");
        assert!(!h1.is_method);
        assert!(h1.is_free);
        assert_eq!(h1.matches, vec!["X"]);
    }

    #[test]
    fn parse_from_conversions() {
        let sexp = parse(
            r#"(error-enum Target
              :derive (Debug)
              :features ()

              (category a
                (variant A :code 0x01 :message "a")
                (variant B :code 0x02 :message "b"))

              (from SourceError
                (map SrcA A)
                (map SrcB B)))"#,
        );
        let decl = ErrorEnumDecl::from_sexp(&sexp).unwrap();

        assert_eq!(decl.from_conversions.len(), 1);
        let fc = &decl.from_conversions[0];
        assert_eq!(fc.source, "SourceError");
        assert!(!fc.is_reverse);
        assert_eq!(fc.mappings.len(), 2);
        assert_eq!(fc.mappings[0].source_variant, "SrcA");
        assert_eq!(fc.mappings[0].target_variant, "A");
        assert_eq!(fc.mappings[1].source_variant, "SrcB");
        assert_eq!(fc.mappings[1].target_variant, "B");
    }

    #[test]
    fn parse_from_reverse() {
        let sexp = parse(
            r#"(error-enum E
              :derive (Debug)
              :features ()

              (category a
                (variant X :code 0x01 :message "x"))

              (from-reverse OtherError
                (map X OtherX)))"#,
        );
        let decl = ErrorEnumDecl::from_sexp(&sexp).unwrap();

        assert_eq!(decl.from_conversions.len(), 1);
        let fc = &decl.from_conversions[0];
        assert!(fc.is_reverse);
        assert_eq!(fc.source, "OtherError");
    }

    #[test]
    fn parse_custom_fns() {
        let sexp = parse(
            r#"(error-enum E
              :derive (Debug)
              :features ()

              (category a (variant X :code 0x01 :message "x"))

              (custom to_object_err)
              (custom is_network_or_host_down))"#,
        );
        let decl = ErrorEnumDecl::from_sexp(&sexp).unwrap();

        assert_eq!(decl.custom_fns, vec!["to_object_err", "is_network_or_host_down"]);
    }

    #[test]
    fn all_variants_flattens_categories() {
        let sexp = parse(
            r#"(error-enum E
              :derive (Debug)
              :features ()

              (category a
                (variant A1 :code 0x01 :message "a1")
                (variant A2 :code 0x02 :message "a2"))

              (category b
                (variant B1 :code 0x03 :message "b1")))"#,
        );
        let decl = ErrorEnumDecl::from_sexp(&sexp).unwrap();

        let all = decl.all_variants();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].name, "A1");
        assert_eq!(all[1].name, "A2");
        assert_eq!(all[2].name, "B1");
    }

    #[test]
    fn reject_bad_feature() {
        let sexp = parse(
            r#"(error-enum Bad
              :derive (Debug)
              :features (u32-codes badfeature)

              (category a (variant X :code 0x01 :message "x")))"#,
        );
        let e = ErrorEnumDecl::from_sexp(&sexp).unwrap_err();
        assert_eq!(e.field, ":features");
        assert!(e.got.contains("badfeature"));
    }

    #[test]
    fn reject_bad_hex_code() {
        let sexp = parse(
            r#"(error-enum Bad
              :derive (Debug)
              :features ()

              (category a (variant X :code 0xZZ :message "x")))"#,
        );
        let e = ErrorEnumDecl::from_sexp(&sexp).unwrap_err();
        assert_eq!(e.field, ":code");
    }

    #[test]
    fn parse_via_into_mapping() {
        let sexp = parse(
            r#"(error-enum E
              :derive (Debug)
              :features ()

              (category a (variant Io :code 0x01 :params (std::io::Error) :message "io: {0}"))

              (from Source
                (map Io Io :via-into true)))"#,
        );
        let decl = ErrorEnumDecl::from_sexp(&sexp).unwrap();
        assert!(decl.from_conversions[0].mappings[0].via_into);
    }

    #[test]
    fn parse_from_attr_variant() {
        let sexp = parse(
            r#"(error-enum E
              :derive (Debug)
              :features ()

              (category a
                (variant Lock :code 0x01 :params (LockError) :message "Lock: {0}" :from-attr true)))"#,
        );
        let decl = ErrorEnumDecl::from_sexp(&sexp).unwrap();
        assert!(decl.categories[0].variants[0].from_attr);
    }
}
