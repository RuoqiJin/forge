//! Atom: row_to
//!
//! Generates `fn row_to_X(row: &rusqlite::Row) -> rusqlite::Result<X>` functions
//! for rusqlite row-to-struct mapping.
//!
//! Handles: simple get, unwrap_or defaults, enum parsing, JSON deserialization,
//! bool from i32 (since rusqlite bundled has no native bool support).

use nc_jarvis_ir::crud::{ColumnDecl, ColumnType, RowTypeDecl, TableDecl};

use super::w;

/// Generate a `row_to_X` function for a table with a row type declaration.
///
/// Output: `pub(crate) fn row_to_X(row: &rusqlite::Row) -> rusqlite::Result<X> { ... }`
pub fn gen_row_to(out: &mut String, indent: usize, table: &TableDecl, rt: &RowTypeDecl) {
    w(
        out,
        indent,
        &format!(
            "pub(crate) fn {}(row: &rusqlite::Row) -> rusqlite::Result<{}> {{",
            rt.convert_fn, rt.target_type
        ),
    );
    w(out, indent + 1, &format!("Ok({} {{", rt.target_type));

    for col in &table.columns {
        let field_code = gen_field_extract(col, rt);
        w(out, indent + 2, &format!("{}: {},", col.name, field_code));
    }

    w(out, indent + 1, "})");
    w(out, indent, "}");
}

/// Generate the extraction expression for a single column.
fn gen_field_extract(col: &ColumnDecl, rt: &RowTypeDecl) -> String {
    // Check if this field has an enum mapping
    if let Some(em) = rt.enum_maps.iter().find(|m| m.field == col.name) {
        return gen_enum_extract(col, &em.converter);
    }

    match col.col_type {
        ColumnType::Boolean => gen_bool_extract(col),
        ColumnType::Jsonb => gen_json_extract(col),
        _ => gen_simple_extract(col),
    }
}

/// Simple: `row.get("col")?` or `row.get("col").unwrap_or(default)`
fn gen_simple_extract(col: &ColumnDecl) -> String {
    if col.nullable {
        if let Some(ref d) = col.default {
            format!("row.get(\"{}\").unwrap_or(Some({}))", col.name, format_default(d, &col.col_type))
        } else {
            format!("row.get(\"{}\").unwrap_or(None)", col.name)
        }
    } else if let Some(ref d) = col.default {
        format!(
            "row.get(\"{}\").unwrap_or({})",
            col.name,
            format_default(d, &col.col_type)
        )
    } else {
        format!("row.get(\"{}\")?", col.name)
    }
}

/// Boolean: `{ let v: i32 = row.get("col").unwrap_or(0); v != 0 }`
fn gen_bool_extract(col: &ColumnDecl) -> String {
    let default_val = col
        .default
        .as_ref()
        .map(|d| if d == "true" || d == "1" { "1" } else { "0" })
        .unwrap_or("0");

    if col.nullable {
        format!(
            "{{ let v: Option<i32> = row.get(\"{}\").unwrap_or(None); v.map(|n| n != 0) }}",
            col.name
        )
    } else {
        format!(
            "{{ let v: i32 = row.get(\"{}\").unwrap_or({default_val}); v != 0 }}",
            col.name
        )
    }
}

/// JSON: `{ let raw: String = row.get("col").unwrap_or_else(|_| "{}".into()); serde_json::from_str(&raw).unwrap_or_default() }`
fn gen_json_extract(col: &ColumnDecl) -> String {
    let fallback = if col.nullable { "null" } else { "{}" };

    if col.nullable {
        format!(
            "{{ let raw: Option<String> = row.get(\"{}\").unwrap_or(None); raw.and_then(|s| serde_json::from_str(&s).ok()) }}",
            col.name
        )
    } else {
        format!(
            "{{ let raw: String = row.get(\"{}\").unwrap_or_else(|_| \"{fallback}\".to_string()); serde_json::from_str(&raw).unwrap_or_default() }}",
            col.name
        )
    }
}

/// Enum: `{ let s: String = row.get("col")?; converter_fn(&s) }`
fn gen_enum_extract(col: &ColumnDecl, converter: &str) -> String {
    if col.nullable {
        format!(
            "{{ let s: Option<String> = row.get(\"{}\").unwrap_or(None); s.map(|v| {converter}(&v)) }}",
            col.name
        )
    } else if let Some(ref d) = col.default {
        format!(
            "{{ let s: String = row.get(\"{}\").unwrap_or_else(|_| \"{d}\".to_string()); {converter}(&s) }}",
            col.name
        )
    } else {
        format!(
            "{{ let s: String = row.get(\"{}\")?; {converter}(&s) }}",
            col.name
        )
    }
}

