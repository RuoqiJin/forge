//! Quark: type-map
//!
//! Cross-language type mapping. Maps between IR types, JSON Schema types,
//! and target-language types. Pure functions, no side effects.
//!
//! This is the single source of truth for all type conversions in the Forge
//! code generator. Previously scattered across tool_schema, arg_extract,
//! mcp_tool, and missiond_tool.

use nc_jarvis_ir::mcp_tool::ArgType;

/// Canonical JSON Schema type — the universal pivot between IR and target languages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JsonType {
    String,
    Integer,
    Number,
    Boolean,
    Array,
    Object,
}

impl JsonType {
    /// JSON Schema type string: `"string"`, `"integer"`, etc.
    pub fn as_json_str(&self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Integer => "integer",
            Self::Number => "number",
            Self::Boolean => "boolean",
            Self::Array => "array",
            Self::Object => "object",
        }
    }

    /// serde_json extractor method name: `"as_str()"`, `"as_i64()"`, etc.
    pub fn serde_extractor(&self) -> &'static str {
        match self {
            Self::String => "as_str()",
            Self::Integer => "as_i64()",
            Self::Number => "as_f64()",
            Self::Boolean => "as_bool()",
            Self::Array => "as_array()",
            Self::Object => "as_object()",
        }
    }

    /// Rust owned type: `"String"`, `"i64"`, `"f64"`, `"bool"`, etc.
    pub fn rust_owned_type(&self) -> &'static str {
        match self {
            Self::String => "String",
            Self::Integer => "i64",
            Self::Number => "f64",
            Self::Boolean => "bool",
            Self::Array => "&Vec<Value>",
            Self::Object => "&serde_json::Map<String, Value>",
        }
    }

    /// Rust reference type for trait signatures: `"&str"`, `"i64"`, `"&Value"`, etc.
    pub fn rust_ref_type(&self) -> &'static str {
        match self {
            Self::String => "&str",
            Self::Integer => "i64",
            Self::Number => "f64",
            Self::Boolean => "bool",
            Self::Array | Self::Object => "&Value",
        }
    }

    /// Zero/default value for Rust codegen.
    pub fn rust_default(&self) -> &'static str {
        match self {
            Self::String => "",
            Self::Integer => "0",
            Self::Number => "0.0",
            Self::Boolean => "false",
            Self::Array | Self::Object => "null",
        }
    }

    /// TypeScript type string.
    pub fn ts_type(&self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Integer | Self::Number => "number",
            Self::Boolean => "boolean",
            Self::Array => "unknown[]",
            Self::Object => "Record<string, unknown>",
        }
    }

    /// TypeScript default value for initializers.
    pub fn ts_default(&self) -> &'static str {
        match self {
            Self::String => "''",
            Self::Integer | Self::Number => "0",
            Self::Boolean => "false",
            Self::Array => "[]",
            Self::Object => "{}",
        }
    }

    /// Python type string (generic types only, no domain types).
    pub fn py_type(&self) -> &'static str {
        match self {
            Self::String => "str",
            Self::Integer => "int",
            Self::Number => "float",
            Self::Boolean => "bool",
            Self::Array => "list",
            Self::Object => "dict",
        }
    }

    /// Python default value for initializers.
    pub fn py_default(&self) -> &'static str {
        match self {
            Self::String => "''",
            Self::Integer => "0",
            Self::Number => "0.0",
            Self::Boolean => "False",
            Self::Array => "[]",
            Self::Object => "{}",
        }
    }

    /// Zod validator expression.
    pub fn zod_type(&self) -> &'static str {
        match self {
            Self::String => "z.string()",
            Self::Integer => "z.number().int()",
            Self::Number => "z.number()",
            Self::Boolean => "z.boolean()",
            Self::Array => "z.array(z.unknown())",
            Self::Object => "z.record(z.unknown())",
        }
    }
}

/// Convert from IR `ArgType` to `JsonType`.
/// This is the ONE place that bridges IR → gen types.
pub fn from_ir_arg_type(arg_type: &ArgType) -> JsonType {
    match arg_type {
        ArgType::String => JsonType::String,
        ArgType::Integer => JsonType::Integer,
        ArgType::Number => JsonType::Number,
        ArgType::Boolean => JsonType::Boolean,
        ArgType::Array => JsonType::Array,
        ArgType::Object => JsonType::Object,
    }
}

// ---------------------------------------------------------------------------
// CRUD Column Type → SQL Dialect Mapping
// ---------------------------------------------------------------------------

use nc_jarvis_ir::crud::ColumnType;

