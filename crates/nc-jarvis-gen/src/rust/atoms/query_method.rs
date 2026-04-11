//! Atom: query_method
//!
//! Generates rusqlite query execution patterns:
//! - `conn.execute(sql, params![...])?` for INSERT/UPDATE/DELETE
//! - `conn.query_row(sql, params, |row| fn(row)).optional()?` for SELECT one
//! - `stmt.query_map(params, |row| fn(row))?.collect()` for SELECT many

use super::w;

/// Generate a `conn.execute()` call for INSERT/UPDATE/DELETE.
///
/// `conn_method` is "conn" for writes, "read_conn" for reads.
pub fn gen_rusqlite_execute(
    out: &mut String,
    indent: usize,
    sql: &str,
    binds: &[String],
    conn_method: &str,
) {
    w(out, indent, &format!("let conn = self.{conn_method}();"));

    if binds.is_empty() {
        w(out, indent, &format!("conn.execute(\"{sql}\", [])?;"));
    } else {
        w(out, indent, &format!("conn.execute("));
        w(out, indent + 1, &format!("\"{sql}\","));
        w(out, indent + 1, &format!("params![{}],", binds.join(", ")));
        w(out, indent, ")?;");
    }
}

/// Generate a `conn.query_row().optional()` call for SELECT one row.
///
/// Returns `Option<T>` where T is constructed by `row_fn`.
pub fn gen_rusqlite_query_one(
    out: &mut String,
    indent: usize,
    sql: &str,
    binds: &[String],
    row_fn: &str,
    conn_method: &str,
) {
    w(out, indent, &format!("let conn = self.{conn_method}();"));

    if binds.is_empty() {
        w(
            out,
            indent,
            &format!("conn.query_row(\"{sql}\", [], |row| {row_fn}(row))"),
        );
    } else {
        w(out, indent, "conn.query_row(");
        w(out, indent + 1, &format!("\"{sql}\","));
        w(out, indent + 1, &format!("params![{}],", binds.join(", ")));
        w(out, indent + 1, &format!("|row| {row_fn}(row),"));
        w(out, indent, ")");
    }
    w(out, indent + 1, ".optional()");
    w(out, indent + 1, ".map_err(Into::into)");
}

/// Generate a `stmt.query_map().collect()` call for SELECT many rows.
pub fn gen_rusqlite_query_many(
    out: &mut String,
    indent: usize,
    sql: &str,
    binds: &[String],
    row_fn: &str,
    conn_method: &str,
) {
    w(out, indent, &format!("let conn = self.{conn_method}();"));

    if binds.is_empty() {
        w(
            out,
            indent,
            &format!("let mut stmt = conn.prepare(\"{sql}\")?;"),
        );
        w(
            out,
            indent,
            &format!("let rows = stmt.query_map([], |row| {row_fn}(row))?;"),
        );
    } else {
        w(
            out,
            indent,
            &format!("let mut stmt = conn.prepare(\"{sql}\")?;"),
        );
        w(
            out,
            indent,
            &format!(
                "let rows = stmt.query_map(params![{}], |row| {row_fn}(row))?;",
                binds.join(", ")
            ),
        );
    }
    w(
        out,
        indent,
        "rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)",
    );
}

/// Generate a COUNT(*) query that returns i64.
pub fn gen_rusqlite_count(
    out: &mut String,
    indent: usize,
    sql: &str,
    binds: &[String],
    conn_method: &str,
) {
    w(out, indent, &format!("let conn = self.{conn_method}();"));

    if binds.is_empty() {
        w(
            out,
            indent,
            &format!("let count: i64 = conn.query_row(\"{sql}\", [], |row| row.get(0))?;"),
        );
    } else {
        w(
            out,
            indent,
            &format!(
                "let count: i64 = conn.query_row(\"{sql}\", params![{}], |row| row.get(0))?;",
                binds.join(", ")
            ),
        );
    }
    w(out, indent, "Ok(count)");
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn execute_with_binds() {
        let mut out = String::new();
        gen_rusqlite_execute(
            &mut out,
            0,
            "INSERT INTO t (a, b) VALUES (?1, ?2)",
            &["val_a".into(), "val_b".into()],
            "conn",
        );
        assert!(out.contains("self.conn()"));
        assert!(out.contains("params![val_a, val_b]"));
        assert!(out.contains("INSERT INTO t"));
    }

    #[test]
    fn query_one_with_optional() {
        let mut out = String::new();
        gen_rusqlite_query_one(
            &mut out,
            0,
            "SELECT * FROM t WHERE id = ?1",
            &["id".into()],
            "row_to_t",
            "read_conn",
        );
        assert!(out.contains("self.read_conn()"));
        assert!(out.contains(".optional()"));
        assert!(out.contains("row_to_t(row)"));
    }

    #[test]
    fn query_many_with_collect() {
        let mut out = String::new();
        gen_rusqlite_query_many(
            &mut out,
            0,
            "SELECT * FROM t",
            &[],
            "row_to_t",
            "read_conn",
        );
        assert!(out.contains("conn.prepare("));
        assert!(out.contains("query_map([]"));
        assert!(out.contains("collect::<Result<Vec<_>, _>>()"));
    }

    #[test]
    fn count_query() {
        let mut out = String::new();
        gen_rusqlite_count(
            &mut out,
            0,
            "SELECT COUNT(*) FROM t WHERE status = ?1",
            &["status".into()],
            "read_conn",
        );
        assert!(out.contains("count: i64"));
        assert!(out.contains("row.get(0)"));
    }
}
