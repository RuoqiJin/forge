//! Atom: sqlx_query
//!
//! Generates sqlx query method bodies for PostgreSQL CRUD operations.
//! Each function produces a String fragment containing the sqlx call chain:
//! query construction, `.bind(...)` calls, fetch/execute, and `.await?`.
//!
//! ## What it generates
//!
//! ```text
//! sqlx::query!("INSERT INTO sessions (id, title) VALUES ($1, $2)",
//!     id,
//!     title
//! )
//! .execute(&self.pool)
//! .await?;
//! ```
//!
//! ## What it does NOT generate
//!
//! - No function signatures (that's the pattern layer)
//! - No struct definitions (that's struct_def atom)
//! - No DDL statements (that's pg_ddl atom)

use super::w;
use nc_jarvis_ir::crud::ColumnType;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// The kind of sqlx operation to generate.
#[derive(Debug, Clone, PartialEq)]
pub enum SqlxOpKind {
    Insert,
    SelectOne,
    SelectMany,
    Update,
    Delete,
    Upsert,
    Count,
}

/// A single parameter to bind in the query.
#[derive(Debug, Clone)]
pub struct BindParam {
    /// Rust expression to bind (e.g. "id", "title", "offset as i64").
    pub expr: String,
}

/// A SET clause entry for UPDATE operations.
#[derive(Debug, Clone)]
pub struct SetClause {
    /// Column name in SQL.
    pub column: String,
    /// If Some, use a literal SQL expression instead of a placeholder (e.g. "now()").
    pub literal: Option<String>,
}

/// A WHERE clause condition.
#[derive(Debug, Clone)]
pub struct WhereCondition {
    /// Column name in SQL.
    pub column: String,
    /// SQL operator (defaults to "=" if None).
    pub op: Option<String>,
}

/// Input to the sqlx_query atom.
#[derive(Debug, Clone)]
pub struct SqlxQueryInput {
    /// Table name.
    pub table: String,
    /// Operation kind.
    pub op: SqlxOpKind,
    /// Columns to INSERT or SELECT (ignored for DELETE/COUNT).
    pub columns: Vec<String>,
    /// Parameters to bind (in placeholder order: $1, $2, ...).
    pub binds: Vec<BindParam>,
    /// WHERE conditions (appended after INSERT/SELECT columns).
    pub where_conditions: Vec<WhereCondition>,
    /// SET clauses for UPDATE.
    pub set_clauses: Vec<SetClause>,
    /// ON CONFLICT clause for UPSERT (e.g. "(id) DO UPDATE SET title = $2").
    pub on_conflict: Option<String>,
    /// ORDER BY clause (e.g. "created_at DESC").
    pub order_by: Option<String>,
    /// LIMIT expression (e.g. "$3").
    pub limit: Option<String>,
    /// OFFSET expression (e.g. "$4").
    pub offset: Option<String>,
    /// Row type name for query_as (e.g. "SessionRow"). If None, uses query! macro.
    pub row_type: Option<String>,
    /// Pool field name (defaults to "self.pool").
    pub pool_field: String,
    /// Indentation level for the generated code.
    pub indent: usize,
    /// Whether to use the compile-time checked `sqlx::query!` macro vs runtime `sqlx::query`.
    /// When true, binds are passed as macro arguments; when false, uses `.bind()` chain.
    pub use_macro: bool,
    /// Additional fixed WHERE conditions as raw SQL strings (e.g. "deleted_at IS NULL").
    pub where_fixed: Vec<String>,
    /// Extra preconditions appended to WHERE (for CAS updates).
    pub preconditions: Vec<String>,
}