/// SQLite DDL type for a column type.
/// IR stays pure — the gen layer decides dialect-specific types.
pub fn sqlite_ddl_type(col_type: &ColumnType) -> &'static str {
    match col_type {
        ColumnType::Uuid => "TEXT",
        ColumnType::Text => "TEXT",
        ColumnType::Integer => "INTEGER",
        ColumnType::Bigint => "INTEGER",
        ColumnType::Boolean => "INTEGER",
        ColumnType::Float8 => "REAL",
        ColumnType::Jsonb => "TEXT",
        ColumnType::Timestamptz => "TEXT",
    }
}

/// Rust type that rusqlite's `Row::get()` returns for this column type.
/// Accounts for rusqlite with only `bundled` feature (no chrono/serde_json).
pub fn sqlite_rust_get_type(col_type: &ColumnType) -> &'static str {
    match col_type {
        ColumnType::Uuid | ColumnType::Text | ColumnType::Timestamptz => "String",
        ColumnType::Integer => "i32",
        ColumnType::Bigint => "i64",
        ColumnType::Boolean => "i32",
        ColumnType::Float8 => "f64",
        ColumnType::Jsonb => "String",
    }
}

/// Rust domain type (the type the field should be in the target struct).
pub fn sqlite_rust_domain_type(col_type: &ColumnType, nullable: bool) -> String {
    let base = match col_type {
        ColumnType::Uuid => "String",
        ColumnType::Text => "String",
        ColumnType::Integer => "i32",
        ColumnType::Bigint => "i64",
        ColumnType::Boolean => "bool",
        ColumnType::Float8 => "f64",
        ColumnType::Jsonb => "serde_json::Value",
        ColumnType::Timestamptz => "String",
    };
    if nullable {
        format!("Option<{base}>")
    } else {
        base.to_string()
    }
}

// ---------------------------------------------------------------------------
// PostgreSQL (sqlx) Column Type → Rust Type Mapping
// ---------------------------------------------------------------------------

/// PostgreSQL DDL type for a column type.
pub fn pg_ddl_type(col_type: &ColumnType) -> &'static str {
    match col_type {
        ColumnType::Uuid => "UUID",
        ColumnType::Text => "TEXT",
        ColumnType::Integer => "INTEGER",
        ColumnType::Bigint => "BIGINT",
        ColumnType::Boolean => "BOOLEAN",
        ColumnType::Float8 => "DOUBLE PRECISION",
        ColumnType::Jsonb => "JSONB",
        ColumnType::Timestamptz => "TIMESTAMPTZ",
    }
}

