//! DB Store code generator.
//!
//! Generates PostgreSQL store boilerplate:
//! - Struct wrapping a `PgPool`
//! - Constructor
//! - Async trait impl with query method signatures
//! - Each query delegates to a hand-written `_impl()` method (Generation Gap)
//!
//! ## Generation Gap
//!
//! The generated file provides the scaffold; the custom file implements
//! each `query_name_impl()` with actual SQL queries.

use nc_jarvis_ir::db_store::*;

use crate::common::{blank, forge_header, w};

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub fn generate_db_store(decl: &DbStoreDecl) -> String {
    let mut out = String::with_capacity(4096);

    gen_header(&mut out, decl);
    gen_imports(&mut out);
    blank(&mut out);
    gen_struct(&mut out, decl);
    blank(&mut out);
    gen_constructor(&mut out, decl);
    blank(&mut out);
    gen_trait_impl(&mut out, decl);

    out
}

// ---------------------------------------------------------------------------
// Header
// ---------------------------------------------------------------------------

fn gen_header(out: &mut String, decl: &DbStoreDecl) {
    forge_header(
        out,
        "rust",
        &format!("intent.lisp (db-store {})", decl.name),
    );
    blank(out);
}

// ---------------------------------------------------------------------------
// Imports
// ---------------------------------------------------------------------------

fn gen_imports(out: &mut String) {
    w(out, 0, "use sqlx::PgPool;");
    w(out, 0, "use async_trait::async_trait;");
}

// ---------------------------------------------------------------------------
// Struct definition
// ---------------------------------------------------------------------------

fn gen_struct(out: &mut String, decl: &DbStoreDecl) {
    let struct_name = format!("Pg{}Store", to_pascal_case(&decl.name));
    w(out, 0, &format!("pub struct {struct_name} {{"));
    w(out, 1, "pub(crate) pool: PgPool,");
    w(out, 0, "}");
}

// ---------------------------------------------------------------------------
// Constructor
// ---------------------------------------------------------------------------

fn gen_constructor(out: &mut String, decl: &DbStoreDecl) {
    let struct_name = format!("Pg{}Store", to_pascal_case(&decl.name));
    w(out, 0, &format!("impl {struct_name} {{"));
    w(out, 1, "pub fn new(pool: PgPool) -> Self {");
    w(out, 2, "Self { pool }");
    w(out, 1, "}");
    w(out, 0, "}");
}

// ---------------------------------------------------------------------------
// Trait impl
// ---------------------------------------------------------------------------

fn gen_trait_impl(out: &mut String, decl: &DbStoreDecl) {
    let struct_name = format!("Pg{}Store", to_pascal_case(&decl.name));

    w(out, 0, "#[async_trait]");
    w(
        out,
        0,
        &format!("impl {} for {struct_name} {{", decl.trait_name),
    );

    for (i, query) in decl.queries.iter().enumerate() {
        if i > 0 {
            blank(out);
        }
        gen_query_method(out, query);
    }

    w(out, 0, "}");
}

fn gen_query_method(out: &mut String, query: &QueryDecl) {
    let return_type = resolve_return_type(query);

    // Method signature
    w(out, 1, &format!("async fn {}(", query.name));
    w(out, 2, "&self,");
    for param in &query.params {
        w(out, 2, &format!("{param},"));
    }
    w(
        out,
        1,
        &format!(") -> Result<{return_type}, Box<dyn std::error::Error + Send + Sync>> {{"),
    );

    // Delegation to hand-written impl
    w(
        out,
        2,
        "// Delegate to hand-written implementation (Generation Gap)",
    );

    let param_names: Vec<&str> = query
        .params
        .iter()
        .map(|p| {
            // Extract parameter name from "name: Type" pattern
            p.split(':').next().unwrap_or(p).trim()
        })
        .collect();
    let args = param_names.join(", ");

    w(
        out,
        2,
        &format!("self.{}_impl({args}).await", query.name),
    );

    w(out, 1, "}");
}

fn resolve_return_type(query: &QueryDecl) -> String {
    if let Some(ref ret) = query.returns {
        return ret.clone();
    }
    match query.method {
        QueryMethod::Insert => "()".to_string(),
        QueryMethod::Select => "Vec<serde_json::Value>".to_string(),
        QueryMethod::Update => "u64".to_string(),
        QueryMethod::Delete => "u64".to_string(),
    }
}

