//! Universe Graph — cross-service dependency DAG for blast-radius analysis.
//!
//! Parses a `universe.intent.lisp` manifest to discover all services,
//! then scans each service's `intent.lisp` for `upstream-dependency`
//! declarations.  The resulting DAG answers: "if I change service A,
//! which downstream services break, and in what repair order?"

use crate::sexp_parser::SexpNode;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

// ============================================================================
// Data types
// ============================================================================

/// A service declared in the universe manifest.
#[derive(Debug, Clone)]
pub struct ServiceEntry {
    /// Short identifier, e.g. "auth".
    pub id: String,
    /// Relative path to the service root (relative to `universe.intent.lisp`).
    pub path: PathBuf,
}

/// A cross-service dependency extracted from a service's `intent.lisp`.
#[derive(Debug, Clone)]
pub struct ServiceDependency {
    /// The upstream service that this dependency points to.
    pub provider_id: String,
    /// Interface names consumed from the provider.
    pub consumes: Vec<String>,
    /// Conditions under which this dependency breaks.
    pub breaks_if: Vec<String>,
    /// The file from which this dependency was extracted.
    pub source_file: PathBuf,
}

/// The full cross-service dependency graph for a universe.
#[derive(Debug)]
pub struct UniverseGraph {
    pub services: Vec<ServiceEntry>,
    /// Maps `consumer_id` to its upstream dependencies.
    pub dependencies: HashMap<String, Vec<ServiceDependency>>,
}

/// Describes a set of changes made to a single service.
#[derive(Debug, Clone)]
pub struct ServiceDelta {
    pub service_id: String,
    pub changed_interfaces: Vec<String>,
}

/// A service that falls within a change's blast radius.
#[derive(Debug, Clone)]
pub struct ImpactedService {
    pub service_id: String,
    /// Human-readable explanation of why this service is affected.
    pub reason: String,
    /// Topological distance from the change source (0 = direct consumer).
    pub depth: usize,
}

// ============================================================================
// Keyword-arg helper
// ============================================================================

/// Given a list of `SexpNode` children, find the value atom after a keyword
/// atom like `:path`.  Returns `None` if the keyword is absent or has no
/// following atom.
fn keyword_val<'a>(children: &'a [SexpNode], kw: &str) -> Option<&'a str> {
    children
        .iter()
        .position(|n| n.as_atom() == Some(kw))
        .and_then(|i| children.get(i + 1))
        .and_then(|n| n.as_atom())
}

/// Collect all atoms inside a parenthesized list that follows a keyword.
/// E.g. for `:consumes ("A" "B")`, returns `["A", "B"]`.
fn keyword_list_val<'a>(children: &'a [SexpNode], kw: &str) -> Vec<&'a str> {
    children
        .iter()
        .position(|n| n.as_atom() == Some(kw))
        .and_then(|i| children.get(i + 1))
        .map(|node| {
            node.children()
                .iter()
                .filter_map(|n| n.as_atom())
                .collect()
        })
        .unwrap_or_default()
}

// ============================================================================
// Parsing
// ============================================================================

/// Parse a `universe.intent.lisp` manifest and return all declared services.
///
/// Expected format:
/// ```lisp
/// (universe <name>
///   (service <id> :path "<relative-path>")
///   ...)
/// ```
pub fn parse_universe_manifest(manifest_path: &Path) -> Result<Vec<ServiceEntry>, String> {
    let content = std::fs::read_to_string(manifest_path)
        .map_err(|e| format!("cannot read {}: {e}", manifest_path.display()))?;

    let root = SexpNode::parse_one(&content)
        .map_err(|e| format!("parse error in {}: {e}", manifest_path.display()))?;

    if root.tag() != Some("universe") {
        return Err(format!(
            "expected (universe ...) as top-level form, got {:?}",
            root.tag()
        ));
    }

    let mut entries = Vec::new();

    // Parse top-level (service ...) declarations
    for svc in root.find_all("service") {
        let id = svc
            .id()
            .ok_or_else(|| "service declaration missing id".to_string())?
            .to_string();
        let path_str = keyword_val(svc.children(), ":path")
            .ok_or_else(|| format!("service '{}' missing :path", id))?;
        entries.push(ServiceEntry {
            id,
            path: PathBuf::from(path_str),
        });
    }

    // Parse (monorepo ...) declarations — expand each child service with combined path
    for mono in root.find_all("monorepo") {
        let mono_path = keyword_val(mono.children(), ":path")
            .ok_or_else(|| {
                let mono_id = mono.id().unwrap_or("unnamed");
                format!("monorepo '{}' missing :path", mono_id)
            })?;

        for svc in mono.find_all("service") {
            let id = svc
                .id()
                .ok_or_else(|| "service declaration in monorepo missing id".to_string())?
                .to_string();
            let subpath = keyword_val(svc.children(), ":subpath")
                .ok_or_else(|| format!("service '{}' in monorepo missing :subpath", id))?;
            entries.push(ServiceEntry {
                id,
                path: PathBuf::from(mono_path).join(subpath),
            });
        }
    }

    if entries.is_empty() {
        return Err("universe manifest contains no service declarations".into());
    }

    Ok(entries)
}

