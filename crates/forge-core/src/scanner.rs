//! AST Scanner — tree-sitter based code symbol extraction.
//!
//! Parses Rust, TypeScript, and Python files, extracts struct/fn/trait/enum/class symbols.
//! Ported from Jarvis `ast_indexer.rs` — no DB dependency, pure in-memory pipeline.

use std::path::Path;

use anyhow::Result;
use ignore::WalkBuilder;
use tracing::{info, warn};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A code symbol extracted from source.
#[derive(Debug, Clone)]
pub struct CodeNode {
    pub id: String,
    pub file_path: String,       // relative to project root
    pub node_type: String,       // "function", "struct", "enum", "trait", "impl", "type", "const", "mod"
    pub name: String,            // identifier
    pub signature: String,       // full signature line(s)
    pub start_line: u32,
    pub end_line: u32,
    pub is_exported: bool,       // pub/export
    pub docstring: Option<String>,
    pub stub_content: String,    // condensed native-syntax stub
    pub calls: Vec<String>,      // function names called within body
    pub beacons: Vec<String>,    // @beacon: tags
}

// ---------------------------------------------------------------------------
// Language detection
// ---------------------------------------------------------------------------

/// Detect the primary language of a project by checking for marker files.
pub fn detect_language(project_root: &Path) -> String {
    if project_root.join("Cargo.toml").exists() {
        if project_root.join("package.json").exists() {
            return "mixed".to_string();
        }
        return "rust".to_string();
    }
    if project_root.join("package.json").exists() || project_root.join("tsconfig.json").exists() {
        return "typescript".to_string();
    }
    if project_root.join("setup.py").exists()
        || project_root.join("pyproject.toml").exists()
        || project_root.join("requirements.txt").exists()
    {
        return "python".to_string();
    }
    if project_root.join("go.mod").exists() {
        return "go".to_string();
    }
    "unknown".to_string()
}

// ---------------------------------------------------------------------------
// Full project scan
// ---------------------------------------------------------------------------

/// Scan a project directory and extract all code nodes.
///
/// Uses the `ignore` crate's `WalkBuilder` to efficiently walk directories,
/// automatically respecting `.gitignore` and skipping `node_modules`, `target`, `.git`.
///
/// `language` can be "rust", "typescript", "mixed", or "auto" (auto-detect).
pub fn auto_scan(project_root: &Path, language: &str) -> Result<Vec<CodeNode>> {
    let lang = if language == "auto" {
        detect_language(project_root)
    } else {
        language.to_string()
    };

    let scan_rust = matches!(lang.as_str(), "rust" | "mixed");
    let scan_ts = matches!(lang.as_str(), "typescript" | "mixed");
    let scan_py = matches!(lang.as_str(), "python");

    let mut all_nodes = Vec::new();

    let walker = WalkBuilder::new(project_root)
        .hidden(true)           // skip hidden dirs by default
        .git_ignore(true)       // respect .gitignore
        .git_global(false)
        .git_exclude(false)
        .filter_entry(|entry| {
            let name = entry.file_name().to_string_lossy();
            // Explicitly skip common large directories
            !matches!(name.as_ref(), "target" | "node_modules" | ".git" | ".next" | "dist" | "build")
        })
        .build();

    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");

        let should_parse = match ext {
            "rs" => scan_rust,
            "ts" | "tsx" => scan_ts,
            "py" => scan_py,
            _ => false,
        };

        if !should_parse {
            continue;
        }

        let source = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(_) => continue,
        };

        let rel_path = path
            .strip_prefix(project_root)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();

        let nodes = match ext {
            "rs" => extract_rust(&rel_path, &source),
            "ts" | "tsx" => extract_typescript(&rel_path, &source),
            "py" => extract_python(&rel_path, &source),
            _ => vec![],
        };

        all_nodes.extend(nodes);
    }

    info!(nodes = all_nodes.len(), language = %lang, "AST scan complete");
    Ok(all_nodes)
}

// ---------------------------------------------------------------------------
// Rust extraction
// ---------------------------------------------------------------------------