/// Rust domain type for PostgreSQL/sqlx — uses native types (Uuid, DateTime<Utc>)
/// unlike SQLite which maps everything to String.
pub fn pg_rust_domain_type(col_type: &ColumnType, nullable: bool) -> String {
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_str_all_variants() {
        assert_eq!(JsonType::String.as_json_str(), "string");
        assert_eq!(JsonType::Integer.as_json_str(), "integer");
        assert_eq!(JsonType::Number.as_json_str(), "number");
        assert_eq!(JsonType::Boolean.as_json_str(), "boolean");
        assert_eq!(JsonType::Array.as_json_str(), "array");
        assert_eq!(JsonType::Object.as_json_str(), "object");
    }

    #[test]
    fn serde_extractors() {
        assert_eq!(JsonType::String.serde_extractor(), "as_str()");
        assert_eq!(JsonType::Integer.serde_extractor(), "as_i64()");
        assert_eq!(JsonType::Number.serde_extractor(), "as_f64()");
        assert_eq!(JsonType::Boolean.serde_extractor(), "as_bool()");
        assert_eq!(JsonType::Array.serde_extractor(), "as_array()");
        assert_eq!(JsonType::Object.serde_extractor(), "as_object()");
    }

    #[test]
    fn rust_owned_types() {
        assert_eq!(JsonType::String.rust_owned_type(), "String");
        assert_eq!(JsonType::Integer.rust_owned_type(), "i64");
        assert_eq!(JsonType::Number.rust_owned_type(), "f64");
        assert_eq!(JsonType::Boolean.rust_owned_type(), "bool");
    }

    #[test]
    fn rust_ref_types() {
        assert_eq!(JsonType::String.rust_ref_type(), "&str");
        assert_eq!(JsonType::Integer.rust_ref_type(), "i64");
        assert_eq!(JsonType::Array.rust_ref_type(), "&Value");
        assert_eq!(JsonType::Object.rust_ref_type(), "&Value");
    }

    #[test]
    fn rust_defaults() {
        assert_eq!(JsonType::String.rust_default(), "");
        assert_eq!(JsonType::Integer.rust_default(), "0");
        assert_eq!(JsonType::Boolean.rust_default(), "false");
    }

    #[test]
    fn ts_types() {
        assert_eq!(JsonType::String.ts_type(), "string");
        assert_eq!(JsonType::Integer.ts_type(), "number");
        assert_eq!(JsonType::Number.ts_type(), "number");
        assert_eq!(JsonType::Boolean.ts_type(), "boolean");
        assert_eq!(JsonType::Array.ts_type(), "unknown[]");
        assert_eq!(JsonType::Object.ts_type(), "Record<string, unknown>");
    }

    #[test]
    fn ts_defaults() {
        assert_eq!(JsonType::String.ts_default(), "''");
        assert_eq!(JsonType::Integer.ts_default(), "0");
        assert_eq!(JsonType::Boolean.ts_default(), "false");
        assert_eq!(JsonType::Array.ts_default(), "[]");
        assert_eq!(JsonType::Object.ts_default(), "{}");
    }

    #[test]
    fn from_ir_roundtrip() {
        assert_eq!(from_ir_arg_type(&ArgType::String), JsonType::String);
        assert_eq!(from_ir_arg_type(&ArgType::Integer), JsonType::Integer);
        assert_eq!(from_ir_arg_type(&ArgType::Number), JsonType::Number);
        assert_eq!(from_ir_arg_type(&ArgType::Boolean), JsonType::Boolean);
        assert_eq!(from_ir_arg_type(&ArgType::Array), JsonType::Array);
        assert_eq!(from_ir_arg_type(&ArgType::Object), JsonType::Object);
    }

    #[test]
    fn sqlite_ddl_types() {
        assert_eq!(sqlite_ddl_type(&ColumnType::Uuid), "TEXT");
        assert_eq!(sqlite_ddl_type(&ColumnType::Text), "TEXT");
        assert_eq!(sqlite_ddl_type(&ColumnType::Integer), "INTEGER");
        assert_eq!(sqlite_ddl_type(&ColumnType::Bigint), "INTEGER");
        assert_eq!(sqlite_ddl_type(&ColumnType::Boolean), "INTEGER");
        assert_eq!(sqlite_ddl_type(&ColumnType::Float8), "REAL");
        assert_eq!(sqlite_ddl_type(&ColumnType::Jsonb), "TEXT");
        assert_eq!(sqlite_ddl_type(&ColumnType::Timestamptz), "TEXT");
    }

    #[test]
    fn sqlite_get_types() {
        assert_eq!(sqlite_rust_get_type(&ColumnType::Boolean), "i32");
        assert_eq!(sqlite_rust_get_type(&ColumnType::Jsonb), "String");
        assert_eq!(sqlite_rust_get_type(&ColumnType::Integer), "i32");
        assert_eq!(sqlite_rust_get_type(&ColumnType::Bigint), "i64");
    }

    #[test]
    fn sqlite_domain_types() {
        assert_eq!(sqlite_rust_domain_type(&ColumnType::Boolean, false), "bool");
        assert_eq!(sqlite_rust_domain_type(&ColumnType::Text, true), "Option<String>");
        assert_eq!(sqlite_rust_domain_type(&ColumnType::Jsonb, false), "serde_json::Value");
    }

    #[test]
    fn pg_ddl_types() {
        assert_eq!(pg_ddl_type(&ColumnType::Uuid), "UUID");
        assert_eq!(pg_ddl_type(&ColumnType::Text), "TEXT");
        assert_eq!(pg_ddl_type(&ColumnType::Integer), "INTEGER");
        assert_eq!(pg_ddl_type(&ColumnType::Bigint), "BIGINT");
        assert_eq!(pg_ddl_type(&ColumnType::Boolean), "BOOLEAN");
        assert_eq!(pg_ddl_type(&ColumnType::Float8), "DOUBLE PRECISION");
        assert_eq!(pg_ddl_type(&ColumnType::Jsonb), "JSONB");
        assert_eq!(pg_ddl_type(&ColumnType::Timestamptz), "TIMESTAMPTZ");
    }

    #[test]
    fn pg_domain_types() {
        assert_eq!(pg_rust_domain_type(&ColumnType::Uuid, false), "Uuid");
        assert_eq!(pg_rust_domain_type(&ColumnType::Timestamptz, false), "DateTime<Utc>");
        assert_eq!(pg_rust_domain_type(&ColumnType::Text, true), "Option<String>");
        assert_eq!(pg_rust_domain_type(&ColumnType::Jsonb, false), "serde_json::Value");
        assert_eq!(pg_rust_domain_type(&ColumnType::Boolean, true), "Option<bool>");
    }
}
