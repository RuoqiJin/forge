//! Domain Types IR — typed representation of `(domain-types ...)`.
//!
//! Lowering: S-EXP → DomainTypesDecl → Rust code (struct defs + enum defs).

use nc_parser::SexpNode;

// ---------------------------------------------------------------------------
// Declaration types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct FieldDecl {
    pub name: String,
    pub rust_type: String,
    pub doc: Option<String>,
    pub optional: bool,
    pub default: Option<String>,
    pub skip_none: bool,
    pub serde_rename: Option<String>,
}

#[derive(Debug, Clone)]
pub struct StructDecl {
    pub name: String,
    pub doc: Option<String>,
    pub fields: Vec<FieldDecl>,
    pub serde_rename_all: Option<String>,
    pub derives: Vec<String>,
    pub default_impl: bool,
}

#[derive(Debug, Clone)]
pub struct VariantDecl {
    pub name: String,
    pub str_value: String,
    pub doc: Option<String>,
}

#[derive(Debug, Clone)]
pub struct EnumDecl {
    pub name: String,
    pub doc: Option<String>,
    pub variants: Vec<VariantDecl>,
    pub serde_rename_all: Option<String>,
    pub gen_as_str: bool,
    pub gen_from_str: bool,
    pub from_str_default: Option<String>,
    pub copy: bool,
}

#[derive(Debug, Clone)]
pub struct DomainTypesDecl {
    pub target: String,
    pub structs: Vec<StructDecl>,
    pub enums: Vec<EnumDecl>,
}

// ---------------------------------------------------------------------------
// Error
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct IrError(pub String);

impl std::fmt::Display for IrError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "domain-types IR error: {}", self.0)
    }
}

impl std::error::Error for IrError {}

// ---------------------------------------------------------------------------
// S-EXP → IR lowering
// ---------------------------------------------------------------------------

impl DomainTypesDecl {
    pub fn from_sexp(node: &SexpNode) -> Result<Self, IrError> {
        let children = match node {
            SexpNode::List(c) => c,
            _ => return Err(IrError("expected list".into())),
        };
        if children.len() < 2 {
            return Err(IrError("missing domain-types tag".into()));
        }
        match &children[0] {
            SexpNode::Atom(s) if s == "domain-types" => {}
            _ => return Err(IrError("expected (domain-types ...)".into())),
        }

        let target = node
            .keyword(":target")
            .ok_or_else(|| IrError("missing :target".into()))?
            .to_string();

        let mut structs = Vec::new();
        let mut enums = Vec::new();

        for child in children.iter().skip(1) {
            match child.tag() {
                Some("struct") => structs.push(parse_struct(child)?),
                Some("enum") => enums.push(parse_enum(child)?),
                _ => {}
            }
        }

        Ok(DomainTypesDecl { target, structs, enums })
    }
}

fn parse_struct(node: &SexpNode) -> Result<StructDecl, IrError> {
    let children = match node {
        SexpNode::List(c) => c,
        _ => return Err(IrError("struct: expected list".into())),
    };
    let name = match children.get(1) {
        Some(SexpNode::Atom(s)) => s.clone(),
        _ => return Err(IrError("struct: missing name".into())),
    };

    let serde_rename_all = node.keyword(":serde-rename").map(String::from);
    let default_impl = node.keyword(":default").map(|v| v == "true").unwrap_or(false);
    let doc = node.keyword(":doc").map(String::from);

    let derives: Vec<String> = node
        .keyword_node(":derives")
        .map(|n| match n {
            SexpNode::List(cs) => cs
                .iter()
                .filter_map(|c| match c {
                    SexpNode::Atom(s) => Some(s.clone()),
                    _ => None,
                })
                .collect(),
            _ => vec![],
        })
        .unwrap_or_default();

    let mut fields = Vec::new();
    for child in children.iter().skip(2) {
        if child.tag() == Some("field") {
            fields.push(parse_field(child)?);
        }
    }

    Ok(StructDecl { name, doc, fields, serde_rename_all, derives, default_impl })
}