/// Parse a Rust file and extract code nodes.
pub fn extract_rust(file_path: &str, source: &str) -> Vec<CodeNode> {
    let mut parser = tree_sitter::Parser::new();
    let language = tree_sitter_rust::LANGUAGE;
    parser
        .set_language(&language.into())
        .expect("Error loading Rust grammar");

    let Some(tree) = parser.parse(source, None) else {
        warn!(file = file_path, "Failed to parse Rust file");
        return vec![];
    };

    let mut nodes = Vec::new();
    let root = tree.root_node();
    collect_rust_nodes(root, source.as_bytes(), file_path, &mut nodes);
    nodes
}

fn collect_rust_nodes(
    node: tree_sitter::Node,
    src: &[u8],
    file_path: &str,
    out: &mut Vec<CodeNode>,
) {
    let kind = node.kind();

    match kind {
        "function_item" | "struct_item" | "enum_item" | "trait_item" | "impl_item"
        | "type_item" | "const_item" | "static_item" | "mod_item" => {
            if let Some(code_node) = extract_rust_node(node, src, file_path) {
                out.push(code_node);
            }
        }
        _ => {}
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_rust_nodes(child, src, file_path, out);
    }
}

fn extract_rust_node(
    node: tree_sitter::Node,
    src: &[u8],
    file_path: &str,
) -> Option<CodeNode> {
    let kind = node.kind();
    let start_line = node.start_position().row as u32 + 1;
    let end_line = node.end_position().row as u32 + 1;
    let full_text = node_text(node, src);

    // Extract name
    let name = find_child_by_type(node, "identifier")
        .or_else(|| find_child_by_type(node, "type_identifier"))
        .map(|n| node_text(n, src))
        .unwrap_or_default();

    if name.is_empty() && kind != "impl_item" {
        return None;
    }

    // For impl blocks, use "impl TypeName"
    let name = if kind == "impl_item" {
        let type_name = find_child_by_type(node, "type_identifier")
            .map(|n| node_text(n, src))
            .unwrap_or_else(|| "Unknown".to_string());
        format!("impl {}", type_name)
    } else {
        name
    };

    let node_type = match kind {
        "function_item" => "function",
        "struct_item" => "struct",
        "enum_item" => "enum",
        "trait_item" => "trait",
        "impl_item" => "impl",
        "type_item" => "type",
        "const_item" => "const",
        "static_item" => "static",
        "mod_item" => "mod",
        _ => return None,
    };

    // Check if exported (has visibility_modifier "pub")
    let is_exported = node
        .children(&mut node.walk())
        .any(|child| child.kind() == "visibility_modifier");

    // Extract signature (first line(s) before the body block)
    let signature = extract_signature(&full_text);

    // Extract docstring (preceding line_comment with ///)
    let docstring = extract_preceding_docs(node, src);

    // Extract beacon tags from preceding comments
    let beacons = extract_beacon_tags(node, src);

    // Extract function calls
    let calls = if kind == "function_item" {
        extract_calls(node, src)
    } else {
        vec![]
    };

    // Build stub content
    let stub = build_stub(&signature, &calls, kind);

    // Generate deterministic ID
    let id = format!(
        "{:x}",
        hash_id(&format!("{file_path}:{node_type}:{name}:{start_line}"))
    );

    Some(CodeNode {
        id,
        file_path: file_path.to_string(),
        node_type: node_type.to_string(),
        name,
        signature,
        start_line,
        end_line,
        is_exported,
        docstring,
        stub_content: stub,
        calls,
        beacons,
    })
}

// ---------------------------------------------------------------------------
// TypeScript extraction
// ---------------------------------------------------------------------------

/// Parse a TypeScript file and extract code nodes.
pub fn extract_typescript(file_path: &str, source: &str) -> Vec<CodeNode> {
    let mut parser = tree_sitter::Parser::new();
    let language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT;
    parser
        .set_language(&language.into())
        .expect("Error loading TypeScript grammar");

    let Some(tree) = parser.parse(source, None) else {
        warn!(file = file_path, "Failed to parse TypeScript file");
        return vec![];
    };

    let mut nodes = Vec::new();
    let root = tree.root_node();
    collect_ts_nodes(root, source.as_bytes(), file_path, &mut nodes);
    nodes
}