/// Extract `upstream-dependency` declarations from a service's `intent.lisp`
/// content string.
///
/// Expected format inside the intent form:
/// ```lisp
/// (upstream-dependency
///   (service "<provider>"
///     :consumes ("<iface>" ...)
///     :breaks-if ("<cond>" ...)))
/// ```
pub fn extract_upstream_dependencies(
    intent_content: &str,
    source_file: &Path,
) -> Vec<ServiceDependency> {
    let root = match SexpNode::parse_one(intent_content) {
        Ok(r) => r,
        Err(_) => return vec![],
    };

    let mut deps = Vec::new();

    for ud_block in root.find_all("upstream-dependency") {
        for svc_node in ud_block.find_all("service") {
            let provider_id = match svc_node.id() {
                Some(id) => id.to_string(),
                None => continue,
            };
            let consumes: Vec<String> = keyword_list_val(svc_node.children(), ":consumes")
                .into_iter()
                .map(|s| s.to_string())
                .collect();
            let breaks_if: Vec<String> = keyword_list_val(svc_node.children(), ":breaks-if")
                .into_iter()
                .map(|s| s.to_string())
                .collect();

            deps.push(ServiceDependency {
                provider_id,
                consumes,
                breaks_if,
                source_file: source_file.to_path_buf(),
            });
        }
    }

    deps
}

// ============================================================================
// Graph resolution
// ============================================================================

/// Scan a universe manifest and every service's `intent.lisp` to build the
/// full cross-service dependency graph.
///
/// Returns structured errors (one per problem) rather than panicking.
pub fn resolve_universe_graph(manifest_path: &Path) -> Result<UniverseGraph, Vec<String>> {
    let mut errors: Vec<String> = Vec::new();

    let services = match parse_universe_manifest(manifest_path) {
        Ok(s) => s,
        Err(e) => return Err(vec![e]),
    };

    let manifest_dir = manifest_path
        .parent()
        .unwrap_or_else(|| Path::new("."));

    let service_ids: HashSet<&str> = services.iter().map(|s| s.id.as_str()).collect();
    let mut dependencies: HashMap<String, Vec<ServiceDependency>> = HashMap::new();

    for svc in &services {
        let svc_root = manifest_dir.join(&svc.path);
        // Try .missiond/intent.lisp first, then .jarvis/intent.lisp, then intent.lisp
        let intent_path = [".missiond/intent.lisp", ".jarvis/intent.lisp", "intent.lisp"]
            .iter()
            .map(|p| svc_root.join(p))
            .find(|p| p.exists())
            .unwrap_or_else(|| svc_root.join("intent.lisp"));
        let content = match std::fs::read_to_string(&intent_path) {
            Ok(c) => c,
            Err(e) => {
                errors.push(format!(
                    "cannot read intent.lisp for service '{}': {} ({})",
                    svc.id,
                    e,
                    intent_path.display()
                ));
                continue;
            }
        };

        let deps = extract_upstream_dependencies(&content, &intent_path);

        // Validate that every referenced provider actually exists in the universe
        for dep in &deps {
            if !service_ids.contains(dep.provider_id.as_str()) {
                errors.push(format!(
                    "service '{}' declares upstream-dependency on unknown service '{}'",
                    svc.id, dep.provider_id
                ));
            }
        }

        if !deps.is_empty() {
            dependencies.insert(svc.id.clone(), deps);
        }
    }

    // Cycle detection — build an edge set (consumer_idx → provider_idx) and
    // use Kahn's algorithm.
    let id_to_idx: HashMap<&str, usize> = services
        .iter()
        .enumerate()
        .map(|(i, s)| (s.id.as_str(), i))
        .collect();

    let mut edges: HashSet<(usize, usize)> = HashSet::new();
    for (consumer_id, deps) in &dependencies {
        if let Some(&consumer_idx) = id_to_idx.get(consumer_id.as_str()) {
            for dep in deps {
                if let Some(&provider_idx) = id_to_idx.get(dep.provider_id.as_str()) {
                    // Edge: consumer depends on provider (consumer → provider)
                    edges.insert((consumer_idx, provider_idx));
                }
            }
        }
    }

    if has_cycle(services.len(), &edges) {
        errors.push("cycle detected in universe dependency graph".into());
    }

    if !errors.is_empty() {
        return Err(errors);
    }

    Ok(UniverseGraph {
        services,
        dependencies,
    })
}

