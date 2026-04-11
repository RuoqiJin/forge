//! Pure Utility IR — typed representation of `(pure-utility ...)` declarations.
//!
//! The "algorithm isolation chamber" pattern:
//! - Declares pure function signatures (no &self, no side effects)
//! - Generates trait contract + `todo!()` stubs + TDD test skeleton
//! - Locks complex algorithms (regex, token math, etc.) inside custom.rs
//!
//! Forge does NOT attempt to generate algorithm internals.

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
            "PureUtility IR error in `{}`: {} (expected {:?}, got {:?})",
            self.field, self.message, self.expected, self.got
        )
    }
}

impl std::error::Error for IrError {}

// ---------------------------------------------------------------------------
// Declaration types
// ---------------------------------------------------------------------------

/// A single test case for a pure function.
#[derive(Debug, Clone)]
pub struct TestCaseDecl {
    pub name: String,
    pub input: String,
    pub expected: String,
}

/// A pure function declaration — no &self, no side effects.
#[derive(Debug, Clone)]
pub struct FnDecl {
    pub name: String,
    pub params: Vec<(String, String)>, // (name, type)
    pub returns: String,
    pub doc: Option<String>,
    pub test_cases: Vec<TestCaseDecl>,
}

/// Top-level pure-utility module declaration.
#[derive(Debug, Clone)]
pub struct PureUtilityDecl {
    pub name: String,
    pub target: Option<String>,
    pub fns: Vec<FnDecl>,
}

// ---------------------------------------------------------------------------
// S-EXP → IR lowering
// ---------------------------------------------------------------------------

impl PureUtilityDecl {
    pub fn from_sexp(node: &SexpNode) -> Result<Self, IrError> {
        let children = expect_list(node)?;
        if children.len() < 2 {
            return Err(err(
                "pure-utility",
                "missing utility name",
                &["utility_name"],
                "",
            ));
        }

        expect_tag(node, "pure-utility")?;
        let name = expect_atom(&children[1])?.to_string();
        let target = node.keyword(":target").map(String::from);

        let mut fns = Vec::new();
        for child in children.iter().skip(2) {
            if child.tag() == Some("fn") {
                fns.push(FnDecl::from_sexp(child)?);
            }
        }

        if fns.is_empty() {
            return Err(err(
                "pure-utility",
                "must declare at least one (fn ...)",
                &["(fn name :params (...) :returns type)"],
                &name,
            ));
        }

        Ok(PureUtilityDecl { name, target, fns })
    }
}

impl FnDecl {
    fn from_sexp(node: &SexpNode) -> Result<Self, IrError> {
        let children = expect_list(node)?;
        if children.len() < 2 {
            return Err(err("fn", "missing function name", &["fn_name"], ""));
        }

        let name = expect_atom(&children[1])?.to_string();

        // Validate: function names must be snake_case identifiers
        if name.contains('-') {
            return Err(err(
                "fn",
                "function name must be snake_case (use _ not -)",
                &["detect_idle_prompt"],
                &name,
            ));
        }

        let returns = keyword_required(node, ":returns")?;
        let doc = node.keyword(":doc").map(String::from);
        let params = parse_params(node);
        let test_cases = parse_test_cases(node)?;

        Ok(FnDecl {
            name,
            params,
            returns,
            doc,
            test_cases,
        })
    }
}

/// Parse all `(pure-utility ...)` declarations from a top-level S-EXP list.
pub fn parse_pure_utilities(nodes: &[SexpNode]) -> Result<Vec<PureUtilityDecl>, IrError> {
    nodes
        .iter()
        .filter(|n| n.tag() == Some("pure-utility"))
        .map(PureUtilityDecl::from_sexp)
        .collect()
}

// ---------------------------------------------------------------------------
// Sub-parsers
// ---------------------------------------------------------------------------

fn parse_params(parent: &SexpNode) -> Vec<(String, String)> {
    let node = match parent.keyword_node(":params") {
        Some(n) => n,
        None => return vec![],
    };
    match node {
        SexpNode::List(items) => items
            .iter()
            .filter_map(|item| {
                if let SexpNode::List(parts) = item {
                    if parts.len() >= 2 {
                        let name = expect_atom(&parts[0]).ok()?.to_string();
                        let rust_type = expect_atom(&parts[1]).ok()?.to_string();
                        return Some((name, rust_type));
                    }
                }
                None
            })
            .collect(),
        _ => vec![],
    }
}