fn collect_ts_nodes(
    node: tree_sitter::Node,
    src: &[u8],
    file_path: &str,
    out: &mut Vec<CodeNode>,
) {
    let kind = node.kind();

    match kind {
        "function_declaration" | "class_declaration" | "interface_declaration"
        | "type_alias_declaration" | "enum_declaration" | "export_statement" => {
            if let Some(code_node) = extract_ts_node(node, src, file_path) {
                out.push(code_node);
            }
        }
        _ => {}
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_ts_nodes(child, src, file_path, out);
    }
}

fn extract_ts_node(
    node: tree_sitter::Node,
    src: &[u8],
    file_path: &str,
) -> Option<CodeNode> {
    let kind = node.kind();
    let start_line = node.start_position().row as u32 + 1;
    let end_line = node.end_position().row as u32 + 1;
    let full_text = node_text(node, src);

    // For export statements, look at the inner declaration
    let (actual_node, is_exported) = if kind == "export_statement" {
        let inner = node.children(&mut node.walk()).find(|c| {
            matches!(
                c.kind(),
                "function_declaration"
                    | "class_declaration"
                    | "interface_declaration"
                    | "type_alias_declaration"
            )
        });
        if let Some(inner) = inner {
            (inner, true)
        } else {
            return None;
        }
    } else {
        (node, false)
    };

    let name = find_child_by_type(actual_node, "identifier")
        .or_else(|| find_child_by_type(actual_node, "type_identifier"))
        .map(|n| node_text(n, src))
        .unwrap_or_default();

    if name.is_empty() {
        return None;
    }

    let node_type = match actual_node.kind() {
        "function_declaration" => "function",
        "class_declaration" => "class",
        "interface_declaration" => "interface",
        "type_alias_declaration" => "type",
        "enum_declaration" => "enum",
        _ => return None,
    };

    let signature = extract_signature(&full_text);
    let docstring = None; // TS doc extraction simplified
    let stub = build_stub(&signature, &[], actual_node.kind());
    let id = format!(
        "{:x}",
        hash_id(&format!("{file_path}:{node_type}:{name}:{start_line}"))
    );

    Some(CodeNode {
        id,
        file_path: file_path.to_string(),
        node_type: node_type.to_string(),
        name,
        signature,
        start_line,
        end_line,
        is_exported,
        docstring,
        stub_content: stub,
        calls: vec![],
        beacons: vec![],
    })
}

// ---------------------------------------------------------------------------
// Python extraction
// ---------------------------------------------------------------------------

/// Parse a Python file and extract code nodes.
pub fn extract_python(file_path: &str, source: &str) -> Vec<CodeNode> {
    let mut parser = tree_sitter::Parser::new();
    let language = tree_sitter_python::LANGUAGE;
    parser
        .set_language(&language.into())
        .expect("Error loading Python grammar");

    let Some(tree) = parser.parse(source, None) else {
        warn!(file = file_path, "Failed to parse Python file");
        return vec![];
    };

    let mut nodes = Vec::new();
    let root = tree.root_node();
    collect_python_nodes(root, source.as_bytes(), file_path, &mut nodes, false);
    nodes
}