/// Format a default value for Rust code.
fn format_default(default: &str, col_type: &ColumnType) -> String {
    match col_type {
        ColumnType::Integer | ColumnType::Bigint | ColumnType::Float8 => default.to_string(),
        ColumnType::Boolean => {
            if default == "true" || default == "1" {
                "1".to_string()
            } else {
                "0".to_string()
            }
        }
        _ => format!("\"{default}\".to_string()"),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use nc_jarvis_ir::crud::EnumMapDecl;

    fn make_col(name: &str, col_type: ColumnType, nullable: bool, default: Option<&str>) -> ColumnDecl {
        ColumnDecl {
            name: name.to_string(),
            col_type,
            primary_key: false,
            nullable,
            default: default.map(String::from),
            default_now: false,
            foreign_key: None,
            on_delete: None,
        }
    }

    fn make_rt(convert_fn: &str, target_type: &str, enum_maps: Vec<EnumMapDecl>) -> RowTypeDecl {
        RowTypeDecl {
            name: "Row".to_string(),
            derives: vec![],
            convert_fn: convert_fn.to_string(),
            target_type: target_type.to_string(),
            enum_maps,
        }
    }

    #[test]
    fn simple_text_field() {
        let col = make_col("title", ColumnType::Text, false, None);
        let rt = make_rt("row_to_x", "X", vec![]);
        assert_eq!(gen_field_extract(&col, &rt), "row.get(\"title\")?");
    }

    #[test]
    fn nullable_text_field() {
        let col = make_col("note", ColumnType::Text, true, None);
        let rt = make_rt("row_to_x", "X", vec![]);
        assert_eq!(gen_field_extract(&col, &rt), "row.get(\"note\").unwrap_or(None)");
    }

    #[test]
    fn text_with_default() {
        let col = make_col("status", ColumnType::Text, false, Some("open"));
        let rt = make_rt("row_to_x", "X", vec![]);
        assert_eq!(
            gen_field_extract(&col, &rt),
            "row.get(\"status\").unwrap_or(\"open\".to_string())"
        );
    }

    #[test]
    fn boolean_field() {
        let col = make_col("active", ColumnType::Boolean, false, None);
        let rt = make_rt("row_to_x", "X", vec![]);
        let code = gen_field_extract(&col, &rt);
        assert!(code.contains("i32"));
        assert!(code.contains("!= 0"));
    }

    #[test]
    fn json_field() {
        let col = make_col("metadata", ColumnType::Jsonb, false, None);
        let rt = make_rt("row_to_x", "X", vec![]);
        let code = gen_field_extract(&col, &rt);
        assert!(code.contains("serde_json::from_str"));
    }

    #[test]
    fn enum_mapped_field() {
        let col = make_col("status", ColumnType::Text, false, None);
        let rt = make_rt(
            "row_to_x",
            "X",
            vec![EnumMapDecl {
                field: "status".to_string(),
                converter: "status_from_str".to_string(),
                target_type: "Status".to_string(),
            }],
        );
        let code = gen_field_extract(&col, &rt);
        assert!(code.contains("status_from_str"));
    }

    #[test]
    fn full_row_to_output() {
        let table = TableDecl {
            name: "tasks".to_string(),
            columns: vec![
                make_col("id", ColumnType::Text, false, None),
                make_col("count", ColumnType::Integer, false, Some("0")),
                make_col("active", ColumnType::Boolean, false, None),
            ],
            indexes: vec![],
            unique_constraints: vec![],
            composite_pk: None,
            row_type: None,
            ops: vec![],
        };
        let rt = make_rt("row_to_task", "Task", vec![]);

        let mut out = String::new();
        gen_row_to(&mut out, 0, &table, &rt);

        assert!(out.contains("pub(crate) fn row_to_task"));
        assert!(out.contains("rusqlite::Result<Task>"));
        assert!(out.contains("row.get(\"id\")?"));
        assert!(out.contains("unwrap_or(0)"));
        assert!(out.contains("!= 0"));
    }
}