fn parse_test_cases(parent: &SexpNode) -> Result<Vec<TestCaseDecl>, IrError> {
    let node = match parent.keyword_node(":test-cases") {
        Some(n) => n,
        None => return Ok(vec![]),
    };
    let items = match node {
        SexpNode::List(items) => items,
        _ => return Ok(vec![]),
    };
    let mut cases = Vec::new();
    for item in items {
        if item.tag() == Some("case") {
            if let SexpNode::List(parts) = item {
                if parts.len() < 2 {
                    return Err(err("case", "missing case name", &["case_name"], ""));
                }
                let name = expect_atom(&parts[1])?.to_string();
                let input = item
                    .keyword(":input")
                    .map(String::from)
                    .unwrap_or_default();
                let expected = keyword_required(item, ":expected")?;
                cases.push(TestCaseDecl {
                    name,
                    input,
                    expected,
                });
            }
        }
    }
    Ok(cases)
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
        SexpNode::List(_) => Err(err("node", "expected atom", &["atom"], "(…)")),
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
    fn parse_basic_pure_utility() {
        let sexp = parse(
            r#"(pure-utility semantic-helpers
              :target "missiond-core/src/semantic/gen_helpers.rs"
              (fn detect_idle_prompt
                :params ((lines "&[&str]"))
                :returns bool
                :doc "Check if screen shows idle prompt")
              (fn extract_timer
                :params ((line "&str"))
                :returns "Option<Duration>"
                :doc "Extract timer pattern from line"))"#,
        );
        let decl = PureUtilityDecl::from_sexp(&sexp).unwrap();
        assert_eq!(decl.name, "semantic-helpers");
        assert_eq!(
            decl.target.as_deref(),
            Some("missiond-core/src/semantic/gen_helpers.rs")
        );
        assert_eq!(decl.fns.len(), 2);

        let f0 = &decl.fns[0];
        assert_eq!(f0.name, "detect_idle_prompt");
        assert_eq!(f0.params.len(), 1);
        assert_eq!(f0.params[0], ("lines".into(), "&[&str]".into()));
        assert_eq!(f0.returns, "bool");
        assert_eq!(f0.doc.as_deref(), Some("Check if screen shows idle prompt"));

        let f1 = &decl.fns[1];
        assert_eq!(f1.name, "extract_timer");
        assert_eq!(f1.returns, "Option<Duration>");
    }

    #[test]
    fn parse_with_test_cases() {
        let sexp = parse(
            r#"(pure-utility parser-helpers
              (fn is_spinner
                :params ((ch char))
                :returns bool
                :doc "Check if character is a spinner glyph"
                :test-cases
                  ((case "spinner-dot" :input "⠋" :expected true)
                   (case "normal-char" :input "A" :expected false)
                   (case "spinner-bar" :input "⣾" :expected true))))"#,
        );
        let decl = PureUtilityDecl::from_sexp(&sexp).unwrap();
        assert_eq!(decl.fns.len(), 1);

        let f = &decl.fns[0];
        assert_eq!(f.test_cases.len(), 3);
        assert_eq!(f.test_cases[0].name, "spinner-dot");
        assert_eq!(f.test_cases[0].input, "⠋");
        assert_eq!(f.test_cases[0].expected, "true");
        assert_eq!(f.test_cases[1].name, "normal-char");
        assert_eq!(f.test_cases[1].expected, "false");
    }

    #[test]
    fn reject_empty_utility() {
        let sexp = parse(r#"(pure-utility empty-module)"#);
        let err = PureUtilityDecl::from_sexp(&sexp).unwrap_err();
        assert_eq!(err.field, "pure-utility");
        assert!(err.message.contains("at least one"));
    }

    #[test]
    fn reject_hyphenated_fn_name() {
        let sexp = parse(
            r#"(pure-utility bad-names
              (fn detect-idle :params ((x i32)) :returns bool))"#,
        );
        let err = PureUtilityDecl::from_sexp(&sexp).unwrap_err();
        assert_eq!(err.field, "fn");
        assert!(err.message.contains("snake_case"));
    }

    #[test]
    fn reject_missing_returns() {
        let sexp = parse(
            r#"(pure-utility no-returns
              (fn foo :params ((x i32))))"#,
        );
        let err = PureUtilityDecl::from_sexp(&sexp).unwrap_err();
        assert_eq!(err.field, ":returns");
    }

    #[test]
    fn multiple_functions_no_target() {
        let sexp = parse(
            r#"(pure-utility math-helpers
              (fn clamp :params ((val f64) (min f64) (max f64)) :returns f64)
              (fn lerp :params ((a f64) (b f64) (t f64)) :returns f64)
              (fn normalize :params ((v f64) (lo f64) (hi f64)) :returns f64))"#,
        );
        let decl = PureUtilityDecl::from_sexp(&sexp).unwrap();
        assert_eq!(decl.name, "math-helpers");
        assert!(decl.target.is_none());
        assert_eq!(decl.fns.len(), 3);
        assert_eq!(decl.fns[0].name, "clamp");
        assert_eq!(decl.fns[1].name, "lerp");
        assert_eq!(decl.fns[2].name, "normalize");
        assert_eq!(decl.fns[2].params.len(), 3);
    }
}