fn collect_python_nodes(
    node: tree_sitter::Node,
    src: &[u8],
    file_path: &str,
    out: &mut Vec<CodeNode>,
    inside_class: bool,
) {
    let kind = node.kind();

    match kind {
        "function_definition" => {
            if let Some(code_node) = extract_python_node(node, src, file_path, inside_class) {
                out.push(code_node);
            }
        }
        "class_definition" => {
            if let Some(code_node) = extract_python_node(node, src, file_path, false) {
                out.push(code_node);
            }
            // Recurse into class body to find methods
            if let Some(body) = find_child_by_type(node, "block") {
                let mut cursor = body.walk();
                for child in body.children(&mut cursor) {
                    collect_python_nodes(child, src, file_path, out, true);
                }
            }
            return; // Already recursed into class body
        }
        "decorated_definition" => {
            // Unwrap to the inner function_definition or class_definition
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                match child.kind() {
                    "function_definition" => {
                        if let Some(mut code_node) =
                            extract_python_node(child, src, file_path, inside_class)
                        {
                            // Use the decorator's start line for the full span
                            code_node.start_line = node.start_position().row as u32 + 1;
                            out.push(code_node);
                        }
                    }
                    "class_definition" => {
                        if let Some(mut code_node) =
                            extract_python_node(child, src, file_path, false)
                        {
                            code_node.start_line = node.start_position().row as u32 + 1;
                            out.push(code_node);
                        }
                        // Recurse into class body to find methods
                        if let Some(body) = find_child_by_type(child, "block") {
                            let mut cursor2 = body.walk();
                            for grandchild in body.children(&mut cursor2) {
                                collect_python_nodes(
                                    grandchild,
                                    src,
                                    file_path,
                                    out,
                                    true,
                                );
                            }
                        }
                    }
                    _ => {}
                }
            }
            return; // Already handled children
        }
        _ => {}
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_python_nodes(child, src, file_path, out, inside_class);
    }
}

fn extract_python_node(
    node: tree_sitter::Node,
    src: &[u8],
    file_path: &str,
    inside_class: bool,
) -> Option<CodeNode> {
    let kind = node.kind();
    let start_line = node.start_position().row as u32 + 1;
    let end_line = node.end_position().row as u32 + 1;

    let name = find_child_by_type(node, "identifier")
        .map(|n| node_text(n, src))
        .unwrap_or_default();

    if name.is_empty() {
        return None;
    }

    let node_type = match kind {
        "function_definition" => {
            if inside_class { "method" } else { "function" }
        }
        "class_definition" => "class",
        _ => return None,
    };

    // Build signature: "def name(params):" or "class Name(bases):"
    let signature = match kind {
        "function_definition" => {
            let params = find_child_by_type(node, "parameters")
                .map(|n| node_text(n, src))
                .unwrap_or_else(|| "()".to_string());
            let ret = find_child_by_type(node, "type")
                .map(|n| format!(" -> {}", node_text(n, src)))
                .unwrap_or_default();
            format!("def {}{}{}", name, params, ret)
        }
        "class_definition" => {
            let bases = find_child_by_type(node, "argument_list")
                .map(|n| node_text(n, src))
                .unwrap_or_default();
            if bases.is_empty() {
                format!("class {}", name)
            } else {
                format!("class {}{}", name, bases)
            }
        }
        _ => name.clone(),
    };

    // Python: names starting with _ are private, otherwise "exported"
    let is_exported = !name.starts_with('_');

    // Extract docstring (first expression_statement with string child in the body)
    let docstring = extract_python_docstring(node, src);

    // Extract function calls
    let calls = if kind == "function_definition" {
        extract_python_calls(node, src)
    } else {
        vec![]
    };

    let stub = build_stub_python(&signature, &calls, kind);

    let id = format!(
        "{:x}",
        hash_id(&format!("{file_path}:{node_type}:{name}:{start_line}"))
    );

    Some(CodeNode {
        id,
        file_path: file_path.to_string(),
        node_type: node_type.to_string(),
        name,
        signature,
        start_line,
        end_line,
        is_exported,
        docstring,
        stub_content: stub,
        calls,
        beacons: vec![],
    })
}

/// Extract docstring from a Python function or class body.
fn extract_python_docstring(node: tree_sitter::Node, src: &[u8]) -> Option<String> {
    let body = find_child_by_type(node, "block")?;
    let mut cursor = body.walk();
    let expr_stmt = body
        .children(&mut cursor)
        .find(|c| c.kind() == "expression_statement")?;
    let mut cursor2 = expr_stmt.walk();
    let string_node = expr_stmt
        .children(&mut cursor2)
        .find(|c| c.kind() == "string")?;
    let raw = node_text(string_node, src);
    let cleaned = raw
        .trim_start_matches("\"\"\"")
        .trim_start_matches("'''")
        .trim_end_matches("\"\"\"")
        .trim_end_matches("'''")
        .trim()
        .to_string();
    if cleaned.is_empty() { None } else { Some(cleaned) }
}

