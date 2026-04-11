//! Atom: ddl_column
//!
//! Generates a single column definition line for SQLite CREATE TABLE statements.
//! Uses quarks::type_map for dialect-specific type mapping.

use nc_jarvis_ir::crud::ColumnDecl;

use crate::quarks::type_map::sqlite_ddl_type;

/// Generate one SQLite column definition line (without trailing comma).
///
/// Examples:
/// - `id TEXT PRIMARY KEY`
/// - `count INTEGER NOT NULL DEFAULT 0`
/// - `created_at TEXT NOT NULL DEFAULT (datetime('now'))`
pub fn gen_sqlite_ddl_column(col: &ColumnDecl) -> String {
    let mut def = format!("{} {}", col.name, sqlite_ddl_type(&col.col_type));

    if col.primary_key {
        def.push_str(" PRIMARY KEY");
    }

    if col.default_now {
        def.push_str(" NOT NULL DEFAULT (datetime('now'))");
    } else if !col.nullable && !col.primary_key {
        def.push_str(" NOT NULL");
    }

    if let Some(ref d) = col.default {
        if d.parse::<f64>().is_ok() {
            def.push_str(&format!(" DEFAULT {d}"));
        } else {
            def.push_str(&format!(" DEFAULT '{d}'"));
        }
    }

    if let Some(ref fk) = col.foreign_key {
        def.push_str(&format!(" REFERENCES {fk}(id)"));
        if let Some(ref on_del) = col.on_delete {
            def.push_str(&format!(" ON DELETE {}", on_del.to_uppercase()));
        }
    }

    def
}

/// Generate an ALTER TABLE ADD COLUMN statement for SQLite column evolution.
///
/// Returns the full SQL string for the ADD COLUMN.
pub fn gen_sqlite_add_column(table_name: &str, col: &ColumnDecl) -> String {
    let mut col_def = sqlite_ddl_type(&col.col_type).to_string();

    if col.default_now {
        col_def.push_str(" DEFAULT (datetime('now'))");
    }
    if let Some(ref d) = col.default {
        if d.parse::<f64>().is_ok() {
            col_def.push_str(&format!(" DEFAULT {d}"));
        } else {
            col_def.push_str(&format!(" DEFAULT '{d}'"));
        }
    }

    format!(
        "ALTER TABLE {table_name} ADD COLUMN {} {col_def}",
        col.name
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use nc_jarvis_ir::crud::ColumnType;

    fn make_col(name: &str, col_type: ColumnType, pk: bool, nullable: bool, default: Option<&str>, default_now: bool) -> ColumnDecl {
        ColumnDecl {
            name: name.to_string(),
            col_type,
            primary_key: pk,
            nullable,
            default: default.map(String::from),
            default_now,
            foreign_key: None,
            on_delete: None,
        }
    }

    #[test]
    fn pk_column() {
        let col = make_col("id", ColumnType::Text, true, false, None, false);
        assert_eq!(gen_sqlite_ddl_column(&col), "id TEXT PRIMARY KEY");
    }

    #[test]
    fn not_null_with_default() {
        let col = make_col("count", ColumnType::Integer, false, false, Some("0"), false);
        assert_eq!(gen_sqlite_ddl_column(&col), "count INTEGER NOT NULL DEFAULT 0");
    }

    #[test]
    fn text_default() {
        let col = make_col("status", ColumnType::Text, false, false, Some("open"), false);
        assert_eq!(gen_sqlite_ddl_column(&col), "status TEXT NOT NULL DEFAULT 'open'");
    }

    #[test]
    fn default_now() {
        let col = make_col("created_at", ColumnType::Timestamptz, false, false, None, true);
        assert_eq!(
            gen_sqlite_ddl_column(&col),
            "created_at TEXT NOT NULL DEFAULT (datetime('now'))"
        );
    }

    #[test]
    fn nullable_column() {
        let col = make_col("note", ColumnType::Text, false, true, None, false);
        assert_eq!(gen_sqlite_ddl_column(&col), "note TEXT");
    }

    #[test]
    fn boolean_column() {
        let col = make_col("active", ColumnType::Boolean, false, false, None, false);
        assert_eq!(gen_sqlite_ddl_column(&col), "active INTEGER NOT NULL");
    }

    #[test]
    fn add_column() {
        let col = make_col("new_field", ColumnType::Text, false, false, Some("default"), false);
        let sql = gen_sqlite_add_column("tasks", &col);
        assert_eq!(sql, "ALTER TABLE tasks ADD COLUMN new_field TEXT DEFAULT 'default'");
    }
}