// ============================================================================
// Blast-radius computation
// ============================================================================

/// Given a change delta, compute which services are impacted by walking
/// the **reverse** dependency edges (provider → consumers) via BFS.
///
/// Returns impacted services sorted by topological depth (nearest first),
/// suitable for a bottom-up repair order.
pub fn compute_blast_radius(
    graph: &UniverseGraph,
    delta: &ServiceDelta,
) -> Vec<ImpactedService> {
    // Build reverse index: provider_id → vec of (consumer_id, &dep)
    let mut reverse: HashMap<&str, Vec<(&str, &ServiceDependency)>> = HashMap::new();
    for (consumer_id, deps) in &graph.dependencies {
        for dep in deps {
            reverse
                .entry(dep.provider_id.as_str())
                .or_default()
                .push((consumer_id.as_str(), dep));
        }
    }

    let changed_set: HashSet<&str> = delta.changed_interfaces.iter().map(|s| s.as_str()).collect();

    let mut visited: HashSet<String> = HashSet::new();
    visited.insert(delta.service_id.clone());

    let mut queue: VecDeque<(String, usize)> = VecDeque::new();
    queue.push_back((delta.service_id.clone(), 0));

    let mut results: Vec<ImpactedService> = Vec::new();

    while let Some((current_id, depth)) = queue.pop_front() {
        let is_origin = depth == 0;

        if let Some(consumers) = reverse.get(current_id.as_str()) {
            for &(consumer_id, dep) in consumers {
                if visited.contains(consumer_id) {
                    continue;
                }

                // At depth 0 (direct consumers of the changed service), filter
                // by the specific changed interfaces.  At deeper levels the
                // upstream is already broken, so all its consumers are impacted
                // regardless of which specific interface they consume.
                let reason = if is_origin && !changed_set.is_empty() {
                    // Check if any of the consumer's breaks_if conditions
                    // match the changed interfaces
                    let triggered: Vec<&str> = dep
                        .breaks_if
                        .iter()
                        .filter(|b| changed_set.contains(b.as_str()))
                        .map(|s| s.as_str())
                        .collect();

                    if triggered.is_empty() {
                        // This consumer's break conditions don't match the changes
                        continue;
                    }
                    format!(
                        "breaks-if [{}] triggered by '{}'; consumes [{}]",
                        triggered.join(", "),
                        current_id,
                        dep.consumes.join(", "),
                    )
                } else if is_origin {
                    format!(
                        "consumes [{}] from '{}'; breaks-if: [{}]",
                        dep.consumes.join(", "),
                        current_id,
                        dep.breaks_if.join(", "),
                    )
                } else {
                    format!(
                        "transitively impacted: '{}' is broken (via '{}')",
                        current_id, delta.service_id,
                    )
                };

                visited.insert(consumer_id.to_string());
                let next_depth = depth + 1;
                results.push(ImpactedService {
                    service_id: consumer_id.to_string(),
                    reason,
                    depth: next_depth,
                });
                queue.push_back((consumer_id.to_string(), next_depth));
            }
        }
    }

    // Sort by depth (ascending) for bottom-up repair order
    results.sort_by_key(|r| r.depth);
    results
}

// ============================================================================
// Cycle detection (Kahn's algorithm)
// ============================================================================