fn parse_field(node: &SexpNode) -> Result<FieldDecl, IrError> {
    let children = match node {
        SexpNode::List(c) => c,
        _ => return Err(IrError("field: expected list".into())),
    };
    let name = match children.get(1) {
        Some(SexpNode::Atom(s)) => s.clone(),
        _ => return Err(IrError("field: missing name".into())),
    };
    let rust_type = match children.get(2) {
        Some(SexpNode::Atom(s)) => s.clone(),
        _ => return Err(IrError(format!("field {name}: missing type"))),
    };

    let optional = node.keyword(":optional").map(|v| v == "true").unwrap_or(false)
        || rust_type.starts_with("Option<");
    let default = node.keyword(":default").map(String::from);
    let skip_none = node.keyword(":skip-none").map(|v| v == "true").unwrap_or(optional);
    let serde_rename = node.keyword(":rename").map(String::from);
    let doc = node.keyword(":doc").map(String::from);

    Ok(FieldDecl { name, rust_type, doc, optional, default, skip_none, serde_rename })
}

fn parse_enum(node: &SexpNode) -> Result<EnumDecl, IrError> {
    let children = match node {
        SexpNode::List(c) => c,
        _ => return Err(IrError("enum: expected list".into())),
    };
    let name = match children.get(1) {
        Some(SexpNode::Atom(s)) => s.clone(),
        _ => return Err(IrError("enum: missing name".into())),
    };

    let serde_rename_all = node.keyword(":serde-rename").map(String::from);
    let gen_as_str = node.keyword(":as-str").map(|v| v != "false").unwrap_or(true);
    let gen_from_str = node.keyword(":from-str").map(|v| v != "false").unwrap_or(true);
    let from_str_default = node.keyword(":from-str-default").map(String::from);
    let copy = node.keyword(":copy").map(|v| v != "false").unwrap_or(true);
    let doc = node.keyword(":doc").map(String::from);

    let mut variants = Vec::new();
    for child in children.iter().skip(2) {
        if child.tag() == Some("variant") {
            variants.push(parse_variant(child)?);
        }
    }

    Ok(EnumDecl { name, doc, variants, serde_rename_all, gen_as_str, gen_from_str, from_str_default, copy })
}

fn parse_variant(node: &SexpNode) -> Result<VariantDecl, IrError> {
    let children = match node {
        SexpNode::List(c) => c,
        _ => return Err(IrError("variant: expected list".into())),
    };
    let name = match children.get(1) {
        Some(SexpNode::Atom(s)) => s.clone(),
        _ => return Err(IrError("variant: missing name".into())),
    };
    let str_value = match children.get(2) {
        Some(SexpNode::Atom(s)) => s.clone(),
        _ => name.to_lowercase(),
    };
    let doc = node.keyword(":doc").map(String::from);

    Ok(VariantDecl { name, str_value, doc })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(src: &str) -> SexpNode {
        SexpNode::parse_one(src).unwrap()
    }

    #[test]
    fn parse_simple_struct() {
        let decl = DomainTypesDecl::from_sexp(&parse(
            r#"(domain-types :target "gen.rs"
                (struct Foo
                  (field id String)
                  (field count i32 :default "0")))"#,
        ))
        .unwrap();
        assert_eq!(decl.structs.len(), 1);
        assert_eq!(decl.structs[0].name, "Foo");
        assert_eq!(decl.structs[0].fields.len(), 2);
        assert_eq!(decl.structs[0].fields[1].default, Some("0".into()));
    }

    #[test]
    fn parse_enum_with_variants() {
        let decl = DomainTypesDecl::from_sexp(&parse(
            r#"(domain-types :target "gen.rs"
                (enum Status :serde-rename "lowercase"
                  (variant Open "open")
                  (variant Done "done")))"#,
        ))
        .unwrap();
        assert_eq!(decl.enums.len(), 1);
        assert_eq!(decl.enums[0].name, "Status");
        assert_eq!(decl.enums[0].variants.len(), 2);
        assert_eq!(decl.enums[0].variants[0].str_value, "open");
    }

    #[test]
    fn parse_mixed() {
        let decl = DomainTypesDecl::from_sexp(&parse(
            r#"(domain-types :target "gen.rs"
                (enum Color (variant Red "red") (variant Blue "blue"))
                (struct Point (field x f64) (field y f64))
                (enum Size (variant Small "small") (variant Large "large")))"#,
        ))
        .unwrap();
        assert_eq!(decl.enums.len(), 2);
        assert_eq!(decl.structs.len(), 1);
    }
}
