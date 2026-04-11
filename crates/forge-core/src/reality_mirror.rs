//! Reality Mirror — generates `reality.sexp` from in-memory CodeNodes.
//!
//! Ported from Jarvis `reality_mirror.rs`. Key difference: NO database dependency.
//! Takes `&[CodeNode]` directly and clusters by directory structure or existing topology.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::scanner::CodeNode;
use crate::sexp_parser::SexpNode;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Generate a reality S-EXP string from code nodes.
///
/// - `nodes`: all extracted `CodeNode` from the scanner
/// - `project_name`: name used as the root identifier in the S-EXP
/// - `existing_topology`: optional existing topology S-EXP string; if provided,
///   nodes are clustered using its declared pillar/component structure.
///   Otherwise, auto-cluster by directory structure.
pub fn generate(
    nodes: &[CodeNode],
    project_name: &str,
    existing_topology: Option<&str>,
) -> String {
    let (components, orphans) = if let Some(topo_text) = existing_topology {
        cluster_by_topology(nodes, topo_text)
    } else {
        cluster_by_directory(nodes)
    };

    let tree = build_sexp_tree(project_name, &components, &orphans);
    tree.to_pretty(0)
}

/// Write the reality S-EXP to `.jarvis/reality.sexp` inside a project.
///
/// Creates `.jarvis/` and `.jarvis/.gitignore` if they don't exist.
pub fn write_to_disk(
    project_root: &Path,
    nodes: &[CodeNode],
    project_name: &str,
    existing_topology: Option<&str>,
) -> anyhow::Result<std::path::PathBuf> {
    let text = generate(nodes, project_name, existing_topology);

    let jarvis_dir = project_root.join(".jarvis");
    std::fs::create_dir_all(&jarvis_dir)?;

    // Write .gitignore to exclude reality.sexp (auto-generated)
    let gitignore_path = jarvis_dir.join(".gitignore");
    if !gitignore_path.exists() {
        std::fs::write(&gitignore_path, "reality.sexp\n")?;
    }

    let out_path = jarvis_dir.join("reality.sexp");
    std::fs::write(&out_path, text.as_bytes())?;

    Ok(out_path)
}

// ---------------------------------------------------------------------------
// Internal types
// ---------------------------------------------------------------------------

struct ComponentBucket {
    pillar_id: String,
    comp_id: String,
    files: BTreeSet<String>,
    symbols: Vec<SymbolEntry>,
}

struct SymbolEntry {
    node_type: String,
    name: String,
    is_exported: bool,
    signature: String,
}

// ---------------------------------------------------------------------------
// Clustering: by existing topology
// ---------------------------------------------------------------------------

fn cluster_by_topology(
    nodes: &[CodeNode],
    topo_text: &str,
) -> (Vec<ComponentBucket>, BTreeMap<String, Vec<SymbolEntry>>) {
    let sexp_nodes = SexpNode::parse(topo_text).unwrap_or_default();
    let root = match sexp_nodes.first() {
        Some(r) => r,
        None => return cluster_by_directory(nodes),
    };

    // Extract pillar/component declarations
    let mut pillar_comp_list: Vec<(String, String)> = Vec::new();

    for pillar_node in root.find_all("pillar") {
        let pillar_id = pillar_node.id().unwrap_or("").to_string();
        if pillar_id.is_empty() {
            continue;
        }
        if let Some(comps) = pillar_node.find("components") {
            for comp in comps.children().iter().skip(1) {
                // skip the "components" tag atom
                if let Some(comp_id) = comp.as_atom() {
                    pillar_comp_list.push((pillar_id.clone(), comp_id.to_string()));
                } else if let Some(comp_id) = comp.tag() {
                    pillar_comp_list.push((pillar_id.clone(), comp_id.to_string()));
                }
            }
        }
    }

    // Assign files to components using path heuristics:
    // file_path contains pillar_id and comp_id (with _ -> - normalization)
    let mut file_to_comp: BTreeMap<String, (String, String)> = BTreeMap::new();

    for node in nodes {
        let fp = &node.file_path;
        for (pillar_id, comp_id) in &pillar_comp_list {
            let comp_underscore = comp_id.replace('-', "_");
            let pillar_underscore = pillar_id.replace('-', "_");

            let pillar_match = fp.contains(&format!("/{}/", pillar_id))
                || fp.contains(&format!("/{}/", pillar_underscore));
            let comp_match = fp.contains(&format!("/{}/", comp_id))
                || fp.contains(&format!("/{}/", comp_underscore))
                || fp.contains(&format!("/{}.rs", comp_underscore))
                || fp.contains(&format!("/{}.ts", comp_underscore))
                || fp.contains(&format!("/{}.rs", comp_id))
                || fp.contains(&format!("/{}.ts", comp_id));

            if pillar_match && comp_match {
                file_to_comp.insert(fp.clone(), (pillar_id.clone(), comp_id.clone()));
                break;
            }
        }
    }

    build_buckets_from_mapping(nodes, &pillar_comp_list, &file_to_comp)
}

