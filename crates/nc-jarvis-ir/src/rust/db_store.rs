//! DB Store IR — typed representation of `(db-store ...)` declarations.
//!
//! The "db-store" pattern generates PostgreSQL store boilerplate:
//! - Struct wrapping a `PgPool`
//! - Async trait with query method signatures
//! - Generation Gap: generated trait impl delegates to hand-written `_impl()` methods
//!
//! ```lisp
//! (db-store billing
//!   :table "billing_transactions"
//!   :entity "BillingTransaction"
//!   :trait-name "BillingStore"
//!   :queries (
//!     ("insert_transaction" :method insert :params ("tx: BillingTransaction"))
//!     ("get_transactions_by_tenant" :method select :params ("tenant_id: &str" "limit: i64" "offset: i64"))
//!     ("get_cost_summary" :method select :params ("tenant_id: &str" "start: OffsetDateTime" "end: OffsetDateTime"))))
//! ```

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
            "DbStore IR error in `{}`: {} (expected {:?}, got {:?})",
            self.field, self.message, self.expected, self.got
        )
    }
}

impl std::error::Error for IrError {}

// ---------------------------------------------------------------------------
// Query method kind
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum QueryMethod {
    Insert,
    Select,
    Update,
    Delete,
}

impl QueryMethod {
    pub fn from_str(s: &str) -> Result<Self, IrError> {
        match s {
            "insert" => Ok(Self::Insert),
            "select" => Ok(Self::Select),
            "update" => Ok(Self::Update),
            "delete" => Ok(Self::Delete),
            other => Err(err(
                ":method",
                "unknown query method",
                &["insert", "select", "update", "delete"],
                other,
            )),
        }
    }
}

// ---------------------------------------------------------------------------
// Query declaration
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct QueryDecl {
    pub name: String,
    pub method: QueryMethod,
    pub params: Vec<String>,
    pub returns: Option<String>,
}

// ---------------------------------------------------------------------------
// Top-level declaration
// ---------------------------------------------------------------------------

/// Top-level db-store declaration.
#[derive(Debug, Clone)]
pub struct DbStoreDecl {
    pub name: String,
    pub table: String,
    pub entity: String,
    pub trait_name: String,
    pub queries: Vec<QueryDecl>,
}

// ---------------------------------------------------------------------------
// S-EXP -> IR lowering
// ---------------------------------------------------------------------------

impl DbStoreDecl {
    pub fn from_sexp(node: &SexpNode) -> Result<Self, IrError> {
        let children = expect_list(node)?;
        if children.len() < 2 {
            return Err(err(
                "db-store",
                "missing db-store name",
                &["store_name"],
                "",
            ));
        }

        expect_tag(node, "db-store")?;
        let name = expect_atom(&children[1])?.to_string();

        let table = node
            .keyword(":table")
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("{name}s"));

        let entity = node
            .keyword(":entity")
            .map(|s| s.to_string())
            .unwrap_or_else(|| to_pascal_case(&name));

        let trait_name = node
            .keyword(":trait-name")
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("{}Store", to_pascal_case(&name)));

        let queries = parse_queries(node)?;

        Ok(DbStoreDecl {
            name,
            table,
            entity,
            trait_name,
            queries,
        })
    }
}

/// Parse all `(db-store ...)` declarations from a top-level S-EXP list.
pub fn parse_db_stores(nodes: &[SexpNode]) -> Result<Vec<DbStoreDecl>, IrError> {
    nodes
        .iter()
        .filter(|n| n.tag() == Some("db-store"))
        .map(DbStoreDecl::from_sexp)
        .collect()
}

// ---------------------------------------------------------------------------
// Sub-parsers
// ---------------------------------------------------------------------------

fn parse_queries(parent: &SexpNode) -> Result<Vec<QueryDecl>, IrError> {
    let node = match parent.keyword_node(":queries") {
        Some(n) => n,
        None => return Ok(vec![]),
    };
    let items = match node {
        SexpNode::List(items) => items,
        _ => return Ok(vec![]),
    };
    items
        .iter()
        .filter(|item| matches!(item, SexpNode::List(_)))
        .map(parse_single_query)
        .collect()
}