impl Default for SqlxQueryInput {
    fn default() -> Self {
        Self {
            table: String::new(),
            op: SqlxOpKind::Insert,
            columns: vec![],
            binds: vec![],
            where_conditions: vec![],
            set_clauses: vec![],
            on_conflict: None,
            order_by: None,
            limit: None,
            offset: None,
            row_type: None,
            pool_field: "self.pool".into(),
            indent: 2,
            use_macro: true,
            where_fixed: vec![],
            preconditions: vec![],
        }
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Generate a complete sqlx query call chain.
///
/// Returns a String fragment ready to be inserted into a method body.
pub fn gen_sqlx_query(input: &SqlxQueryInput) -> String {
    let mut out = String::with_capacity(512);
    let i = input.indent;

    match input.op {
        SqlxOpKind::Insert => gen_insert(&mut out, input, i),
        SqlxOpKind::SelectOne => gen_select(&mut out, input, i, "fetch_optional"),
        SqlxOpKind::SelectMany => gen_select(&mut out, input, i, "fetch_all"),
        SqlxOpKind::Update => gen_update(&mut out, input, i),
        SqlxOpKind::Delete => gen_delete(&mut out, input, i),
        SqlxOpKind::Upsert => gen_upsert(&mut out, input, i),
        SqlxOpKind::Count => gen_count(&mut out, input, i),
    }

    out
}

// ---------------------------------------------------------------------------
// Internal generators
// ---------------------------------------------------------------------------

fn gen_insert(out: &mut String, input: &SqlxQueryInput, i: usize) {
    let placeholders: Vec<String> = (1..=input.columns.len())
        .map(|n| format!("${n}"))
        .collect();

    let sql = format!(
        "INSERT INTO {} ({}) VALUES ({})",
        input.table,
        input.columns.join(", "),
        placeholders.join(", ")
    );

    emit_query(out, input, i, &sql, "execute");
}

fn gen_select(out: &mut String, input: &SqlxQueryInput, i: usize, fetch: &str) {
    let select_cols = if input.columns.is_empty() {
        "*".to_string()
    } else {
        input.columns.join(", ")
    };

    let where_clause = build_where_clause(input);

    let mut sql = format!("SELECT {select_cols} FROM {}{where_clause}", input.table);

    if let Some(ref order) = input.order_by {
        sql.push_str(&format!(" ORDER BY {order}"));
    }
    if let Some(ref limit) = input.limit {
        sql.push_str(&format!(" LIMIT {limit}"));
    }
    if let Some(ref offset) = input.offset {
        sql.push_str(&format!(" OFFSET {offset}"));
    }

    emit_query(out, input, i, &sql, fetch);
}

fn gen_update(out: &mut String, input: &SqlxQueryInput, i: usize) {
    // Build SET clause
    let mut set_parts = Vec::new();
    let mut placeholder_idx = 1u32;

    for sc in &input.set_clauses {
        if let Some(ref lit) = sc.literal {
            set_parts.push(format!("{} = {lit}", sc.column));
        } else {
            set_parts.push(format!("{} = ${placeholder_idx}", sc.column));
            placeholder_idx += 1;
        }
    }

    // WHERE clause uses subsequent placeholders
    let mut where_parts = Vec::new();
    for wc in &input.where_conditions {
        let op = wc.op.as_deref().unwrap_or("=");
        where_parts.push(format!("{} {op} ${placeholder_idx}", wc.column));
        placeholder_idx += 1;
    }
    where_parts.extend(input.where_fixed.iter().cloned());
    where_parts.extend(input.preconditions.iter().cloned());

    let where_clause = if where_parts.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", where_parts.join(" AND "))
    };

    let sql = format!(
        "UPDATE {} SET {}{where_clause}",
        input.table,
        set_parts.join(", ")
    );

    emit_query(out, input, i, &sql, "execute");
}

fn gen_delete(out: &mut String, input: &SqlxQueryInput, i: usize) {
    let where_clause = build_where_clause(input);
    let sql = format!("DELETE FROM {}{where_clause}", input.table);
    emit_query(out, input, i, &sql, "execute");
}

fn gen_upsert(out: &mut String, input: &SqlxQueryInput, i: usize) {
    let placeholders: Vec<String> = (1..=input.columns.len())
        .map(|n| format!("${n}"))
        .collect();

    let conflict = input
        .on_conflict
        .as_deref()
        .unwrap_or("(id) DO NOTHING");

    let sql = format!(
        "INSERT INTO {} ({}) VALUES ({}) ON CONFLICT {conflict}",
        input.table,
        input.columns.join(", "),
        placeholders.join(", ")
    );

    emit_query(out, input, i, &sql, "execute");
}

fn gen_count(out: &mut String, input: &SqlxQueryInput, i: usize) {
    let where_clause = build_where_clause(input);
    let sql = format!("SELECT COUNT(*) FROM {}{where_clause}", input.table);

    // Count always uses query_as with tuple return, runtime bind
    w(out, i, &format!("let row: (i64,) = sqlx::query_as(\"{sql}\")"));
    for bind in &input.binds {
        w(out, i + 1, &format!(".bind({})", bind.expr));
    }
    w(out, i, &format!(".fetch_one(&{})", input.pool_field));
    w(out, i, ".await?;");
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Build a WHERE clause from conditions, starting placeholder numbering
/// after the columns (for SELECT/DELETE) or using explicit numbering.
fn build_where_clause(input: &SqlxQueryInput) -> String {
    if input.where_conditions.is_empty() && input.where_fixed.is_empty() {
        return String::new();
    }

    let mut parts = Vec::new();
    // For SELECT/DELETE, placeholders start at $1
    for (idx, wc) in input.where_conditions.iter().enumerate() {
        let placeholder_idx = idx as u32 + 1;
        let op = wc.op.as_deref().unwrap_or("=");
        parts.push(format!("{} {op} ${placeholder_idx}", wc.column));
    }
    parts.extend(input.where_fixed.iter().cloned());

    format!(" WHERE {}", parts.join(" AND "))
}

/// Emit the query call, bind chain, fetch/execute, and .await?.
fn emit_query(out: &mut String, input: &SqlxQueryInput, i: usize, sql: &str, terminal: &str) {
    if input.use_macro {
        // Compile-time checked: sqlx::query!("...", bind1, bind2)
        let query_fn = if let Some(ref rt) = input.row_type {
            format!("sqlx::query_as!({rt},")
        } else {
            "sqlx::query!(".to_string()
        };

        if input.binds.is_empty() {
            w(out, i, &format!("{query_fn}\"{sql}\")"));
        } else {
            w(out, i, &format!("{query_fn}\"{sql}\","));
            for (idx, bind) in input.binds.iter().enumerate() {
                let comma = if idx < input.binds.len() - 1 { "," } else { "" };
                w(out, i + 1, &format!("{}{comma}", bind.expr));
            }
            w(out, i, ")");
        }
        w(out, i, &format!(".{terminal}(&{})", input.pool_field));
        w(out, i, ".await?;");
    } else {
        // Runtime: sqlx::query("...").bind(x).bind(y)
        let query_fn = if let Some(ref rt) = input.row_type {
            format!("sqlx::query_as::<_, {rt}>(\"{sql}\")")
        } else {
            format!("sqlx::query(\"{sql}\")")
        };

        w(out, i, &query_fn);
        for bind in &input.binds {
            w(out, i + 1, &format!(".bind({})", bind.expr));
        }
        w(out, i, &format!(".{terminal}(&{})", input.pool_field));
        w(out, i, ".await?;");
    }
}

// ---------------------------------------------------------------------------
// Convenience: ColumnType -> Rust type (for atom-level use)
// ---------------------------------------------------------------------------

/// Map IR ColumnType to Rust type string.
pub fn column_type_to_rust(col_type: &ColumnType, nullable: bool) -> String {
    let base = match col_type {
        ColumnType::Uuid => "Uuid",
        ColumnType::Text => "String",
        ColumnType::Integer => "i32",
        ColumnType::Bigint => "i64",
        ColumnType::Boolean => "bool",
        ColumnType::Float8 => "f64",
        ColumnType::Jsonb => "serde_json::Value",
        ColumnType::Timestamptz => "DateTime<Utc>",
    };
    if nullable {
        format!("Option<{base}>")
    } else {
        base.to_string()
    }
}

/// Map IR ColumnType to parameter type for fn signatures (uses references for large types).
pub fn column_type_to_param(rust_type_str: &str) -> &str {
    match rust_type_str {
        "uuid" => "Uuid",
        "text" => "&str",
        "integer" | "int" => "i32",
        "bigint" => "i64",
        "boolean" | "bool" => "bool",
        "float8" => "f64",
        "jsonb" => "serde_json::Value",
        "timestamptz" => "DateTime<Utc>",
        other => other,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_macro_style() {
        let input = SqlxQueryInput {
            table: "sessions".into(),
            op: SqlxOpKind::Insert,
            columns: vec!["id".into(), "title".into(), "status".into()],
            binds: vec![
                BindParam { expr: "id".into() },
                BindParam { expr: "title".into() },
                BindParam { expr: "\"idle\"".into() },
            ],
            use_macro: true,
            ..Default::default()
        };

        let code = gen_sqlx_query(&input);
        assert!(code.contains("sqlx::query!(\"INSERT INTO sessions (id, title, status) VALUES ($1, $2, $3)\","));
        assert!(code.contains("id,"));
        assert!(code.contains("title,"));
        assert!(code.contains("\"idle\""));
        assert!(code.contains(".execute(&self.pool)"));
        assert!(code.contains(".await?;"));
    }

    #[test]
    fn insert_runtime_style() {
        let input = SqlxQueryInput {
            table: "messages".into(),
            op: SqlxOpKind::Insert,
            columns: vec!["id".into(), "content".into()],
            binds: vec![
                BindParam { expr: "&self.id".into() },
                BindParam { expr: "&self.content".into() },
            ],
            use_macro: false,
            ..Default::default()
        };

        let code = gen_sqlx_query(&input);
        assert!(code.contains("sqlx::query(\"INSERT INTO messages (id, content) VALUES ($1, $2)\")"));
        assert!(code.contains(".bind(&self.id)"));
        assert!(code.contains(".bind(&self.content)"));
        assert!(code.contains(".execute(&self.pool)"));
    }

    #[test]
    fn select_one_with_row_type() {
        let input = SqlxQueryInput {
            table: "sessions".into(),
            op: SqlxOpKind::SelectOne,
            columns: vec!["id".into(), "title".into(), "status".into()],
            binds: vec![BindParam { expr: "id".into() }],
            where_conditions: vec![WhereCondition { column: "id".into(), op: None }],
            row_type: Some("SessionRow".into()),
            use_macro: true,
            ..Default::default()
        };

        let code = gen_sqlx_query(&input);
        assert!(code.contains("sqlx::query_as!(SessionRow,"));
        assert!(code.contains("SELECT id, title, status FROM sessions WHERE id = $1"));
        assert!(code.contains(".fetch_optional(&self.pool)"));
    }

    #[test]
    fn select_many_with_order_limit() {
        let input = SqlxQueryInput {
            table: "messages".into(),
            op: SqlxOpKind::SelectMany,
            columns: vec!["id".into(), "content".into(), "created_at".into()],
            binds: vec![
                BindParam { expr: "session_id".into() },
                BindParam { expr: "limit".into() },
            ],
            where_conditions: vec![WhereCondition { column: "session_id".into(), op: None }],
            order_by: Some("created_at DESC".into()),
            limit: Some("$2".into()),
            use_macro: true,
            ..Default::default()
        };

        let code = gen_sqlx_query(&input);
        assert!(code.contains("SELECT id, content, created_at FROM messages WHERE session_id = $1 ORDER BY created_at DESC LIMIT $2"));
        assert!(code.contains(".fetch_all(&self.pool)"));
    }

    #[test]
    fn select_many_runtime_bind() {
        let input = SqlxQueryInput {
            table: "tasks".into(),
            op: SqlxOpKind::SelectMany,
            columns: vec!["id".into(), "name".into()],
            binds: vec![BindParam { expr: "&status".into() }],
            where_conditions: vec![WhereCondition { column: "status".into(), op: None }],
            row_type: Some("TaskRow".into()),
            use_macro: false,
            ..Default::default()
        };

        let code = gen_sqlx_query(&input);
        assert!(code.contains("sqlx::query_as::<_, TaskRow>(\"SELECT id, name FROM tasks WHERE status = $1\")"));
        assert!(code.contains(".bind(&status)"));
        assert!(code.contains(".fetch_all(&self.pool)"));
    }

    #[test]
    fn update_with_set_and_where() {
        let input = SqlxQueryInput {
            table: "sessions".into(),
            op: SqlxOpKind::Update,
            set_clauses: vec![
                SetClause { column: "title".into(), literal: None },
                SetClause { column: "updated_at".into(), literal: Some("now()".into()) },
            ],
            binds: vec![
                BindParam { expr: "new_title".into() },
                BindParam { expr: "session_id".into() },
            ],
            where_conditions: vec![WhereCondition { column: "id".into(), op: None }],
            use_macro: true,
            ..Default::default()
        };

        let code = gen_sqlx_query(&input);
        assert!(code.contains("UPDATE sessions SET title = $1, updated_at = now() WHERE id = $2"));
        assert!(code.contains("new_title,"));
        assert!(code.contains("session_id"));
        assert!(code.contains(".execute(&self.pool)"));
    }

    #[test]
    fn update_with_preconditions() {
        let input = SqlxQueryInput {
            table: "tasks".into(),
            op: SqlxOpKind::Update,
            set_clauses: vec![
                SetClause { column: "status".into(), literal: None },
            ],
            binds: vec![
                BindParam { expr: "new_status".into() },
                BindParam { expr: "task_id".into() },
            ],
            where_conditions: vec![WhereCondition { column: "id".into(), op: None }],
            preconditions: vec!["status = 'pending'".into()],
            use_macro: true,
            ..Default::default()
        };

        let code = gen_sqlx_query(&input);
        assert!(code.contains("WHERE id = $2 AND status = 'pending'"));
    }

    #[test]
    fn delete_simple() {
        let input = SqlxQueryInput {
            table: "sessions".into(),
            op: SqlxOpKind::Delete,
            binds: vec![BindParam { expr: "id".into() }],
            where_conditions: vec![WhereCondition { column: "id".into(), op: None }],
            use_macro: true,
            ..Default::default()
        };

        let code = gen_sqlx_query(&input);
        assert!(code.contains("DELETE FROM sessions WHERE id = $1"));
        assert!(code.contains(".execute(&self.pool)"));
    }

    #[test]
    fn upsert_on_conflict() {
        let input = SqlxQueryInput {
            table: "watcher_cursors".into(),
            op: SqlxOpKind::Upsert,
            columns: vec!["file_path".into(), "byte_offset".into()],
            binds: vec![
                BindParam { expr: "file_path".into() },
                BindParam { expr: "offset as i64".into() },
            ],
            on_conflict: Some("(file_path) DO UPDATE SET byte_offset = $2, updated_at = now()".into()),
            use_macro: true,
            ..Default::default()
        };

        let code = gen_sqlx_query(&input);
        assert!(code.contains("INSERT INTO watcher_cursors (file_path, byte_offset) VALUES ($1, $2) ON CONFLICT (file_path) DO UPDATE SET byte_offset = $2, updated_at = now()"));
        assert!(code.contains("offset as i64"));
    }

    #[test]
    fn count_query() {
        let input = SqlxQueryInput {
            table: "messages".into(),
            op: SqlxOpKind::Count,
            binds: vec![BindParam { expr: "session_id".into() }],
            where_conditions: vec![WhereCondition { column: "session_id".into(), op: None }],
            ..Default::default()
        };

        let code = gen_sqlx_query(&input);
        assert!(code.contains("let row: (i64,) = sqlx::query_as(\"SELECT COUNT(*) FROM messages WHERE session_id = $1\")"));
        assert!(code.contains(".bind(session_id)"));
        assert!(code.contains(".fetch_one(&self.pool)"));
    }

    #[test]
    fn select_with_fixed_where() {
        let input = SqlxQueryInput {
            table: "tasks".into(),
            op: SqlxOpKind::SelectMany,
            columns: vec!["id".into(), "title".into()],
            binds: vec![BindParam { expr: "user_id".into() }],
            where_conditions: vec![WhereCondition { column: "user_id".into(), op: None }],
            where_fixed: vec!["deleted_at IS NULL".into()],
            use_macro: true,
            ..Default::default()
        };

        let code = gen_sqlx_query(&input);
        assert!(code.contains("WHERE user_id = $1 AND deleted_at IS NULL"));
    }

    #[test]
    fn custom_pool_field() {
        let input = SqlxQueryInput {
            table: "logs".into(),
            op: SqlxOpKind::Delete,
            binds: vec![BindParam { expr: "id".into() }],
            where_conditions: vec![WhereCondition { column: "id".into(), op: None }],
            pool_field: "self.db_pool".into(),
            use_macro: true,
            ..Default::default()
        };

        let code = gen_sqlx_query(&input);
        assert!(code.contains(".execute(&self.db_pool)"));
    }

    #[test]
    fn column_type_mapping() {
        assert_eq!(column_type_to_rust(&ColumnType::Uuid, false), "Uuid");
        assert_eq!(column_type_to_rust(&ColumnType::Text, false), "String");
        assert_eq!(column_type_to_rust(&ColumnType::Integer, false), "i32");
        assert_eq!(column_type_to_rust(&ColumnType::Bigint, false), "i64");
        assert_eq!(column_type_to_rust(&ColumnType::Boolean, false), "bool");
        assert_eq!(column_type_to_rust(&ColumnType::Float8, false), "f64");
        assert_eq!(column_type_to_rust(&ColumnType::Jsonb, false), "serde_json::Value");
        assert_eq!(column_type_to_rust(&ColumnType::Timestamptz, false), "DateTime<Utc>");
        // Nullable wrapping
        assert_eq!(column_type_to_rust(&ColumnType::Text, true), "Option<String>");
        assert_eq!(column_type_to_rust(&ColumnType::Uuid, true), "Option<Uuid>");
    }

    #[test]
    fn param_type_mapping() {
        assert_eq!(column_type_to_param("uuid"), "Uuid");
        assert_eq!(column_type_to_param("text"), "&str");
        assert_eq!(column_type_to_param("integer"), "i32");
        assert_eq!(column_type_to_param("bigint"), "i64");
        assert_eq!(column_type_to_param("boolean"), "bool");
        assert_eq!(column_type_to_param("DateTime<Utc>"), "DateTime<Utc>");
    }

    #[test]
    fn insert_no_binds() {
        let input = SqlxQueryInput {
            table: "defaults_only".into(),
            op: SqlxOpKind::Insert,
            columns: vec![],
            binds: vec![],
            use_macro: true,
            ..Default::default()
        };

        let code = gen_sqlx_query(&input);
        assert!(code.contains("INSERT INTO defaults_only () VALUES ()"));
        assert!(code.contains(".execute(&self.pool)"));
    }

    #[test]
    fn select_all_columns() {
        let input = SqlxQueryInput {
            table: "things".into(),
            op: SqlxOpKind::SelectMany,
            columns: vec![],
            binds: vec![],
            use_macro: true,
            ..Default::default()
        };

        let code = gen_sqlx_query(&input);
        assert!(code.contains("SELECT * FROM things"));
    }

    #[test]
    fn select_with_offset() {
        let input = SqlxQueryInput {
            table: "items".into(),
            op: SqlxOpKind::SelectMany,
            columns: vec!["id".into()],
            binds: vec![
                BindParam { expr: "limit_val".into() },
                BindParam { expr: "offset_val".into() },
            ],
            order_by: Some("id ASC".into()),
            limit: Some("$1".into()),
            offset: Some("$2".into()),
            use_macro: true,
            ..Default::default()
        };

        let code = gen_sqlx_query(&input);
        assert!(code.contains("SELECT id FROM items ORDER BY id ASC LIMIT $1 OFFSET $2"));
    }
}