// ---------------------------------------------------------------------------
// Clustering: by directory structure (auto)
// ---------------------------------------------------------------------------

fn cluster_by_directory(
    nodes: &[CodeNode],
) -> (Vec<ComponentBucket>, BTreeMap<String, Vec<SymbolEntry>>) {
    // Group nodes by their top-level directory (the "pillar") and sub-directory (the "component").
    // e.g. "src/scanner.rs" -> pillar "src", comp "scanner"
    //      "crates/core/src/lib.rs" -> pillar "crates/core", comp "src"
    let mut pillar_comp_list: Vec<(String, String)> = Vec::new();
    let mut file_to_comp: BTreeMap<String, (String, String)> = BTreeMap::new();
    let mut seen: BTreeSet<(String, String)> = BTreeSet::new();

    for node in nodes {
        let path = Path::new(&node.file_path);
        let components: Vec<&str> = path
            .components()
            .filter_map(|c| c.as_os_str().to_str())
            .collect();

        let (pillar, comp) = match components.len() {
            0 => continue,
            1 => {
                // File at root: pillar = "root", comp = filename stem
                let stem = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown");
                ("root".to_string(), stem.to_string())
            }
            2 => {
                // dir/file.rs: pillar = dir, comp = file stem
                let stem = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown");
                (components[0].to_string(), stem.to_string())
            }
            _ => {
                // Deeper paths: use first dir as pillar, second as component
                // For crates/X/src/Y/file.rs: pillar = "crates/X", comp = "Y"
                // Find the "src" boundary if it exists
                if let Some(src_idx) = components.iter().position(|&c| c == "src") {
                    if src_idx + 2 < components.len() {
                        // Has subdir after src: pillar = everything up to src, comp = subdir after src
                        let pillar = components[..src_idx].join("/");
                        let comp = components[src_idx + 1].to_string();
                        (
                            if pillar.is_empty() {
                                "src".to_string()
                            } else {
                                pillar
                            },
                            comp,
                        )
                    } else {
                        // File directly under src/
                        let pillar = components[..src_idx].join("/");
                        let stem = path
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("unknown");
                        (
                            if pillar.is_empty() {
                                "src".to_string()
                            } else {
                                pillar
                            },
                            stem.to_string(),
                        )
                    }
                } else {
                    // No "src" dir: first dir = pillar, second dir = component
                    (components[0].to_string(), components[1].to_string())
                }
            }
        };

        let key = (pillar.clone(), comp.clone());
        if !seen.contains(&key) {
            seen.insert(key.clone());
            pillar_comp_list.push(key.clone());
        }
        file_to_comp.insert(node.file_path.clone(), (pillar, comp));
    }

    build_buckets_from_mapping(nodes, &pillar_comp_list, &file_to_comp)
}