fn parse_single_query(node: &SexpNode) -> Result<QueryDecl, IrError> {
    let children = expect_list(node)?;
    if children.is_empty() {
        return Err(err("query", "empty query declaration", &["query_name"], ""));
    }

    let name = expect_atom(&children[0])?.to_string();

    let method = match node.keyword(":method") {
        Some(s) => QueryMethod::from_str(s)?,
        None => return Err(err(
            ":method",
            "missing :method in query",
            &["insert", "select", "update", "delete"],
            &name,
        )),
    };

    let params = parse_string_list(node, ":params");

    let returns = node.keyword(":returns").map(|s| s.to_string());

    Ok(QueryDecl {
        name,
        method,
        params,
        returns,
    })
}

fn parse_string_list(parent: &SexpNode, keyword: &str) -> Vec<String> {
    let node = match parent.keyword_node(keyword) {
        Some(n) => n,
        None => return vec![],
    };
    match node {
        SexpNode::List(items) => items
            .iter()
            .filter_map(|i| {
                if let SexpNode::Atom(s) = i {
                    if !s.starts_with(':') {
                        return Some(s.clone());
                    }
                }
                None
            })
            .collect(),
        SexpNode::Atom(s) => vec![s.clone()],
    }
}

// ---------------------------------------------------------------------------
// S-EXP utility helpers (same pattern as connector.rs)
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

fn to_pascal_case(s: &str) -> String {
    s.split(|c: char| c == '-' || c == '_')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(c) => c.to_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect()
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
    fn parse_full_db_store() {
        let sexp = parse(
            r#"(db-store billing
              :table "billing_transactions"
              :entity "BillingTransaction"
              :trait-name "BillingStore"
              :queries (
                ("insert_transaction" :method insert :params ("tx: BillingTransaction"))
                ("get_transactions_by_tenant" :method select :params ("tenant_id: &str" "limit: i64" "offset: i64"))
                ("get_cost_summary" :method select :params ("tenant_id: &str" "start: OffsetDateTime" "end: OffsetDateTime"))))"#,
        );
        let d = DbStoreDecl::from_sexp(&sexp).unwrap();
        assert_eq!(d.name, "billing");
        assert_eq!(d.table, "billing_transactions");
        assert_eq!(d.entity, "BillingTransaction");
        assert_eq!(d.trait_name, "BillingStore");
        assert_eq!(d.queries.len(), 3);

        assert_eq!(d.queries[0].name, "insert_transaction");
        assert_eq!(d.queries[0].method, QueryMethod::Insert);
        assert_eq!(d.queries[0].params, vec!["tx: BillingTransaction"]);
        assert!(d.queries[0].returns.is_none());

        assert_eq!(d.queries[1].name, "get_transactions_by_tenant");
        assert_eq!(d.queries[1].method, QueryMethod::Select);
        assert_eq!(d.queries[1].params.len(), 3);
        assert_eq!(d.queries[1].params[0], "tenant_id: &str");
        assert_eq!(d.queries[1].params[1], "limit: i64");
        assert_eq!(d.queries[1].params[2], "offset: i64");

        assert_eq!(d.queries[2].name, "get_cost_summary");
        assert_eq!(d.queries[2].method, QueryMethod::Select);
        assert_eq!(d.queries[2].params.len(), 3);
    }

    #[test]
    fn parse_minimal_db_store() {
        let sexp = parse(r#"(db-store user)"#);
        let d = DbStoreDecl::from_sexp(&sexp).unwrap();
        assert_eq!(d.name, "user");
        assert_eq!(d.table, "users");         // default: name + "s"
        assert_eq!(d.entity, "User");         // default: PascalCase(name)
        assert_eq!(d.trait_name, "UserStore"); // default: PascalCase(name) + "Store"
        assert!(d.queries.is_empty());
    }

    #[test]
    fn reject_invalid_query_method() {
        let sexp = parse(
            r#"(db-store x
              :queries (("bad_query" :method upsert :params ("id: i64"))))"#,
        );
        let err = DbStoreDecl::from_sexp(&sexp).unwrap_err();
        assert_eq!(err.field, ":method");
        assert!(err.got.contains("upsert"));
    }

    #[test]
    fn parse_query_with_returns() {
        let sexp = parse(
            r#"(db-store audit
              :table "audit_log"
              :entity "AuditEntry"
              :trait-name "AuditStore"
              :queries (
                ("find_by_id" :method select :params ("id: i64") :returns "Option<AuditEntry>")))"#,
        );
        let d = DbStoreDecl::from_sexp(&sexp).unwrap();
        assert_eq!(d.queries.len(), 1);
        assert_eq!(
            d.queries[0].returns.as_deref(),
            Some("Option<AuditEntry>")
        );
    }
}
