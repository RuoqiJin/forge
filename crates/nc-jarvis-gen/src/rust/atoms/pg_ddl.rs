//! Atom: pg_ddl
//!
//! Generates PostgreSQL DDL statements: CREATE TABLE, ALTER TABLE ADD COLUMN,
//! CREATE INDEX, and the sqlx execution wrappers around them.
//!
//! ## What it generates
//!
//! ```text
//! "CREATE TABLE IF NOT EXISTS sessions (
//!     id UUID PRIMARY KEY,
//!     title TEXT NOT NULL,
//!     data JSONB,
//!     created_at TIMESTAMPTZ NOT NULL DEFAULT now()
//! )"
//! ```
//!
//! ## What it does NOT generate
//!
//! - No Rust struct definitions (that's struct_def atom)
//! - No query methods (that's sqlx_query atom)
//! - No S-EXP parsing (that's the IR layer)

use super::w;
use nc_jarvis_ir::crud::{ColumnDecl, IndexDecl, UniqueConstraint, CompositePk};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Input for generating a CREATE TABLE statement.
#[derive(Debug, Clone)]
pub struct CreateTableInput {
    /// Table name.
    pub table: String,
    /// Column definitions.
    pub columns: Vec<ColumnDecl>,
    /// Composite primary key (if any). Single-column PKs use column-level PRIMARY KEY.
    pub composite_pk: Option<CompositePk>,
    /// UNIQUE constraints.
    pub unique_constraints: Vec<UniqueConstraint>,
}

/// Input for generating ALTER TABLE ADD COLUMN statements (idempotent migration).
#[derive(Debug, Clone)]
pub struct AlterColumnInput {
    /// Table name.
    pub table: String,
    /// Column to ensure exists.
    pub column: ColumnDecl,
}

/// Input for generating CREATE INDEX statement.
#[derive(Debug, Clone)]
pub struct CreateIndexInput {
    /// The index definition.
    pub index: IndexDecl,
    /// Table to index.
    pub table: String,
}