// ---------------------------------------------------------------------------
// Shared: build buckets from file-to-component mapping
// ---------------------------------------------------------------------------

fn build_buckets_from_mapping(
    nodes: &[CodeNode],
    pillar_comp_list: &[(String, String)],
    file_to_comp: &BTreeMap<String, (String, String)>,
) -> (Vec<ComponentBucket>, BTreeMap<String, Vec<SymbolEntry>>) {
    let mut buckets: BTreeMap<(String, String), ComponentBucket> = BTreeMap::new();

    for (pillar_id, comp_id) in pillar_comp_list {
        buckets.insert(
            (pillar_id.clone(), comp_id.clone()),
            ComponentBucket {
                pillar_id: pillar_id.clone(),
                comp_id: comp_id.clone(),
                files: BTreeSet::new(),
                symbols: Vec::new(),
            },
        );
    }

    let mut orphans: BTreeMap<String, Vec<SymbolEntry>> = BTreeMap::new();

    for node in nodes {
        // Apply noise filter
        if should_filter(&node.node_type, &node.name, node.is_exported, node.start_line, node.end_line) {
            continue;
        }

        let sym = SymbolEntry {
            node_type: node.node_type.clone(),
            name: node.name.clone(),
            is_exported: node.is_exported,
            signature: truncate_sig(&node.signature),
        };

        if let Some((pillar_id, comp_id)) = file_to_comp.get(&node.file_path) {
            let key = (pillar_id.clone(), comp_id.clone());
            if let Some(bucket) = buckets.get_mut(&key) {
                bucket.files.insert(node.file_path.clone());
                bucket.symbols.push(sym);
            } else {
                orphans.entry(node.file_path.clone()).or_default().push(sym);
            }
        } else {
            orphans.entry(node.file_path.clone()).or_default().push(sym);
        }
    }

    // Collect in declaration order
    let components: Vec<ComponentBucket> = pillar_comp_list
        .iter()
        .filter_map(|key| buckets.remove(key))
        .collect();

    (components, orphans)
}

// ---------------------------------------------------------------------------
// S-EXP tree construction
// ---------------------------------------------------------------------------

fn build_sexp_tree(
    project_name: &str,
    components: &[ComponentBucket],
    orphans: &BTreeMap<String, Vec<SymbolEntry>>,
) -> SexpNode {
    // Group components by pillar
    let mut pillars: BTreeMap<&str, Vec<&ComponentBucket>> = BTreeMap::new();
    for comp in components {
        pillars.entry(&comp.pillar_id).or_default().push(comp);
    }

    let mut top_children: Vec<SexpNode> = Vec::new();
    top_children.push(atom("reality"));
    top_children.push(atom(project_name));

    // Pillar nodes
    for (pillar_id, comps) in &pillars {
        let mut pillar_children: Vec<SexpNode> = Vec::new();
        pillar_children.push(atom("pillar"));
        pillar_children.push(atom(pillar_id));

        // Components list
        let mut comp_list_node = vec![atom("components")];
        for comp in comps {
            comp_list_node.push(atom(&comp.comp_id));
        }
        pillar_children.push(SexpNode::List(comp_list_node));

        // Each component
        for comp in comps {
            pillar_children.push(build_component_node(comp));
        }

        top_children.push(SexpNode::List(pillar_children));
    }

    // Orphans
    if !orphans.is_empty() {
        let mut orphans_children: Vec<SexpNode> = Vec::new();
        orphans_children.push(atom("orphans"));

        for (file_path, syms) in orphans {
            let mut file_children: Vec<SexpNode> = Vec::new();
            file_children.push(atom("file"));
            file_children.push(atom(file_path));

            let mut sym_node = vec![atom("symbols")];
            for sym in syms {
                sym_node.push(build_symbol_node(sym));
            }
            file_children.push(SexpNode::List(sym_node));

            orphans_children.push(SexpNode::List(file_children));
        }

        top_children.push(SexpNode::List(orphans_children));
    }

    SexpNode::List(top_children)
}

