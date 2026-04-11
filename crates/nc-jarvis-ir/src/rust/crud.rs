//! CRUD Gateway IR — typed representation of `(pattern crud-gateway ...)`.
//!
//! Lowering: S-EXP → CrudGatewayDecl → Rust code (via nc-jarvis-gen).
//! All fields are validated on parse — invalid column types, unknown op kinds,
//! and malformed references are rejected with structured IrError.

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
            "CRUD IR error in `{}`: {} (expected {:?}, got {:?})",
            self.field, self.message, self.expected, self.got
        )
    }
}

impl std::error::Error for IrError {}

// ---------------------------------------------------------------------------
// Column types — the whitelist
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum ColumnType {
    Uuid,
    Text,
    Integer,
    Bigint,
    Boolean,
    Float8,
    Jsonb,
    Timestamptz,
}

impl ColumnType {
    pub fn from_str(s: &str) -> Result<Self, IrError> {
        let lower = s.to_lowercase();
        let base = lower.as_str();
        match base {
            "uuid" => Ok(Self::Uuid),
            "text" => Ok(Self::Text),
            "integer" | "int" | "serial" => Ok(Self::Integer),
            "bigint" | "bigserial" => Ok(Self::Bigint),
            "boolean" | "bool" => Ok(Self::Boolean),
            "float8" | "double" => Ok(Self::Float8),
            "jsonb" | "json" => Ok(Self::Jsonb),
            "timestamptz" | "timestamp" => Ok(Self::Timestamptz),
            // Extended: SQL types from deep survey intent files
            _ if base.starts_with("varchar") || base.starts_with("char(") => Ok(Self::Text),
            _ if base.starts_with("decimal") || base.starts_with("numeric") => Ok(Self::Float8),
            "bytea" => Ok(Self::Text),
            _ if base.ends_with("[]") => Ok(Self::Jsonb), // array types → JSONB
            other => Err(IrError {
                field: "column type".into(),
                message: "unknown column type".into(),
                expected: vec![
                    "uuid", "text", "integer", "bigint", "boolean", "float8", "jsonb",
                    "timestamptz", "varchar(...)", "bigserial", "serial", "bytea",
                ]
                .into_iter()
                .map(String::from)
                .collect(),
                got: other.into(),
            }),
        }
    }

    /// SQL type name for DDL generation.
    pub fn sql_type(&self) -> &'static str {
        match self {
            Self::Uuid => "UUID",
            Self::Text => "TEXT",
            Self::Integer => "INTEGER",
            Self::Bigint => "BIGINT",
            Self::Boolean => "BOOLEAN",
            Self::Float8 => "DOUBLE PRECISION",
            Self::Jsonb => "JSONB",
            Self::Timestamptz => "TIMESTAMPTZ",
        }
    }
}

// ---------------------------------------------------------------------------
// CRUD operation kinds — the whitelist
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum CrudOpKind {
    /// INSERT, auto-gen id/ts, return constructed struct.
    Insert,
    /// INSERT from &StructRef, all fields bound.
    InsertStruct,
    /// SELECT WHERE pk=$1, return Option<T>.
    SelectOne,
    /// SELECT with WHERE/ORDER/LIMIT, return Vec<T>.
    SelectMany,
    /// SELECT to_jsonb(t), return Vec<Value> or Option<Value>.
    SelectJson,
    /// UPDATE SET explicit columns WHERE pk.
    Update,
    /// Build SET clause from JSON keys at runtime.
    UpdateDynamic,
    /// Compare-and-swap: UPDATE with preconditions, return bool.
    UpdateCas,
    /// DELETE WHERE pk=$1.
    Delete,
    /// INSERT ON CONFLICT DO UPDATE/NOTHING.
    Upsert,
    /// SELECT COUNT(*) WHERE ...
    Count,
    /// DELETE old + INSERT N new (transactional batch).
    SyncBatch,
    /// Raw SQL escape hatch.
    Custom,
}