/// Extract function call names from a Python function body.
fn extract_python_calls(node: tree_sitter::Node, src: &[u8]) -> Vec<String> {
    let mut calls = Vec::new();
    collect_python_calls_recursive(node, src, &mut calls);
    calls.sort();
    calls.dedup();
    calls
}

fn collect_python_calls_recursive(
    node: tree_sitter::Node,
    src: &[u8],
    calls: &mut Vec<String>,
) {
    if node.kind() == "call" {
        if let Some(func) = node.child(0) {
            let text = node_text(func, src);
            // Take last component: "os.path.join" → "join", "foo" → "foo"
            let name = text.rsplit('.').next().unwrap_or(&text).to_string();
            if !name.is_empty() && name.len() < 50 {
                calls.push(name);
            }
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_python_calls_recursive(child, src, calls);
    }
}

/// Build a condensed Python stub.
fn build_stub_python(signature: &str, calls: &[String], kind: &str) -> String {
    match kind {
        "function_definition" => {
            if calls.is_empty() {
                format!("{}:\n    ...", signature)
            } else {
                format!(
                    "{}:\n    # Calls: {}\n    ...",
                    signature,
                    calls.join(", ")
                )
            }
        }
        "class_definition" => format!("{}:\n    ...", signature),
        _ => signature.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Extract signature: everything before { or ;
fn extract_signature(full_text: &str) -> String {
    if let Some(brace_pos) = full_text.find('{') {
        full_text[..brace_pos].trim().to_string()
    } else if let Some(semi_pos) = full_text.find(';') {
        full_text[..=semi_pos].trim().to_string()
    } else {
        full_text.lines().next().unwrap_or("").to_string()
    }
}

/// Build a condensed stub in native syntax.
fn build_stub(signature: &str, calls: &[String], kind: &str) -> String {
    match kind {
        "function_item" | "function_declaration" => {
            if calls.is_empty() {
                format!("{} {{ /* ... */ }}", signature)
            } else {
                format!(
                    "{} {{\n    // Calls: {}\n    /* ... */\n}}",
                    signature,
                    calls.join(", ")
                )
            }
        }
        "struct_item" | "enum_item" | "trait_item" | "class_declaration"
        | "interface_declaration" | "enum_declaration" => {
            format!("{} {{ /* ... */ }}", signature)
        }
        _ => signature.to_string(),
    }
}

/// Extract preceding doc comments (/// lines).
fn extract_preceding_docs(node: tree_sitter::Node, src: &[u8]) -> Option<String> {
    let mut docs = Vec::new();
    let mut sibling = node.prev_sibling();
    while let Some(s) = sibling {
        if s.kind() == "line_comment" {
            let text = node_text(s, src);
            if text.starts_with("///") {
                docs.push(text[3..].trim().to_string());
            } else if text.starts_with("//!") {
                docs.push(text[3..].trim().to_string());
            } else {
                break;
            }
        } else if s.kind() == "attribute_item" {
            // Skip attributes like #[derive(...)]
        } else {
            break;
        }
        sibling = s.prev_sibling();
    }

    if docs.is_empty() {
        None
    } else {
        docs.reverse();
        Some(docs.join("\n"))
    }
}

/// Extract @beacon: tags from preceding comments.
fn extract_beacon_tags(node: tree_sitter::Node, src: &[u8]) -> Vec<String> {
    let mut tags = Vec::new();
    let mut sibling = node.prev_sibling();
    while let Some(s) = sibling {
        if s.kind() == "line_comment" {
            let text = node_text(s, src);
            if let Some(pos) = text.find("@beacon:") {
                let tag = text[pos + 8..].trim();
                for t in tag.split_whitespace() {
                    tags.push(t.to_string());
                }
            }
        } else if s.kind() != "attribute_item" {
            break;
        }
        sibling = s.prev_sibling();
    }
    tags
}

/// Extract function call names from a function body.
fn extract_calls(node: tree_sitter::Node, src: &[u8]) -> Vec<String> {
    let mut calls = Vec::new();
    collect_calls_recursive(node, src, &mut calls);
    calls.sort();
    calls.dedup();
    calls
}

fn collect_calls_recursive(node: tree_sitter::Node, src: &[u8], calls: &mut Vec<String>) {
    if node.kind() == "call_expression" {
        if let Some(func) = node.child(0) {
            let text = node_text(func, src);
            let name = text
                .rsplit("::")
                .next()
                .unwrap_or(&text)
                .rsplit('.')
                .next()
                .unwrap_or(&text)
                .to_string();
            if !name.is_empty() && name.len() < 50 {
                calls.push(name);
            }
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_calls_recursive(child, src, calls);
    }
}

/// Extract text content of a node.
fn node_text(node: tree_sitter::Node, src: &[u8]) -> String {
    node.utf8_text(src).unwrap_or("").to_string()
}

fn find_child_by_type<'a>(
    node: tree_sitter::Node<'a>,
    kind: &str,
) -> Option<tree_sitter::Node<'a>> {
    let mut cursor = node.walk();
    let result = node.children(&mut cursor).find(|c| c.kind() == kind);
    result
}

/// Simple hash for deterministic IDs.
fn hash_id(input: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    input.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_rust_function() {
        let source = r#"
pub fn hello(name: &str) -> String {
    format!("Hello, {}", name)
}
"#;
        let nodes = extract_rust("test.rs", source);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].name, "hello");
        assert_eq!(nodes[0].node_type, "function");
        assert!(nodes[0].is_exported);
    }

    #[test]
    fn test_extract_rust_struct() {
        let source = r#"
pub struct Config {
    pub name: String,
    pub value: i32,
}
"#;
        let nodes = extract_rust("test.rs", source);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].name, "Config");
        assert_eq!(nodes[0].node_type, "struct");
    }

    #[test]
    fn test_extract_typescript() {
        let source = r#"
export function greet(name: string): string {
    return `Hello, ${name}`;
}
"#;
        let nodes = extract_typescript("test.ts", source);
        // export_statement + inner function_declaration both produce a node
        assert!(nodes.len() >= 1);
        let exported = nodes.iter().find(|n| n.is_exported).unwrap();
        assert_eq!(exported.name, "greet");
        assert_eq!(exported.node_type, "function");
    }

    #[test]
    fn test_extract_python_function() {
        let source = r#"
def greet(name: str) -> str:
    """Say hello."""
    return f"Hello, {name}"
"#;
        let nodes = extract_python("test.py", source);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].name, "greet");
        assert_eq!(nodes[0].node_type, "function");
        assert!(nodes[0].is_exported);
        assert_eq!(nodes[0].docstring.as_deref(), Some("Say hello."));
    }

    #[test]
    fn test_extract_python_class_with_methods() {
        let source = r#"
class MyNode:
    """A ComfyUI node."""

    def __init__(self, value):
        self.value = value

    def process(self, input_data):
        return input_data
"#;
        let nodes = extract_python("nodes.py", source);
        // class + 2 methods
        assert_eq!(nodes.len(), 3);
        assert_eq!(nodes[0].name, "MyNode");
        assert_eq!(nodes[0].node_type, "class");
        assert_eq!(nodes[1].name, "__init__");
        assert_eq!(nodes[1].node_type, "method");
        assert!(!nodes[1].is_exported); // starts with _
        assert_eq!(nodes[2].name, "process");
        assert_eq!(nodes[2].node_type, "method");
    }

    #[test]
    fn test_extract_python_decorated() {
        let source = r#"
@staticmethod
def helper():
    pass

@dataclass
class Config:
    name: str
"#;
        let nodes = extract_python("test.py", source);
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].name, "helper");
        assert_eq!(nodes[0].node_type, "function");
        assert_eq!(nodes[1].name, "Config");
        assert_eq!(nodes[1].node_type, "class");
    }

    #[test]
    fn test_detect_language() {
        // This test would need a temp dir; just ensure it returns "unknown" for non-existent
        let lang = detect_language(std::path::Path::new("/nonexistent"));
        assert_eq!(lang, "unknown");
    }
}