/// Build a component S-EXP node.
fn build_component_node(comp: &ComponentBucket) -> SexpNode {
    let mut comp_children: Vec<SexpNode> = Vec::new();
    comp_children.push(atom("component"));
    comp_children.push(atom(&comp.comp_id));

    // (files "file1" "file2" ...)
    if !comp.files.is_empty() {
        let mut files_node = vec![atom("files")];
        for f in &comp.files {
            files_node.push(quoted(f));
        }
        comp_children.push(SexpNode::List(files_node));
    }

    // (symbols ...)
    if !comp.symbols.is_empty() {
        let mut sym_node = vec![atom("symbols")];
        for sym in &comp.symbols {
            sym_node.push(build_symbol_node(sym));
        }
        comp_children.push(SexpNode::List(sym_node));
    }

    SexpNode::List(comp_children)
}

/// Build a symbol S-EXP node like `(fn name (exported true) (sig "..."))`.
fn build_symbol_node(sym: &SymbolEntry) -> SexpNode {
    let mut children: Vec<SexpNode> = Vec::new();
    children.push(atom(&sym.node_type));
    children.push(atom(&sym.name));
    children.push(SexpNode::List(vec![
        atom("exported"),
        atom(if sym.is_exported { "true" } else { "false" }),
    ]));
    if !sym.signature.is_empty() {
        children.push(SexpNode::List(vec![atom("sig"), quoted(&sym.signature)]));
    }
    SexpNode::List(children)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn atom(s: &str) -> SexpNode {
    SexpNode::Atom(s.to_string())
}

/// Create a quoted atom (wraps in double quotes for paths/signatures).
fn quoted(s: &str) -> SexpNode {
    SexpNode::Atom(format!("\"{}\"", s))
}

/// Truncate a signature to 120 characters, appending "..." if truncated.
fn truncate_sig(sig: &str) -> String {
    if sig.len() > 120 {
        format!("{}...", &sig[..120])
    } else {
        sig.to_string()
    }
}

/// Noise filter: returns true if the AST node should be excluded.
fn should_filter(
    node_type: &str,
    name: &str,
    is_exported: bool,
    start_line: u32,
    end_line: u32,
) -> bool {
    // Never filter exported symbols
    if is_exported {
        return false;
    }

    // Filter pure impl blocks
    if node_type == "impl" && name.starts_with("impl ") {
        return true;
    }

    // Filter trivial getter/setter: body <= 2 lines
    let body_lines = end_line.saturating_sub(start_line);
    if body_lines <= 2
        && (name.starts_with("get_") || name.starts_with("set_") || name.starts_with("is_"))
    {
        return true;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scanner::extract_rust;

    #[test]
    fn test_generate_from_rust_source() {
        let source = r#"
pub struct Config {
    pub name: String,
}

pub fn init() -> Config {
    Config { name: "test".into() }
}

fn internal_helper() {
    // not exported
}
"#;
        let nodes = extract_rust("src/config.rs", source);
        let sexp = generate(&nodes, "test-project", None);
        assert!(sexp.contains("reality"));
        assert!(sexp.contains("test-project"));
        assert!(sexp.contains("Config"));
        assert!(sexp.contains("init"));
    }

    #[test]
    fn test_noise_filter_excludes_private_impl() {
        assert!(should_filter("impl", "impl MyStruct", false, 1, 20));
        // Exported should not be filtered
        assert!(!should_filter("impl", "impl MyStruct", true, 1, 20));
    }

    #[test]
    fn test_noise_filter_excludes_trivial_getters() {
        assert!(should_filter("function", "get_name", false, 10, 12));
        assert!(!should_filter("function", "get_name", false, 10, 20));
    }
}