/// Input for generating a complete DDL + execute block for a table migration.
#[derive(Debug, Clone)]
pub struct MigrateTableInput {
    /// Table name.
    pub table: String,
    /// Column definitions.
    pub columns: Vec<ColumnDecl>,
    /// Composite primary key (if any).
    pub composite_pk: Option<CompositePk>,
    /// UNIQUE constraints.
    pub unique_constraints: Vec<UniqueConstraint>,
    /// Indexes to create.
    pub indexes: Vec<IndexDecl>,
    /// Pool field name (e.g. "self.pool").
    pub pool_field: String,
    /// Indentation level.
    pub indent: usize,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Generate a raw CREATE TABLE DDL string (no sqlx wrapper).
pub fn gen_create_table_ddl(input: &CreateTableInput) -> String {
    let mut ddl = format!("CREATE TABLE IF NOT EXISTS {} (\n", input.table);
    let mut col_defs = Vec::new();
    let mut pk_cols: Vec<&str> = Vec::new();

    for col in &input.columns {
        let mut def = format!("    {} {}", col.name, col.col_type.sql_type());
        if col.primary_key {
            pk_cols.push(&col.name);
        }
        if col.default_now {
            def.push_str(" NOT NULL DEFAULT now()");
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
        col_defs.push(def);
    }

    // Inline single-column PK
    if pk_cols.len() == 1 {
        if let Some(pos) = col_defs
            .iter()
            .position(|d| d.trim_start().starts_with(pk_cols[0]))
        {
            col_defs[pos] = col_defs[pos].replace(
                &format!("{} {}", pk_cols[0], input.columns[pos].col_type.sql_type()),
                &format!(
                    "{} {} PRIMARY KEY",
                    pk_cols[0],
                    input.columns[pos].col_type.sql_type()
                ),
            );
        }
    }

    // Composite PK
    if let Some(ref cpk) = input.composite_pk {
        col_defs.push(format!("    PRIMARY KEY ({})", cpk.columns.join(", ")));
    }

    // UNIQUE constraints
    for uc in &input.unique_constraints {
        col_defs.push(format!("    UNIQUE({})", uc.columns.join(", ")));
    }

    ddl.push_str(&col_defs.join(",\n"));
    ddl.push_str("\n)");
    ddl
}

/// Generate a raw ALTER TABLE ADD COLUMN DDL string (idempotent via DO $$ block).
pub fn gen_alter_column_ddl(input: &AlterColumnInput) -> String {
    let mut col_def = input.column.col_type.sql_type().to_string();
    if input.column.default_now {
        col_def.push_str(" NOT NULL DEFAULT now()");
    }
    if let Some(ref d) = input.column.default {
        if d.parse::<f64>().is_ok() {
            col_def.push_str(&format!(" DEFAULT {d}"));
        } else {
            col_def.push_str(&format!(" DEFAULT '{d}'"));
        }
    }

    format!(
        "DO $$ BEGIN IF NOT EXISTS (SELECT 1 FROM information_schema.columns WHERE table_name='{}' AND column_name='{}') THEN ALTER TABLE {} ADD COLUMN {} {}; END IF; END $$",
        input.table, input.column.name, input.table, input.column.name, col_def
    )
}

/// Generate a raw CREATE INDEX DDL string.
pub fn gen_create_index_ddl(input: &CreateIndexInput) -> String {
    let unique = if input.index.unique { "UNIQUE " } else { "" };
    format!(
        "CREATE {}INDEX IF NOT EXISTS {} ON {}({})",
        unique,
        input.index.name,
        input.table,
        input.index.columns.join(", ")
    )
}

/// Generate a complete migration block for a table: CREATE TABLE + ALTER columns + CREATE INDEXes.
///
/// Wraps each DDL in `sqlx::query("...").execute(&pool).await?;`.
pub fn gen_migrate_table(input: &MigrateTableInput) -> String {
    let mut out = String::with_capacity(2048);
    let i = input.indent;

    // CREATE TABLE
    let create_input = CreateTableInput {
        table: input.table.clone(),
        columns: input.columns.clone(),
        composite_pk: input.composite_pk.clone(),
        unique_constraints: input.unique_constraints.clone(),
    };
    let ddl = gen_create_table_ddl(&create_input);
    w(&mut out, i, &format!("sqlx::query(\"{ddl}\")"));
    w(&mut out, i + 1, &format!(".execute(&{})", input.pool_field));
    w(&mut out, i + 1, ".await?;\n");

    // ALTER TABLE — ensure columns exist on pre-existing tables
    for col in &input.columns {
        let alter_input = AlterColumnInput {
            table: input.table.clone(),
            column: col.clone(),
        };
        let alter = gen_alter_column_ddl(&alter_input);
        w(&mut out, i, &format!("sqlx::query(\"{alter}\")"));
        w(&mut out, i + 1, &format!(".execute(&{})", input.pool_field));
        w(&mut out, i + 1, ".await?;\n");
    }

    // CREATE INDEX
    for idx in &input.indexes {
        let idx_input = CreateIndexInput {
            index: idx.clone(),
            table: input.table.clone(),
        };
        let sql = gen_create_index_ddl(&idx_input);
        w(&mut out, i, &format!("sqlx::query(\"{sql}\")"));
        w(&mut out, i + 1, &format!(".execute(&{})", input.pool_field));
        w(&mut out, i + 1, ".await?;\n");
    }

    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use nc_jarvis_ir::crud::ColumnType;

    fn text_col(name: &str) -> ColumnDecl {
        ColumnDecl {
            name: name.into(),
            col_type: ColumnType::Text,
            primary_key: false,
            nullable: false,
            default: None,
            default_now: false,
            foreign_key: None,
            on_delete: None,
        }
    }

    fn uuid_pk(name: &str) -> ColumnDecl {
        ColumnDecl {
            name: name.into(),
            col_type: ColumnType::Uuid,
            primary_key: true,
            nullable: false,
            default: None,
            default_now: false,
            foreign_key: None,
            on_delete: None,
        }
    }

    #[test]
    fn create_table_basic() {
        let input = CreateTableInput {
            table: "users".into(),
            columns: vec![
                uuid_pk("id"),
                text_col("name"),
            ],
            composite_pk: None,
            unique_constraints: vec![],
        };

        let ddl = gen_create_table_ddl(&input);
        assert!(ddl.contains("CREATE TABLE IF NOT EXISTS users"));
        assert!(ddl.contains("id UUID PRIMARY KEY"));
        assert!(ddl.contains("name TEXT NOT NULL"));
    }

    #[test]
    fn create_table_with_defaults() {
        let input = CreateTableInput {
            table: "counters".into(),
            columns: vec![
                uuid_pk("id"),
                ColumnDecl {
                    name: "count".into(),
                    col_type: ColumnType::Integer,
                    primary_key: false,
                    nullable: false,
                    default: Some("0".into()),
                    default_now: false,
                    foreign_key: None,
                    on_delete: None,
                },
                ColumnDecl {
                    name: "label".into(),
                    col_type: ColumnType::Text,
                    primary_key: false,
                    nullable: false,
                    default: Some("unnamed".into()),
                    default_now: false,
                    foreign_key: None,
                    on_delete: None,
                },
            ],
            composite_pk: None,
            unique_constraints: vec![],
        };

        let ddl = gen_create_table_ddl(&input);
        assert!(ddl.contains("count INTEGER NOT NULL DEFAULT 0"));
        assert!(ddl.contains("label TEXT NOT NULL DEFAULT 'unnamed'"));
    }

    #[test]
    fn create_table_with_timestamptz_default_now() {
        let input = CreateTableInput {
            table: "events".into(),
            columns: vec![
                uuid_pk("id"),
                ColumnDecl {
                    name: "created_at".into(),
                    col_type: ColumnType::Timestamptz,
                    primary_key: false,
                    nullable: false,
                    default: None,
                    default_now: true,
                    foreign_key: None,
                    on_delete: None,
                },
            ],
            composite_pk: None,
            unique_constraints: vec![],
        };

        let ddl = gen_create_table_ddl(&input);
        assert!(ddl.contains("created_at TIMESTAMPTZ NOT NULL DEFAULT now()"));
    }

    #[test]
    fn create_table_nullable_columns() {
        let input = CreateTableInput {
            table: "profiles".into(),
            columns: vec![
                uuid_pk("id"),
                ColumnDecl {
                    name: "bio".into(),
                    col_type: ColumnType::Text,
                    primary_key: false,
                    nullable: true,
                    default: None,
                    default_now: false,
                    foreign_key: None,
                    on_delete: None,
                },
                ColumnDecl {
                    name: "data".into(),
                    col_type: ColumnType::Jsonb,
                    primary_key: false,
                    nullable: true,
                    default: None,
                    default_now: false,
                    foreign_key: None,
                    on_delete: None,
                },
            ],
            composite_pk: None,
            unique_constraints: vec![],
        };

        let ddl = gen_create_table_ddl(&input);
        // Nullable columns should NOT have NOT NULL
        assert!(ddl.contains("    bio TEXT,"));
        assert!(ddl.contains("    data JSONB"));
        assert!(!ddl.contains("bio TEXT NOT NULL"));
    }

    #[test]
    fn create_table_with_foreign_key() {
        let input = CreateTableInput {
            table: "messages".into(),
            columns: vec![
                uuid_pk("id"),
                ColumnDecl {
                    name: "session_id".into(),
                    col_type: ColumnType::Uuid,
                    primary_key: false,
                    nullable: false,
                    default: None,
                    default_now: false,
                    foreign_key: Some("sessions".into()),
                    on_delete: Some("cascade".into()),
                },
            ],
            composite_pk: None,
            unique_constraints: vec![],
        };

        let ddl = gen_create_table_ddl(&input);
        assert!(ddl.contains("REFERENCES sessions(id)"));
        assert!(ddl.contains("ON DELETE CASCADE"));
    }

    #[test]
    fn create_table_composite_pk() {
        let input = CreateTableInput {
            table: "pivot".into(),
            columns: vec![
                ColumnDecl {
                    name: "left_id".into(),
                    col_type: ColumnType::Uuid,
                    primary_key: false,
                    nullable: false,
                    default: None,
                    default_now: false,
                    foreign_key: None,
                    on_delete: None,
                },
                ColumnDecl {
                    name: "right_id".into(),
                    col_type: ColumnType::Uuid,
                    primary_key: false,
                    nullable: false,
                    default: None,
                    default_now: false,
                    foreign_key: None,
                    on_delete: None,
                },
            ],
            composite_pk: Some(CompositePk {
                columns: vec!["left_id".into(), "right_id".into()],
            }),
            unique_constraints: vec![],
        };

        let ddl = gen_create_table_ddl(&input);
        assert!(ddl.contains("PRIMARY KEY (left_id, right_id)"));
    }

    #[test]
    fn create_table_unique_constraint() {
        let input = CreateTableInput {
            table: "users".into(),
            columns: vec![
                uuid_pk("id"),
                text_col("email"),
            ],
            composite_pk: None,
            unique_constraints: vec![UniqueConstraint {
                columns: vec!["email".into()],
            }],
        };

        let ddl = gen_create_table_ddl(&input);
        assert!(ddl.contains("UNIQUE(email)"));
    }

    #[test]
    fn alter_column_basic() {
        let input = AlterColumnInput {
            table: "users".into(),
            column: text_col("nickname"),
        };

        let ddl = gen_alter_column_ddl(&input);
        assert!(ddl.contains("information_schema.columns"));
        assert!(ddl.contains("table_name='users'"));
        assert!(ddl.contains("column_name='nickname'"));
        assert!(ddl.contains("ALTER TABLE users ADD COLUMN nickname TEXT"));
    }

    #[test]
    fn alter_column_with_default() {
        let input = AlterColumnInput {
            table: "counters".into(),
            column: ColumnDecl {
                name: "total".into(),
                col_type: ColumnType::Integer,
                primary_key: false,
                nullable: false,
                default: Some("0".into()),
                default_now: false,
                foreign_key: None,
                on_delete: None,
            },
        };

        let ddl = gen_alter_column_ddl(&input);
        assert!(ddl.contains("ADD COLUMN total INTEGER DEFAULT 0"));
    }

    #[test]
    fn alter_column_default_now() {
        let input = AlterColumnInput {
            table: "events".into(),
            column: ColumnDecl {
                name: "created_at".into(),
                col_type: ColumnType::Timestamptz,
                primary_key: false,
                nullable: false,
                default: None,
                default_now: true,
                foreign_key: None,
                on_delete: None,
            },
        };

        let ddl = gen_alter_column_ddl(&input);
        assert!(ddl.contains("ADD COLUMN created_at TIMESTAMPTZ NOT NULL DEFAULT now()"));
    }

    #[test]
    fn create_index_basic() {
        let input = CreateIndexInput {
            table: "messages".into(),
            index: IndexDecl {
                name: "idx_messages_session_id".into(),
                columns: vec!["session_id".into()],
                unique: false,
            },
        };

        let sql = gen_create_index_ddl(&input);
        assert_eq!(
            sql,
            "CREATE INDEX IF NOT EXISTS idx_messages_session_id ON messages(session_id)"
        );
    }

    #[test]
    fn create_index_unique_multi_column() {
        let input = CreateIndexInput {
            table: "tags".into(),
            index: IndexDecl {
                name: "idx_tags_name_scope".into(),
                columns: vec!["name".into(), "scope".into()],
                unique: true,
            },
        };

        let sql = gen_create_index_ddl(&input);
        assert!(sql.contains("CREATE UNIQUE INDEX IF NOT EXISTS"));
        assert!(sql.contains("ON tags(name, scope)"));
    }

    #[test]
    fn migrate_table_full() {
        let input = MigrateTableInput {
            table: "sessions".into(),
            columns: vec![
                uuid_pk("id"),
                text_col("title"),
                ColumnDecl {
                    name: "created_at".into(),
                    col_type: ColumnType::Timestamptz,
                    primary_key: false,
                    nullable: false,
                    default: None,
                    default_now: true,
                    foreign_key: None,
                    on_delete: None,
                },
            ],
            composite_pk: None,
            unique_constraints: vec![],
            indexes: vec![IndexDecl {
                name: "idx_sessions_created".into(),
                columns: vec!["created_at".into()],
                unique: false,
            }],
            pool_field: "self.pool".into(),
            indent: 2,
        };

        let code = gen_migrate_table(&input);

        // CREATE TABLE
        assert!(code.contains("CREATE TABLE IF NOT EXISTS sessions"));
        assert!(code.contains("id UUID PRIMARY KEY"));
        assert!(code.contains("title TEXT NOT NULL"));
        assert!(code.contains("created_at TIMESTAMPTZ NOT NULL DEFAULT now()"));

        // ALTER TABLE for each column
        assert!(code.contains("ALTER TABLE sessions ADD COLUMN id"));
        assert!(code.contains("ALTER TABLE sessions ADD COLUMN title"));
        assert!(code.contains("ALTER TABLE sessions ADD COLUMN created_at"));

        // CREATE INDEX
        assert!(code.contains("CREATE INDEX IF NOT EXISTS idx_sessions_created ON sessions(created_at)"));

        // sqlx wrappers
        assert!(code.contains("sqlx::query(\""));
        assert!(code.contains(".execute(&self.pool)"));
        assert!(code.contains(".await?;"));
    }

    #[test]
    fn all_column_types_in_ddl() {
        let input = CreateTableInput {
            table: "all_types".into(),
            columns: vec![
                ColumnDecl { name: "a".into(), col_type: ColumnType::Uuid, primary_key: true, nullable: false, default: None, default_now: false, foreign_key: None, on_delete: None },
                ColumnDecl { name: "b".into(), col_type: ColumnType::Text, primary_key: false, nullable: false, default: None, default_now: false, foreign_key: None, on_delete: None },
                ColumnDecl { name: "c".into(), col_type: ColumnType::Integer, primary_key: false, nullable: false, default: None, default_now: false, foreign_key: None, on_delete: None },
                ColumnDecl { name: "d".into(), col_type: ColumnType::Bigint, primary_key: false, nullable: false, default: None, default_now: false, foreign_key: None, on_delete: None },
                ColumnDecl { name: "e".into(), col_type: ColumnType::Boolean, primary_key: false, nullable: false, default: None, default_now: false, foreign_key: None, on_delete: None },
                ColumnDecl { name: "f".into(), col_type: ColumnType::Float8, primary_key: false, nullable: false, default: None, default_now: false, foreign_key: None, on_delete: None },
                ColumnDecl { name: "g".into(), col_type: ColumnType::Jsonb, primary_key: false, nullable: true, default: None, default_now: false, foreign_key: None, on_delete: None },
                ColumnDecl { name: "h".into(), col_type: ColumnType::Timestamptz, primary_key: false, nullable: false, default: None, default_now: true, foreign_key: None, on_delete: None },
            ],
            composite_pk: None,
            unique_constraints: vec![],
        };

        let ddl = gen_create_table_ddl(&input);
        assert!(ddl.contains("a UUID PRIMARY KEY"));
        assert!(ddl.contains("b TEXT NOT NULL"));
        assert!(ddl.contains("c INTEGER NOT NULL"));
        assert!(ddl.contains("d BIGINT NOT NULL"));
        assert!(ddl.contains("e BOOLEAN NOT NULL"));
        assert!(ddl.contains("f DOUBLE PRECISION NOT NULL"));
        assert!(ddl.contains("g JSONB"));
        assert!(ddl.contains("h TIMESTAMPTZ NOT NULL DEFAULT now()"));
    }

    #[test]
    fn custom_pool_field() {
        let input = MigrateTableInput {
            table: "simple".into(),
            columns: vec![uuid_pk("id")],
            composite_pk: None,
            unique_constraints: vec![],
            indexes: vec![],
            pool_field: "self.db".into(),
            indent: 3,
        };

        let code = gen_migrate_table(&input);
        assert!(code.contains(".execute(&self.db)"));
    }
}