/// Returns `true` if the directed graph contains a cycle.
/// Uses Kahn's algorithm (topological sort): if we cannot visit all nodes,
/// a cycle exists.
fn has_cycle(node_count: usize, edges: &HashSet<(usize, usize)>) -> bool {
    let mut in_degree = vec![0usize; node_count];
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); node_count];

    for &(from, to) in edges {
        adj[from].push(to);
        in_degree[to] += 1;
    }

    let mut queue: VecDeque<usize> = VecDeque::new();
    for (i, &deg) in in_degree.iter().enumerate() {
        if deg == 0 {
            queue.push_back(i);
        }
    }

    let mut visited = 0usize;
    while let Some(node) = queue.pop_front() {
        visited += 1;
        for &neighbor in &adj[node] {
            in_degree[neighbor] -= 1;
            if in_degree[neighbor] == 0 {
                queue.push_back(neighbor);
            }
        }
    }

    visited < node_count
}

// ============================================================================
// Universe integrity validation (Dependency Linter)
// ============================================================================

/// Violation found during universe integrity validation.
#[derive(Debug, Clone)]
pub struct UniverseViolation {
    pub service_id: String,
    pub severity: &'static str, // "error" or "warning"
    pub message: String,
}

/// Validate the integrity of a resolved Universe Graph.
///
/// Checks performed:
/// 1. All upstream-dependency providers exist in the universe (already done by resolve)
/// 2. No service depends on itself
/// 3. Cargo.toml cross-crate dependencies match upstream-dependency declarations
///    (if Cargo.toml exists in the service root)
/// 4. Orphan detection: services with no dependents and no dependencies (warning)
pub fn validate_universe_integrity(
    graph: &UniverseGraph,
    manifest_dir: &Path,
) -> Vec<UniverseViolation> {
    let mut violations = Vec::new();
    let _service_ids: HashSet<&str> = graph.services.iter().map(|s| s.id.as_str()).collect();

    // Check for self-dependency
    for (consumer_id, deps) in &graph.dependencies {
        for dep in deps {
            if dep.provider_id == *consumer_id {
                violations.push(UniverseViolation {
                    service_id: consumer_id.clone(),
                    severity: "error",
                    message: format!("service '{}' depends on itself", consumer_id),
                });
            }
        }
    }

    // Check Cargo.toml for undeclared cross-service dependencies
    for svc in &graph.services {
        let svc_root = manifest_dir.join(&svc.path);
        let cargo_path = svc_root.join("Cargo.toml");
        if !cargo_path.exists() {
            continue;
        }
        let cargo_content = match std::fs::read_to_string(&cargo_path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        // Declared upstream providers for this service
        let declared_providers: HashSet<&str> = graph
            .dependencies
            .get(&svc.id)
            .map(|deps| deps.iter().map(|d| d.provider_id.as_str()).collect())
            .unwrap_or_default();

        // Scan Cargo.toml for path dependencies that point to other services.
        // We resolve actual path deps from Cargo.toml and check if they
        // overlap with any other service's root directory.
        let svc_abs = manifest_dir.join(&svc.path);
        for other in &graph.services {
            if other.id == svc.id {
                continue;
            }
            let other_abs = manifest_dir.join(&other.path);
            let other_abs_str = other_abs.to_string_lossy();
            // Extract all path = "..." values from Cargo.toml and resolve them
            // against the service root to see if they point into the other service.
            let has_cargo_path_dep = cargo_content
                .lines()
                .filter_map(|line| {
                    // Match: path = "..." or path = '...'
                    let line = line.trim();
                    if let Some(start) = line.find("path") {
                        let rest = &line[start..];
                        let eq = rest.find('=')?;
                        let after_eq = rest[eq + 1..].trim();
                        let end_quote = if after_eq.starts_with('"') {
                            after_eq[1..].find('"')?
                        } else if after_eq.starts_with('\'') {
                            after_eq[1..].find('\'')?
                        } else {
                            return None;
                        };
                        let path_val = &after_eq[1..1 + end_quote];
                        Some(path_val.to_string())
                    } else {
                        None
                    }
                })
                .any(|dep_path| {
                    let resolved = svc_abs.join(&dep_path);
                    // Check if the resolved path starts with (is inside) the other service dir
                    if let (Ok(r), Ok(o)) = (resolved.canonicalize(), other_abs.canonicalize()) {
                        r.starts_with(&o)
                    } else {
                        // Fallback: string-based check on the resolved path
                        resolved.to_string_lossy().contains(&*other_abs_str)
                    }
                });

            if has_cargo_path_dep && !declared_providers.contains(other.id.as_str()) {
                violations.push(UniverseViolation {
                    service_id: svc.id.clone(),
                    severity: "warning",
                    message: format!(
                        "Cargo.toml has path dependency into '{}' but no (upstream-dependency (service \"{}\")) declared in intent.lisp",
                        other.id, other.id
                    ),
                });
            }
        }
    }

    // Orphan detection (warning only)
    let has_dependents: HashSet<&str> = graph
        .dependencies
        .values()
        .flat_map(|deps| deps.iter().map(|d| d.provider_id.as_str()))
        .collect();
    for svc in &graph.services {
        let is_provider = has_dependents.contains(svc.id.as_str());
        let is_consumer = graph.dependencies.contains_key(&svc.id);
        if !is_provider && !is_consumer && graph.services.len() > 1 {
            violations.push(UniverseViolation {
                service_id: svc.id.clone(),
                severity: "warning",
                message: format!(
                    "service '{}' is isolated — no upstream or downstream dependencies",
                    svc.id
                ),
            });
        }
    }

    violations
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    // ---- parse_universe_manifest (in-memory via temp files) ----

    #[test]
    fn test_parse_universe_manifest() {
        let dir = std::env::temp_dir().join("forge_universe_parse");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let manifest = dir.join("universe.intent.lisp");
        std::fs::write(
            &manifest,
            r#"(universe xiaojinpro
  (service auth :path "./auth-service")
  (service router :path "./router-gateway")
  (service billing :path "./billing-service"))"#,
        )
        .unwrap();

        let entries = parse_universe_manifest(&manifest).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].id, "auth");
        assert_eq!(entries[0].path, PathBuf::from("./auth-service"));
        assert_eq!(entries[1].id, "router");
        assert_eq!(entries[2].id, "billing");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_parse_universe_manifest_monorepo() {
        let dir = std::env::temp_dir().join("forge_universe_monorepo");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let manifest = dir.join("universe.intent.lisp");
        std::fs::write(
            &manifest,
            r#"(universe xiaojinpro
  (service forge :path "./jarvis-forge")
  (monorepo backend :path "./xiaojinpro-backend"
    (service router :subpath "services/router")
    (service billing :subpath "crates/xjp-billing")))"#,
        )
        .unwrap();

        let entries = parse_universe_manifest(&manifest).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].id, "forge");
        assert_eq!(entries[0].path, PathBuf::from("./jarvis-forge"));
        assert_eq!(entries[1].id, "router");
        assert_eq!(
            entries[1].path,
            PathBuf::from("./xiaojinpro-backend/services/router")
        );
        assert_eq!(entries[2].id, "billing");
        assert_eq!(
            entries[2].path,
            PathBuf::from("./xiaojinpro-backend/crates/xjp-billing")
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_parse_universe_manifest_missing_path() {
        let dir = std::env::temp_dir().join("forge_universe_nopath");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let manifest = dir.join("universe.intent.lisp");
        std::fs::write(&manifest, r#"(universe bad (service oops))"#).unwrap();

        let err = parse_universe_manifest(&manifest).unwrap_err();
        assert!(err.contains("missing :path"), "got: {err}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_parse_universe_manifest_empty() {
        let dir = std::env::temp_dir().join("forge_universe_empty");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let manifest = dir.join("universe.intent.lisp");
        std::fs::write(&manifest, r#"(universe empty)"#).unwrap();

        let err = parse_universe_manifest(&manifest).unwrap_err();
        assert!(err.contains("no service"), "got: {err}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    // ---- extract_upstream_dependencies ----

    #[test]
    fn test_extract_upstream_dependencies() {
        let content = r#"(intent router
  (granularity L2-Topology)
  (upstream-dependency
    (service "auth"
      :consumes ("UserProfile" "AuthToken")
      :breaks-if ("type-signature-change" "api-removal"))))"#;

        let deps = extract_upstream_dependencies(content, Path::new("router/intent.lisp"));
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].provider_id, "auth");
        assert_eq!(deps[0].consumes, vec!["UserProfile", "AuthToken"]);
        assert_eq!(
            deps[0].breaks_if,
            vec!["type-signature-change", "api-removal"]
        );
        assert_eq!(deps[0].source_file, PathBuf::from("router/intent.lisp"));
    }

    #[test]
    fn test_extract_multiple_upstream_dependencies() {
        let content = r#"(intent billing
  (upstream-dependency
    (service "auth"
      :consumes ("UserProfile")
      :breaks-if ("type-signature-change"))
    (service "router"
      :consumes ("RouteTable")
      :breaks-if ("api-removal"))))"#;

        let deps = extract_upstream_dependencies(content, Path::new("billing/intent.lisp"));
        assert_eq!(deps.len(), 2);
        assert_eq!(deps[0].provider_id, "auth");
        assert_eq!(deps[1].provider_id, "router");
        assert_eq!(deps[1].consumes, vec!["RouteTable"]);
    }

    #[test]
    fn test_extract_no_upstream_dependencies() {
        let content = r#"(intent auth (granularity L2-Topology))"#;
        let deps = extract_upstream_dependencies(content, Path::new("auth/intent.lisp"));
        assert!(deps.is_empty());
    }

    // ---- topological sort / chain dependency A→B→C ----

    #[test]
    fn test_chain_dependency_topo_order() {
        // C provides to B, B provides to A  (A → B → C in dependency direction)
        let graph = UniverseGraph {
            services: vec![
                ServiceEntry {
                    id: "C".into(),
                    path: PathBuf::from("./c"),
                },
                ServiceEntry {
                    id: "B".into(),
                    path: PathBuf::from("./b"),
                },
                ServiceEntry {
                    id: "A".into(),
                    path: PathBuf::from("./a"),
                },
            ],
            dependencies: {
                let mut m = HashMap::new();
                // B depends on C
                m.insert(
                    "B".into(),
                    vec![ServiceDependency {
                        provider_id: "C".into(),
                        consumes: vec!["TypeX".into()],
                        breaks_if: vec!["type-signature-change".into()],
                        source_file: PathBuf::from("./b/intent.lisp"),
                    }],
                );
                // A depends on B
                m.insert(
                    "A".into(),
                    vec![ServiceDependency {
                        provider_id: "B".into(),
                        consumes: vec!["TypeY".into()],
                        breaks_if: vec!["api-removal".into()],
                        source_file: PathBuf::from("./a/intent.lisp"),
                    }],
                );
                m
            },
        };

        // Changing C with type-signature-change should impact B (depth 1), then A (depth 2)
        let delta = ServiceDelta {
            service_id: "C".into(),
            changed_interfaces: vec!["type-signature-change".into()],
        };

        let impacted = compute_blast_radius(&graph, &delta);
        assert_eq!(impacted.len(), 2);
        assert_eq!(impacted[0].service_id, "B");
        assert_eq!(impacted[0].depth, 1);
        assert_eq!(impacted[1].service_id, "A");
        assert_eq!(impacted[1].depth, 2);
    }

    // ---- blast radius: unrelated interfaces are not impacted ----

    #[test]
    fn test_blast_radius_unrelated_interface() {
        let graph = UniverseGraph {
            services: vec![
                ServiceEntry {
                    id: "provider".into(),
                    path: PathBuf::from("./provider"),
                },
                ServiceEntry {
                    id: "consumer".into(),
                    path: PathBuf::from("./consumer"),
                },
            ],
            dependencies: {
                let mut m = HashMap::new();
                m.insert(
                    "consumer".into(),
                    vec![ServiceDependency {
                        provider_id: "provider".into(),
                        consumes: vec!["FooType".into()],
                        breaks_if: vec!["type-signature-change".into()],
                        source_file: PathBuf::from("./consumer/intent.lisp"),
                    }],
                );
                m
            },
        };

        // Change BarType — consumer only uses FooType, so no impact
        let delta = ServiceDelta {
            service_id: "provider".into(),
            changed_interfaces: vec!["BarType".into()],
        };

        let impacted = compute_blast_radius(&graph, &delta);
        assert!(impacted.is_empty());
    }

    // ---- cycle detection ----

    #[test]
    fn test_has_cycle_no_cycle() {
        let mut edges = HashSet::new();
        edges.insert((0, 1));
        edges.insert((1, 2));
        assert!(!has_cycle(3, &edges));
    }

    #[test]
    fn test_has_cycle_with_cycle() {
        let mut edges = HashSet::new();
        edges.insert((0, 1));
        edges.insert((1, 2));
        edges.insert((2, 0));
        assert!(has_cycle(3, &edges));
    }

    #[test]
    fn test_has_cycle_self_loop() {
        let mut edges = HashSet::new();
        edges.insert((0, 0));
        assert!(has_cycle(1, &edges));
    }

    // ---- full resolve_universe_graph (filesystem round-trip) ----

    #[test]
    fn test_resolve_universe_graph() {
        let dir = std::env::temp_dir().join("forge_universe_resolve");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("auth-service")).unwrap();
        std::fs::create_dir_all(dir.join("router-gateway")).unwrap();
        std::fs::create_dir_all(dir.join("billing-service")).unwrap();

        // Universe manifest
        std::fs::write(
            dir.join("universe.intent.lisp"),
            r#"(universe xiaojinpro
  (service auth :path "./auth-service")
  (service router :path "./router-gateway")
  (service billing :path "./billing-service"))"#,
        )
        .unwrap();

        // auth — no upstream deps (leaf provider)
        std::fs::write(
            dir.join("auth-service/intent.lisp"),
            r#"(intent auth (granularity L2-Topology))"#,
        )
        .unwrap();

        // router depends on auth
        std::fs::write(
            dir.join("router-gateway/intent.lisp"),
            r#"(intent router
  (granularity L2-Topology)
  (upstream-dependency
    (service "auth"
      :consumes ("UserProfile" "AuthToken")
      :breaks-if ("type-signature-change" "api-removal"))))"#,
        )
        .unwrap();

        // billing depends on auth
        std::fs::write(
            dir.join("billing-service/intent.lisp"),
            r#"(intent billing
  (granularity L2-Topology)
  (upstream-dependency
    (service "auth"
      :consumes ("UserProfile")
      :breaks-if ("type-signature-change"))))"#,
        )
        .unwrap();

        let graph =
            resolve_universe_graph(&dir.join("universe.intent.lisp")).unwrap();
        assert_eq!(graph.services.len(), 3);
        assert_eq!(graph.dependencies.len(), 2); // router + billing

        // Blast radius: auth had a type-signature-change
        let delta = ServiceDelta {
            service_id: "auth".into(),
            changed_interfaces: vec!["type-signature-change".into()],
        };
        let impacted = compute_blast_radius(&graph, &delta);
        assert_eq!(impacted.len(), 2);
        let ids: Vec<&str> = impacted.iter().map(|i| i.service_id.as_str()).collect();
        assert!(ids.contains(&"router"));
        assert!(ids.contains(&"billing"));
        // Both should be depth 1 (direct consumers)
        assert!(impacted.iter().all(|i| i.depth == 1));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_resolve_universe_graph_unknown_provider() {
        let dir = std::env::temp_dir().join("forge_universe_unknown");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("svc-a")).unwrap();

        std::fs::write(
            dir.join("universe.intent.lisp"),
            r#"(universe test (service svc-a :path "./svc-a"))"#,
        )
        .unwrap();

        // svc-a depends on a service that is NOT in the universe
        std::fs::write(
            dir.join("svc-a/intent.lisp"),
            r#"(intent svc-a
  (upstream-dependency
    (service "ghost-service"
      :consumes ("Phantom")
      :breaks-if ("removal"))))"#,
        )
        .unwrap();

        let err = resolve_universe_graph(&dir.join("universe.intent.lisp")).unwrap_err();
        assert!(err.iter().any(|e| e.contains("unknown service")));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_resolve_universe_graph_cycle_detection() {
        let dir = std::env::temp_dir().join("forge_universe_cycle");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("svc-a")).unwrap();
        std::fs::create_dir_all(dir.join("svc-b")).unwrap();

        std::fs::write(
            dir.join("universe.intent.lisp"),
            r#"(universe test
  (service svc-a :path "./svc-a")
  (service svc-b :path "./svc-b"))"#,
        )
        .unwrap();

        // svc-a depends on svc-b AND svc-b depends on svc-a → cycle
        std::fs::write(
            dir.join("svc-a/intent.lisp"),
            r#"(intent svc-a
  (upstream-dependency
    (service "svc-b" :consumes ("X") :breaks-if ("change"))))"#,
        )
        .unwrap();

        std::fs::write(
            dir.join("svc-b/intent.lisp"),
            r#"(intent svc-b
  (upstream-dependency
    (service "svc-a" :consumes ("Y") :breaks-if ("change"))))"#,
        )
        .unwrap();

        let err = resolve_universe_graph(&dir.join("universe.intent.lisp")).unwrap_err();
        assert!(
            err.iter().any(|e| e.contains("cycle")),
            "expected cycle error, got: {:?}",
            err
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