impl CrudOpKind {
    pub fn from_str(s: &str) -> Result<Self, IrError> {
        match s {
            "insert" => Ok(Self::Insert),
            "insert-struct" => Ok(Self::InsertStruct),
            "select-one" => Ok(Self::SelectOne),
            "select-many" => Ok(Self::SelectMany),
            "select-json" => Ok(Self::SelectJson),
            "update" => Ok(Self::Update),
            "update-dynamic" => Ok(Self::UpdateDynamic),
            "update-cas" => Ok(Self::UpdateCas),
            "delete" => Ok(Self::Delete),
            "upsert" => Ok(Self::Upsert),
            "count" => Ok(Self::Count),
            "sync-batch" => Ok(Self::SyncBatch),
            "custom" => Ok(Self::Custom),
            other => Err(IrError {
                field: ":kind".into(),
                message: "unknown CRUD op kind".into(),
                expected: vec![
                    "insert",
                    "insert-struct",
                    "select-one",
                    "select-many",
                    "select-json",
                    "update",
                    "update-dynamic",
                    "update-cas",
                    "delete",
                    "upsert",
                    "count",
                    "sync-batch",
                    "custom",
                ]
                .into_iter()
                .map(String::from)
                .collect(),
                got: other.into(),
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// Declaration types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ColumnDecl {
    pub name: String,
    pub col_type: ColumnType,
    pub primary_key: bool,
    pub nullable: bool,
    pub default: Option<String>,
    pub default_now: bool,
    pub foreign_key: Option<String>,
    pub on_delete: Option<String>,
}

#[derive(Debug, Clone)]
pub struct IndexDecl {
    pub name: String,
    pub columns: Vec<String>,
    pub unique: bool,
}

#[derive(Debug, Clone)]
pub struct UniqueConstraint {
    pub columns: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct CompositePk {
    pub columns: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct EnumMapDecl {
    pub field: String,
    pub converter: String,
    pub target_type: String,
}

#[derive(Debug, Clone)]
pub struct RowTypeDecl {
    pub name: String,
    pub derives: Vec<String>,
    pub convert_fn: String,
    pub target_type: String,
    pub enum_maps: Vec<EnumMapDecl>,
}

#[derive(Debug, Clone)]
pub struct ParamDecl {
    pub name: String,
    pub rust_type: String,
    pub cast_to: Option<String>,
    pub ilike_wrap: bool,
    pub prefix_like: bool,
    pub bind_name: Option<String>,
    pub op: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AutoFieldDecl {
    pub name: String,
    pub via_expr: Option<String>,
    pub via_query: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SetFieldDecl {
    pub column: String,
    pub rust_type: Option<String>,
    pub param_name: Option<String>,
    pub via_fn: Option<String>,
    pub literal: Option<String>,
    pub converter: Option<String>,
    pub conditional: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CrudOpDecl {
    pub name: String,
    pub kind: CrudOpKind,
    pub params: Vec<ParamDecl>,
    pub where_clause: Vec<ParamDecl>,
    pub where_fixed: Vec<String>,
    pub where_optional: Vec<ParamDecl>,
    pub where_ilike: Vec<String>,
    pub set_fields: Vec<SetFieldDecl>,
    pub auto_fields: Vec<AutoFieldDecl>,
    pub defaults: Vec<(String, String)>,
    pub select_cols: Vec<String>,
    pub order: Option<String>,
    pub on_conflict: Option<String>,
    pub preconditions: Vec<String>,
    pub dynamic_fields: Vec<String>,
    pub auto_set: Vec<(String, String)>,
    pub side_effect: Option<String>,
    pub post_process: Option<String>,
    pub returns: String,
    pub custom_sql: Option<String>,
    pub custom_queries: Vec<(String, String)>,
    pub delete_where: Option<String>,
    pub insert_each: Option<String>,
    pub logic: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct TableDecl {
    pub name: String,
    pub columns: Vec<ColumnDecl>,
    pub indexes: Vec<IndexDecl>,
    pub unique_constraints: Vec<UniqueConstraint>,
    pub composite_pk: Option<CompositePk>,
    pub row_type: Option<RowTypeDecl>,
    pub ops: Vec<CrudOpDecl>,
}

#[derive(Debug, Clone)]
pub struct HelperFnDecl {
    pub name: String,
    pub param_type: String,
    pub return_type: String,
    pub mappings: Vec<(String, String)>,
    pub default: Option<String>,
}

/// Top-level `(pattern crud-gateway ...)` or `(crud-gateway name ...)` declaration.
#[derive(Debug, Clone)]
pub struct CrudGatewayDecl {
    pub gateway_struct: String,
    pub pool_type: String,
    pub backend: String,
    pub target: String,
    pub tables: Vec<TableDecl>,
    pub helpers: Vec<HelperFnDecl>,
    /// Identity name from `(crud-gateway auth-identity ...)` form.
    pub identity: Option<String>,
}

// ---------------------------------------------------------------------------
// S-EXP → IR lowering
// ---------------------------------------------------------------------------

impl CrudGatewayDecl {
    /// Lower `(pattern crud-gateway ...)` or `(crud-gateway name ...)` into IR.
    pub fn from_sexp(node: &SexpNode) -> Result<Self, IrError> {
        let children = expect_list(node)?;
        let tag = node
            .tag()
            .ok_or_else(|| err("crud-gateway", "missing tag", &["pattern", "crud-gateway"], ""))?;

        // Determine form and skip index:
        // (pattern crud-gateway :gateway ... (table ...) ...) → skip 2
        // (crud-gateway name (table ...) ...)                 → skip 2 (name is identity)
        // (crud-gateway :gateway ... (table ...) ...)         → skip 1 (no identity)
        let (skip, identity) = if tag == "pattern" {
            if children.len() < 2 {
                return Err(err("pattern", "missing pattern name", &["crud-gateway"], ""));
            }
            let pattern_name = expect_atom(&children[1])?;
            if pattern_name != "crud-gateway" {
                return Err(err(
                    "pattern",
                    "wrong pattern name",
                    &["crud-gateway"],
                    pattern_name,
                ));
            }
            (2, None)
        } else if tag == "crud-gateway" {
            // Check if second element is an identity name (non-keyword atom)
            if children.len() >= 2 {
                if let SexpNode::Atom(s) = &children[1] {
                    if !s.starts_with(':') {
                        (2, Some(s.clone()))
                    } else {
                        (1, None)
                    }
                } else {
                    (1, None)
                }
            } else {
                (1, None)
            }
        } else {
            return Err(err(
                "crud-gateway",
                "expected 'pattern' or 'crud-gateway' tag",
                &["pattern", "crud-gateway"],
                tag,
            ));
        };

        // Keywords with defaults — optional for (crud-gateway name ...) form
        let gateway_struct = node
            .keyword(":gateway")
            .unwrap_or("Storage")
            .to_string();
        let pool_type = node
            .keyword(":pool")
            .unwrap_or("PgPool")
            .to_string();
        let backend = node.keyword(":backend").unwrap_or("postgres").to_string();
        let target = node
            .keyword(":target")
            .map(String::from)
            .unwrap_or_else(|| {
                // Derive from identity: auth-identity → src/storage/auth_identity_generated.rs
                let name = identity
                    .as_deref()
                    .unwrap_or("storage")
                    .replace('-', "_");
                format!("src/storage/{name}_generated.rs")
            });

        let mut tables = Vec::new();
        let mut helpers = Vec::new();

        for child in children.iter().skip(skip) {
            match child.tag() {
                Some("table") => tables.push(parse_table(child)?),
                Some("helpers") => helpers = parse_helpers(child)?,
                _ => {} // skip keywords, comments, and unrecognized forms
            }
        }

        Ok(CrudGatewayDecl {
            gateway_struct,
            pool_type,
            backend,
            target,
            tables,
            helpers,
            identity,
        })
    }
}

// ---------------------------------------------------------------------------
// Table parsing
// ---------------------------------------------------------------------------

fn parse_table(node: &SexpNode) -> Result<TableDecl, IrError> {
    let children = expect_list(node)?;
    let name = if children.len() >= 2 {
        expect_atom(&children[1])?.to_string()
    } else {
        return Err(err("table", "missing table name", &["table_name"], ""));
    };

    let mut columns = Vec::new();
    let mut indexes = Vec::new();
    let mut unique_constraints = Vec::new();
    let mut composite_pk = None;
    let mut row_type = None;
    let mut ops = Vec::new();

    for child in children.iter().skip(2) {
        match child.tag() {
            Some("col") => columns.push(parse_column(child)?),
            Some("idx") => indexes.push(parse_index(child)?),
            Some("unique") => unique_constraints.push(parse_unique(child)?),
            Some("pk") => composite_pk = Some(parse_composite_pk(child)?),
            Some("row-type") => row_type = Some(parse_row_type(child)?),
            Some("op") => ops.push(parse_crud_op(child)?),
            _ => {}
        }
    }

    // Deep survey format: top-level keywords :unique, :index, :pk as keyword args
    // e.g., (table users ... :unique (tenant_id slug) :index (tenant_id parent_id) :pk (user_id client_id))
    if let Some(pk_node) = node.keyword_node(":pk") {
        if composite_pk.is_none() {
            let cols = collect_atoms(pk_node);
            if !cols.is_empty() {
                composite_pk = Some(CompositePk { columns: cols });
            }
        }
    }
    // Collect all :unique keyword args
    for child in children.iter().skip(2) {
        if let SexpNode::Atom(s) = child {
            if s == ":unique" {
                // Next sibling should be a list of column names
                // Already handled by the (unique ...) child form above
            }
        }
    }
    // :index keyword with inline column list
    if let Some(idx_node) = node.keyword_node(":index") {
        let cols = collect_atoms(idx_node);
        if !cols.is_empty() {
            indexes.push(IndexDecl {
                name: format!("idx_{}_{}", name, cols.join("_")),
                columns: cols,
                unique: false,
            });
        }
    }

    Ok(TableDecl {
        name,
        columns,
        indexes,
        unique_constraints,
        composite_pk,
        row_type,
        ops,
    })
}

fn parse_column(node: &SexpNode) -> Result<ColumnDecl, IrError> {
    let children = expect_list(node)?;
    let name = if children.len() >= 2 {
        expect_atom(&children[1])?.to_string()
    } else {
        return Err(err("col", "missing column name", &["col_name"], ""));
    };

    let type_str = keyword_required(node, ":type", "uuid|text|integer|...")?;
    let col_type = ColumnType::from_str(&type_str)?;

    Ok(ColumnDecl {
        name,
        col_type,
        primary_key: has_keyword(node, ":pk"),
        nullable: has_keyword(node, ":nullable"),
        default: node.keyword(":default").map(String::from),
        default_now: has_keyword(node, ":default-now"),
        foreign_key: node.keyword(":fk").map(String::from),
        on_delete: node.keyword(":on-delete").map(String::from),
    })
}

fn parse_index(node: &SexpNode) -> Result<IndexDecl, IrError> {
    let children = expect_list(node)?;
    let name = if children.len() >= 2 {
        expect_atom(&children[1])?.to_string()
    } else {
        return Err(err("idx", "missing index name", &["idx_name"], ""));
    };

    let cols_node = node.keyword_node(":cols").ok_or_else(|| {
        err("idx", "missing :cols", &["(col1 col2)"], "")
    })?;
    let columns = collect_atoms(cols_node);
    let unique = has_keyword(node, ":unique");

    Ok(IndexDecl {
        name,
        columns,
        unique,
    })
}

fn parse_unique(node: &SexpNode) -> Result<UniqueConstraint, IrError> {
    let children = expect_list(node)?;
    // (unique (col1 col2)) — columns are in a nested list
    for child in children.iter().skip(1) {
        if let SexpNode::List(inner) = child {
            let columns: Vec<String> = inner
                .iter()
                .filter_map(|c| {
                    if let SexpNode::Atom(s) = c {
                        Some(s.clone())
                    } else {
                        None
                    }
                })
                .collect();
            if !columns.is_empty() {
                return Ok(UniqueConstraint { columns });
            }
        }
    }
    // Fallback: atoms directly under unique
    let columns: Vec<String> = children
        .iter()
        .skip(1)
        .filter_map(|c| {
            if let SexpNode::Atom(s) = c {
                if !s.starts_with(':') {
                    return Some(s.clone());
                }
            }
            None
        })
        .collect();
    Ok(UniqueConstraint { columns })
}

fn parse_composite_pk(node: &SexpNode) -> Result<CompositePk, IrError> {
    let children = expect_list(node)?;
    let columns: Vec<String> = children
        .iter()
        .skip(1)
        .filter_map(|c| {
            if let SexpNode::Atom(s) = c {
                if !s.starts_with(':') {
                    return Some(s.clone());
                }
            }
            None
        })
        .collect();
    Ok(CompositePk { columns })
}

fn parse_row_type(node: &SexpNode) -> Result<RowTypeDecl, IrError> {
    let children = expect_list(node)?;
    let name = if children.len() >= 2 {
        expect_atom(&children[1])?.to_string()
    } else {
        return Err(err("row-type", "missing name", &["RowTypeName"], ""));
    };

    let derives = match node.keyword_node(":derive") {
        Some(n) => collect_atoms(n),
        None => vec![],
    };

    let convert_fn = keyword_required(node, ":convert", "into_xxx")?;
    let target_type = keyword_required(node, ":to", "TargetType")?;

    let mut enum_maps = Vec::new();
    if let Some(maps_node) = node.keyword_node(":enum-map") {
        if let SexpNode::List(maps) = maps_node {
            for map_entry in maps {
                if let SexpNode::List(parts) = map_entry {
                    if parts.len() >= 3 {
                        enum_maps.push(EnumMapDecl {
                            field: expect_atom(&parts[0])?.to_string(),
                            converter: expect_atom(&parts[1])?.to_string(),
                            target_type: expect_atom(&parts[2])?.to_string(),
                        });
                    }
                }
            }
        }
    }

    Ok(RowTypeDecl {
        name,
        derives,
        convert_fn,
        target_type,
        enum_maps,
    })
}

// ---------------------------------------------------------------------------
// CRUD op parsing
// ---------------------------------------------------------------------------

fn parse_crud_op(node: &SexpNode) -> Result<CrudOpDecl, IrError> {
    let children = expect_list(node)?;
    let name = if children.len() >= 2 {
        expect_atom(&children[1])?.to_string()
    } else {
        return Err(err("op", "missing op name", &["op_name"], ""));
    };

    let kind_str = keyword_required(node, ":kind", "insert|select-one|...")?;
    let kind = CrudOpKind::from_str(&kind_str)?;

    // Support both :params ((a T) (b T)) and :param (single T)
    let mut params = parse_param_list(node, ":params");
    if params.is_empty() {
        // Try singular :param (name type) — used by insert-struct
        let singular = parse_param_list(node, ":param");
        if !singular.is_empty() {
            params = singular;
        }
    }
    let where_clause = parse_param_list(node, ":where");
    let where_optional = parse_param_list(node, ":where-optional");
    let where_fixed = parse_string_list(node, ":where-fixed");
    let where_ilike = parse_string_list(node, ":where-ilike");
    let set_fields = parse_set_fields(node);
    let auto_fields = parse_auto_fields(node);
    let defaults = parse_kv_list(node, ":defaults");
    let select_cols = parse_string_list(node, ":select");
    let order = node.keyword(":order").map(String::from);
    let on_conflict = node.keyword(":on-conflict").map(String::from);
    let preconditions = parse_string_list(node, ":preconditions");
    let dynamic_fields = parse_string_list(node, ":dynamic-fields");
    let auto_set = parse_kv_list(node, ":auto-set");
    let side_effect = node.keyword(":side-effect").map(String::from);
    let post_process = node.keyword(":post-process").map(String::from);
    let returns = keyword_required(node, ":returns", "ReturnType")?;
    let custom_sql = node.keyword(":sql").map(String::from);
    let custom_queries = parse_kv_list(node, ":queries");
    let delete_where = node.keyword(":delete-where").map(String::from);
    let insert_each = node.keyword(":insert-each").map(String::from);
    let logic = node.keyword(":logic").map(String::from);
    let limit = node.keyword(":limit").and_then(|s| s.parse().ok());

    // Validate: insert-struct requires :param
    if kind == CrudOpKind::InsertStruct {
        let param = node.keyword(":param");
        if param.is_none() && params.is_empty() {
            return Err(err(
                &name,
                "insert-struct requires :param",
                &["(param \"&Type\")"],
                "",
            ));
        }
    }

    Ok(CrudOpDecl {
        name,
        kind,
        params,
        where_clause,
        where_fixed,
        where_optional,
        where_ilike,
        set_fields,
        auto_fields,
        defaults,
        select_cols,
        order,
        on_conflict,
        preconditions,
        dynamic_fields,
        auto_set,
        side_effect,
        post_process,
        returns,
        custom_sql,
        custom_queries,
        delete_where,
        insert_each,
        logic,
        limit,
    })
}

// ---------------------------------------------------------------------------
// Param list parsing: :params ((name type :modifier) ...)
// ---------------------------------------------------------------------------

fn parse_param_list(parent: &SexpNode, keyword: &str) -> Vec<ParamDecl> {
    let node = match parent.keyword_node(keyword) {
        Some(n) => n,
        None => return vec![],
    };

    let items = match node {
        SexpNode::List(items) => items,
        _ => return vec![],
    };

    // Detect single-param shorthand: :param (name type) where items are all atoms
    // vs multi-param: :params ((name type) (name type)) where items contain lists
    let has_inner_lists = items.iter().any(|i| matches!(i, SexpNode::List(_)));

    if !has_inner_lists && items.len() >= 2 {
        // Single param tuple: (name type [:modifier ...])
        let name = match expect_atom(&items[0]) {
            Ok(s) => s.to_string(),
            Err(_) => return vec![],
        };
        let rust_type = match expect_atom(&items[1]) {
            Ok(s) => s.to_string(),
            Err(_) => return vec![],
        };
        return vec![ParamDecl {
            name,
            rust_type,
            cast_to: node.keyword(":cast-to").map(String::from),
            ilike_wrap: has_keyword(node, ":ilike-wrap"),
            prefix_like: has_keyword(node, ":prefix-like"),
            bind_name: node.keyword(":bind-name").map(String::from),
            op: node.keyword(":op").map(String::from),
        }];
    }

    // Multi-param: ((name type) (name type) ...)
    items
        .iter()
        .filter_map(|item| match item {
            SexpNode::List(parts) if parts.len() >= 2 => {
                let name = expect_atom(&parts[0]).ok()?.to_string();
                let rust_type = expect_atom(&parts[1]).ok()?.to_string();
                let cast_to = item.keyword(":cast-to").map(String::from);
                let ilike_wrap = has_keyword(item, ":ilike-wrap");
                let prefix_like = has_keyword(item, ":prefix-like");
                let bind_name = item.keyword(":bind-name").map(String::from);
                let op = item.keyword(":op").map(String::from);
                Some(ParamDecl {
                    name,
                    rust_type,
                    cast_to,
                    ilike_wrap,
                    prefix_like,
                    bind_name,
                    op,
                })
            }
            _ => None,
        })
        .collect()
}

fn parse_set_fields(parent: &SexpNode) -> Vec<SetFieldDecl> {
    let node = match parent.keyword_node(":set") {
        Some(n) => n,
        None => return vec![],
    };

    let items = match node {
        SexpNode::List(items) => items,
        _ => return vec![],
    };

    items
        .iter()
        .filter_map(|item| match item {
            SexpNode::List(parts) if !parts.is_empty() => {
                let column = expect_atom(&parts[0]).ok()?.to_string();
                let rust_type = if parts.len() >= 2 {
                    let s = expect_atom(&parts[1]).ok()?;
                    if s.starts_with(':') {
                        None
                    } else {
                        Some(s.to_string())
                    }
                } else {
                    None
                };
                let param_name = item.keyword(":param").map(String::from);
                let via_fn = item.keyword(":via").map(String::from);
                let literal = item.keyword(":literal").map(String::from);
                let converter = item.keyword(":via").map(String::from);
                let conditional = item.keyword(":conditional").map(String::from);
                Some(SetFieldDecl {
                    column,
                    rust_type,
                    param_name,
                    via_fn,
                    literal,
                    converter,
                    conditional,
                })
            }
            _ => None,
        })
        .collect()
}

fn parse_auto_fields(parent: &SexpNode) -> Vec<AutoFieldDecl> {
    let children = match parent {
        SexpNode::List(c) => c,
        _ => return vec![],
    };

    // Collect :auto keyword-nodes from parent
    // intent.lisp uses multiple `:auto` keywords, nc-parser may only return last one
    // Scan children for (auto ...) or keyword pairs
    let mut autos = Vec::new();
    let mut i = 0;
    while i < children.len() {
        if let SexpNode::Atom(s) = &children[i] {
            if s == ":auto" && i + 1 < children.len() {
                if let SexpNode::List(parts) = &children[i + 1] {
                    if !parts.is_empty() {
                        let name = expect_atom(&parts[0]).unwrap_or("").to_string();
                        let via_expr =
                            children[i + 1].keyword(":via").map(String::from);
                        let via_query =
                            children[i + 1].keyword(":via-query").map(String::from);
                        autos.push(AutoFieldDecl {
                            name,
                            via_expr,
                            via_query,
                        });
                    }
                }
                i += 2;
                continue;
            }
        }
        i += 1;
    }
    autos
}

fn parse_string_list(parent: &SexpNode, keyword: &str) -> Vec<String> {
    match parent.keyword_node(keyword) {
        Some(SexpNode::List(items)) => items
            .iter()
            .filter_map(|i| {
                if let SexpNode::Atom(s) = i {
                    Some(s.clone())
                } else {
                    None
                }
            })
            .collect(),
        Some(SexpNode::Atom(s)) => vec![s.clone()],
        None => vec![],
    }
}

fn parse_kv_list(parent: &SexpNode, keyword: &str) -> Vec<(String, String)> {
    match parent.keyword_node(keyword) {
        Some(SexpNode::List(items)) => items
            .iter()
            .filter_map(|item| {
                if let SexpNode::List(parts) = item {
                    if parts.len() >= 2 {
                        let k = expect_atom(&parts[0]).ok()?;
                        let v = expect_atom(&parts[1]).ok()?;
                        return Some((k.to_string(), v.to_string()));
                    }
                }
                None
            })
            .collect(),
        _ => vec![],
    }
}

// ---------------------------------------------------------------------------
// Helpers parsing
// ---------------------------------------------------------------------------

fn parse_helpers(node: &SexpNode) -> Result<Vec<HelperFnDecl>, IrError> {
    let children = expect_list(node)?;
    let mut helpers = Vec::new();

    for child in children.iter().skip(1) {
        if child.tag() == Some("fn") {
            helpers.push(parse_helper_fn(child)?);
        }
    }
    Ok(helpers)
}

fn parse_helper_fn(node: &SexpNode) -> Result<HelperFnDecl, IrError> {
    let children = expect_list(node)?;
    let name = if children.len() >= 2 {
        expect_atom(&children[1])?.to_string()
    } else {
        return Err(err("fn", "missing function name", &["fn_name"], ""));
    };

    // :param can be atom "Type" or list (Type) — handle both
    let param_type = match node.keyword(":param") {
        Some(s) => s.to_string(),
        None => match node.keyword_node(":param") {
            Some(n) => collect_atoms(n).join(", "),
            None => {
                return Err(err(":param", "missing required keyword :param", &["(Type)"], ""));
            }
        },
    };
    let return_type = keyword_required(node, ":returns", "ReturnType")?;
    let default = node.keyword(":default").map(String::from);

    let mappings = match node.keyword_node(":map") {
        Some(SexpNode::List(items)) => items
            .iter()
            .filter_map(|item| {
                if let SexpNode::List(parts) = item {
                    if parts.len() >= 2 {
                        let from = expect_atom(&parts[0]).ok()?;
                        let to = expect_atom(&parts[1]).ok()?;
                        return Some((from.to_string(), to.to_string()));
                    }
                }
                None
            })
            .collect(),
        _ => vec![],
    };

    Ok(HelperFnDecl {
        name,
        param_type,
        return_type,
        mappings,
        default,
    })
}

// ---------------------------------------------------------------------------
// S-EXP utility helpers
// ---------------------------------------------------------------------------

fn expect_tag<'a>(node: &'a SexpNode, expected: &str) -> Result<&'a str, IrError> {
    let tag = node.tag().ok_or_else(|| err("node", "expected a list", &[expected], "atom"))?;
    if tag != expected {
        return Err(err("tag", "wrong tag", &[expected], tag));
    }
    Ok(tag)
}

fn expect_list(node: &SexpNode) -> Result<&Vec<SexpNode>, IrError> {
    match node {
        SexpNode::List(children) => Ok(children),
        SexpNode::Atom(s) => Err(err("node", "expected list, got atom", &["(...)"], s)),
    }
}

fn expect_atom<'a>(node: &'a SexpNode) -> Result<&'a str, IrError> {
    match node {
        SexpNode::Atom(s) => Ok(s),
        SexpNode::List(_) => Err(err("node", "expected atom, got list", &["atom"], "(…)")),
    }
}

fn keyword_required(node: &SexpNode, key: &str, hint: &str) -> Result<String, IrError> {
    node.keyword(key)
        .map(String::from)
        .ok_or_else(|| err(key, &format!("missing required keyword {key}"), &[hint], ""))
}

fn has_keyword(node: &SexpNode, key: &str) -> bool {
    if let SexpNode::List(children) = node {
        children.iter().any(|c| matches!(c, SexpNode::Atom(s) if s == key))
    } else {
        false
    }
}

fn collect_atoms(node: &SexpNode) -> Vec<String> {
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
    fn reject_wrong_pattern_name() {
        let sexp = parse(r#"(pattern event-listener :gateway Foo :pool Bar :target "x")"#);
        let err = CrudGatewayDecl::from_sexp(&sexp).unwrap_err();
        assert_eq!(err.field, "pattern");
        assert!(err.message.contains("wrong pattern name"));
        assert!(err.expected.contains(&"crud-gateway".to_string()));
    }

    #[test]
    fn missing_gateway_defaults_to_storage() {
        let sexp = parse(r#"(pattern crud-gateway :pool PgPool :target "x.rs")"#);
        let decl = CrudGatewayDecl::from_sexp(&sexp).unwrap();
        assert_eq!(decl.gateway_struct, "Storage");
    }

    #[test]
    fn reject_invalid_column_type() {
        let sexp = parse(
            r#"(pattern crud-gateway :gateway S :pool P :target "t"
                (table t1
                  (col bad_col :type mongodb_objectid)))"#,
        );
        let err = CrudGatewayDecl::from_sexp(&sexp).unwrap_err();
        assert_eq!(err.field, "column type");
        assert!(err.got.contains("mongodb_objectid"));
    }

    #[test]
    fn parse_minimal_table() {
        let sexp = parse(
            r#"(pattern crud-gateway :gateway Storage :pool PgPool :target "storage.rs"
                (table sessions
                  (col id :type uuid :pk)
                  (col name :type text)
                  (op get_session
                    :kind select-one
                    :where ((id uuid))
                    :returns "Option<Session>")))"#,
        );
        let decl = CrudGatewayDecl::from_sexp(&sexp).unwrap();
        assert_eq!(decl.gateway_struct, "Storage");
        assert_eq!(decl.tables.len(), 1);

        let table = &decl.tables[0];
        assert_eq!(table.name, "sessions");
        assert_eq!(table.columns.len(), 2);
        assert!(table.columns[0].primary_key);
        assert_eq!(table.columns[0].col_type, ColumnType::Uuid);

        assert_eq!(table.ops.len(), 1);
        assert_eq!(table.ops[0].kind, CrudOpKind::SelectOne);
        assert_eq!(table.ops[0].returns, "Option<Session>");
    }

    #[test]
    fn parse_column_constraints() {
        let sexp = parse(
            r#"(pattern crud-gateway :gateway S :pool P :target "t"
                (table msgs
                  (col id :type uuid :pk)
                  (col session_id :type uuid :fk sessions)
                  (col content :type text :nullable)
                  (col created_at :type timestamptz :default-now)
                  (col status :type text :default "open")))"#,
        );
        let decl = CrudGatewayDecl::from_sexp(&sexp).unwrap();
        let cols = &decl.tables[0].columns;

        assert!(cols[0].primary_key);
        assert_eq!(cols[1].foreign_key, Some("sessions".into()));
        assert!(cols[2].nullable);
        assert!(cols[3].default_now);
        assert_eq!(cols[4].default, Some("open".into()));
    }

    #[test]
    fn parse_crud_op_kinds() {
        assert_eq!(CrudOpKind::from_str("insert").unwrap(), CrudOpKind::Insert);
        assert_eq!(CrudOpKind::from_str("select-json").unwrap(), CrudOpKind::SelectJson);
        assert_eq!(CrudOpKind::from_str("update-cas").unwrap(), CrudOpKind::UpdateCas);
        assert_eq!(CrudOpKind::from_str("sync-batch").unwrap(), CrudOpKind::SyncBatch);
        assert!(CrudOpKind::from_str("graphql-mutation").is_err());
    }

    #[test]
    fn reject_unknown_op_kind() {
        let sexp = parse(
            r#"(pattern crud-gateway :gateway S :pool P :target "t"
                (table t1
                  (col id :type uuid :pk)
                  (op bad_op :kind graphql-mutation :returns "()")))"#,
        );
        let err = CrudGatewayDecl::from_sexp(&sexp).unwrap_err();
        assert_eq!(err.field, ":kind");
        assert!(err.got.contains("graphql-mutation"));
    }

    #[test]
    fn parse_helpers() {
        let sexp = parse(
            r#"(pattern crud-gateway :gateway S :pool P :target "t"
                (helpers
                  (fn status_to_str :param (Status) :returns "&'static str"
                    :map ((Idle "idle") (Busy "busy"))
                    :default Idle)))"#,
        );
        let decl = CrudGatewayDecl::from_sexp(&sexp).unwrap();
        assert_eq!(decl.helpers.len(), 1);
        assert_eq!(decl.helpers[0].name, "status_to_str");
        assert_eq!(decl.helpers[0].mappings.len(), 2);
        assert_eq!(decl.helpers[0].default, Some("Idle".into()));
    }

    #[test]
    fn parse_index_and_unique() {
        let sexp = parse(
            r#"(pattern crud-gateway :gateway S :pool P :target "t"
                (table kb
                  (col id :type text :pk)
                  (col category :type text)
                  (col key :type text)
                  (unique (category key))
                  (idx idx_kb_cat :cols (category))))"#,
        );
        let decl = CrudGatewayDecl::from_sexp(&sexp).unwrap();
        let table = &decl.tables[0];
        assert_eq!(table.unique_constraints.len(), 1);
        assert_eq!(table.unique_constraints[0].columns, vec!["category", "key"]);
        assert_eq!(table.indexes.len(), 1);
        assert_eq!(table.indexes[0].name, "idx_kb_cat");
    }
}