// ---------------------------------------------------------------------------
// Utilities
// ---------------------------------------------------------------------------

/// Convert kebab-case / snake_case to PascalCase: "billing" -> "Billing", "cost-tracker" -> "CostTracker"
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use nc_parser::SexpNode;

    fn parse_and_gen(input: &str) -> String {
        let sexp = SexpNode::parse_one(input).unwrap();
        let decl = DbStoreDecl::from_sexp(&sexp).unwrap();
        generate_db_store(&decl)
    }

    #[test]
    fn generates_full_db_store() {
        let output = parse_and_gen(
            r#"(db-store billing
              :table "billing_transactions"
              :entity "BillingTransaction"
              :trait-name "BillingStore"
              :queries (
                ("insert_transaction" :method insert :params ("tx: BillingTransaction"))
                ("get_transactions_by_tenant" :method select :params ("tenant_id: &str" "limit: i64" "offset: i64"))
                ("get_cost_summary" :method select :params ("tenant_id: &str" "start: OffsetDateTime" "end: OffsetDateTime"))))"#,
        );

        // Header
        assert!(output.contains("GENERATED BY FORGE"));
        assert!(output.contains("db-store billing"));

        // Imports
        assert!(output.contains("use sqlx::PgPool;"));
        assert!(output.contains("use async_trait::async_trait;"));

        // Struct
        assert!(output.contains("pub struct PgBillingStore"));
        assert!(output.contains("pub(crate) pool: PgPool,"));

        // Constructor
        assert!(output.contains("pub fn new(pool: PgPool) -> Self"));

        // Trait impl
        assert!(output.contains("#[async_trait]"));
        assert!(output.contains("impl BillingStore for PgBillingStore"));

        // Query methods
        assert!(output.contains("async fn insert_transaction("));
        assert!(output.contains("tx: BillingTransaction,"));
        assert!(output.contains("self.insert_transaction_impl(tx).await"));

        assert!(output.contains("async fn get_transactions_by_tenant("));
        assert!(output.contains("tenant_id: &str,"));
        assert!(output.contains("limit: i64,"));
        assert!(output.contains("offset: i64,"));
        assert!(output.contains("self.get_transactions_by_tenant_impl(tenant_id, limit, offset).await"));

        assert!(output.contains("async fn get_cost_summary("));
        assert!(output.contains("self.get_cost_summary_impl(tenant_id, start, end).await"));

        // Generation Gap comment
        assert!(output.contains("Delegate to hand-written implementation"));

        // Brace balance
        let open = output.matches('{').count();
        let close = output.matches('}').count();
        assert_eq!(open, close, "unbalanced: {open} vs {close}");
    }

    #[test]
    fn generates_minimal_db_store() {
        let output = parse_and_gen(r#"(db-store user)"#);

        assert!(output.contains("pub struct PgUserStore"));
        assert!(output.contains("impl UserStore for PgUserStore"));
        assert!(output.contains("pub fn new(pool: PgPool) -> Self"));
        // No query methods
        assert!(!output.contains("async fn"));

        let open = output.matches('{').count();
        let close = output.matches('}').count();
        assert_eq!(open, close, "unbalanced: {open} vs {close}");
    }

    #[test]
    fn generates_delete_and_update_return_types() {
        let output = parse_and_gen(
            r#"(db-store session
              :table "sessions"
              :entity "Session"
              :trait-name "SessionStore"
              :queries (
                ("delete_expired" :method delete :params ("before: OffsetDateTime"))
                ("update_last_seen" :method update :params ("session_id: &str" "now: OffsetDateTime"))))"#,
        );

        // Delete returns u64
        assert!(output.contains("Result<u64, Box<dyn std::error::Error + Send + Sync>>"));
        // Update returns u64
        assert!(output.contains("self.delete_expired_impl(before).await"));
        assert!(output.contains("self.update_last_seen_impl(session_id, now).await"));
    }

    #[test]
    fn generates_custom_return_type() {
        let output = parse_and_gen(
            r#"(db-store audit
              :queries (
                ("find_by_id" :method select :params ("id: i64") :returns "Option<AuditEntry>")))"#,
        );

        assert!(output.contains("Result<Option<AuditEntry>, Box<dyn std::error::Error + Send + Sync>>"));
    }
}
