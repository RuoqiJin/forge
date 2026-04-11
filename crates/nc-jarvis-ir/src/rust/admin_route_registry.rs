//! Admin Route Registry IR — typed representation of `(admin-route-registry ...)` declarations.
//!
//! Declares the admin API route registration layer:
//! - Route groups map 1:1 to `register_*_route()` functions
//! - Each route maps to an `r.insert(Method, path, AdminOperation(&Handler{}))` call
//! - Parent groups delegate to sub-groups (e.g., `user` calls `account_info`, `user_lifecycle`, etc.)
//!
//! Forge generates:
//!   - Handler struct definitions
//!   - Route registration functions
//!   - Coordinator function (`make_admin_route`)

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
            "AdminRouteRegistry IR error in `{}`: {} (expected {:?}, got {:?})",
            self.field, self.message, self.expected, self.got
        )
    }
}

impl std::error::Error for IrError {}

// ---------------------------------------------------------------------------
// Declaration types
// ---------------------------------------------------------------------------

/// HTTP method enum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Delete,
    Head,
}

impl HttpMethod {
    pub fn from_str(s: &str) -> Result<Self, IrError> {
        match s {
            "GET" => Ok(Self::Get),
            "POST" => Ok(Self::Post),
            "PUT" => Ok(Self::Put),
            "DELETE" => Ok(Self::Delete),
            "HEAD" => Ok(Self::Head),
            other => Err(IrError {
                field: "method".into(),
                message: "unknown HTTP method".into(),
                expected: vec![
                    "GET".into(),
                    "POST".into(),
                    "PUT".into(),
                    "DELETE".into(),
                    "HEAD".into(),
                ],
                got: other.into(),
            }),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Delete => "DELETE",
            Self::Head => "HEAD",
        }
    }
}

/// A single route declaration.
#[derive(Debug, Clone)]
pub struct RouteDecl {
    pub handler: String,
    pub method: HttpMethod,
    pub path: String,
}

/// A route group — maps to a `register_*_route()` function.
#[derive(Debug, Clone)]
pub struct RouteGroupDecl {
    pub name: String,
    pub register_fn: String,
    pub module_path: String,
    pub routes: Vec<RouteDecl>,
    pub sub_groups: Vec<String>,
}

/// Top-level admin-route-registry declaration.
#[derive(Debug, Clone)]
pub struct AdminRouteRegistryDecl {
    pub name: String,
    pub coordinator: String,
    pub router_type: String,
    pub groups: Vec<RouteGroupDecl>,
}

impl AdminRouteRegistryDecl {
    /// Count total routes across all groups.
    pub fn total_routes(&self) -> usize {
        self.groups.iter().map(|g| g.routes.len()).sum()
    }

    /// Collect all unique handler struct names.
    pub fn unique_handlers(&self) -> Vec<String> {
        let mut seen = std::collections::HashSet::new();
        let mut result = Vec::new();
        for group in &self.groups {
            for route in &group.routes {
                if seen.insert(route.handler.clone()) {
                    result.push(route.handler.clone());
                }
            }
        }
        result
    }

    /// Get groups that are top-level (called directly by the coordinator).
    /// These are groups NOT referenced as sub-groups by any other group.
    pub fn top_level_groups(&self) -> Vec<&RouteGroupDecl> {
        let all_sub: std::collections::HashSet<&str> = self
            .groups
            .iter()
            .flat_map(|g| g.sub_groups.iter().map(|s| s.as_str()))
            .collect();
        self.groups
            .iter()
            .filter(|g| !all_sub.contains(g.name.as_str()))
            .collect()
    }
}

// ---------------------------------------------------------------------------
// S-EXP -> IR lowering
// ---------------------------------------------------------------------------

impl AdminRouteRegistryDecl {
    pub fn from_sexp(node: &SexpNode) -> Result<Self, IrError> {
        let children = expect_list(node)?;
        if children.len() < 2 {
            return Err(err(
                "admin-route-registry",
                "missing registry name",
                &["name"],
                "",
            ));
        }

        expect_tag(node, "admin-route-registry")?;
        let name = expect_atom(&children[1])?.to_string();
        let coordinator = keyword_required(node, ":coordinator")?;
        let router_type = keyword_required(node, ":router-type")?;

        let mut groups = Vec::new();
        for child in children.iter().skip(2) {
            if child.tag() == Some("route-group") {
                groups.push(RouteGroupDecl::from_sexp(child)?);
            }
        }

        Ok(AdminRouteRegistryDecl {
            name,
            coordinator,
            router_type,
            groups,
        })
    }
}

impl RouteGroupDecl {
    fn from_sexp(node: &SexpNode) -> Result<Self, IrError> {
        let children = expect_list(node)?;
        if children.len() < 2 {
            return Err(err(
                "route-group",
                "missing group name",
                &["group_name"],
                "",
            ));
        }

        let name = expect_atom(&children[1])?.to_string();
        let register_fn = keyword_required(node, ":register")?;
        let module_path = keyword_required(node, ":module")?;

        // Parse sub-groups if present
        let sub_groups = parse_sub_groups(node);

        // Parse routes
        let mut routes = Vec::new();
        for child in children.iter().skip(2) {
            if child.tag() == Some("route") {
                routes.push(RouteDecl::from_sexp(child)?);
            }
        }

        Ok(RouteGroupDecl {
            name,
            register_fn,
            module_path,
            routes,
            sub_groups,
        })
    }
}

impl RouteDecl {
    fn from_sexp(node: &SexpNode) -> Result<Self, IrError> {
        let children = expect_list(node)?;
        if children.len() < 4 {
            return Err(err(
                "route",
                "expected (route Handler METHOD \"/path\")",
                &["handler", "method", "path"],
                &format!("{} elements", children.len()),
            ));
        }

        let handler = expect_atom(&children[1])?.to_string();
        let method_str = expect_atom(&children[2])?;
        let method = HttpMethod::from_str(method_str)?;
        let path = expect_atom(&children[3])?.to_string();

        Ok(RouteDecl {
            handler,
            method,
            path,
        })
    }
}

/// Parse all `(admin-route-registry ...)` declarations from a top-level S-EXP list.
pub fn parse_admin_route_registries(
    nodes: &[SexpNode],
) -> Result<Vec<AdminRouteRegistryDecl>, IrError> {
    nodes
        .iter()
        .filter(|n| n.tag() == Some("admin-route-registry"))
        .map(AdminRouteRegistryDecl::from_sexp)
        .collect()
}

// ---------------------------------------------------------------------------
// Sub-parsers
// ---------------------------------------------------------------------------

fn parse_sub_groups(parent: &SexpNode) -> Vec<String> {
    let node = match parent.keyword_node(":sub-groups") {
        Some(n) => n,
        None => return vec![],
    };
    match node {
        SexpNode::List(items) => items
            .iter()
            .filter_map(|item| {
                if let SexpNode::Atom(s) = item {
                    Some(s.clone())
                } else {
                    None
                }
            })
            .collect(),
        _ => vec![],
    }
}

// ---------------------------------------------------------------------------
// S-EXP utility helpers
// ---------------------------------------------------------------------------

fn expect_tag<'a>(node: &'a SexpNode, expected: &str) -> Result<&'a str, IrError> {
    let tag = node
        .tag()
        .ok_or_else(|| err("node", "expected a list", &[expected], "atom"))?;
    if tag != expected {
        return Err(err("tag", "wrong tag", &[expected], tag));
    }
    Ok(tag)
}

fn expect_list(node: &SexpNode) -> Result<&Vec<SexpNode>, IrError> {
    match node {
        SexpNode::List(children) => Ok(children),
        SexpNode::Atom(s) => Err(err("node", "expected list", &["(...)"], s)),
    }
}

fn expect_atom<'a>(node: &'a SexpNode) -> Result<&'a str, IrError> {
    match node {
        SexpNode::Atom(s) => Ok(s),
        SexpNode::List(_) => Err(err("node", "expected atom", &["atom"], "(...)")),
    }
}

fn keyword_required(node: &SexpNode, key: &str) -> Result<String, IrError> {
    node.keyword(key)
        .map(String::from)
        .ok_or_else(|| err(key, &format!("missing required {key}"), &["value"], ""))
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
    fn parse_minimal_registry() {
        let sexp = parse(
            r#"(admin-route-registry admin
              :coordinator make_admin_route
              :router-type "S3Router<AdminOperation>"

              (route-group health
                :register register_health_route
                :module "handlers::health"
                (route HealthCheckHandler GET "/health")
                (route HealthCheckHandler HEAD "/health")))"#,
        );
        let decl = AdminRouteRegistryDecl::from_sexp(&sexp).unwrap();

        assert_eq!(decl.name, "admin");
        assert_eq!(decl.coordinator, "make_admin_route");
        assert_eq!(decl.router_type, "S3Router<AdminOperation>");
        assert_eq!(decl.groups.len(), 1);

        let g = &decl.groups[0];
        assert_eq!(g.name, "health");
        assert_eq!(g.register_fn, "register_health_route");
        assert_eq!(g.module_path, "handlers::health");
        assert_eq!(g.routes.len(), 2);

        assert_eq!(g.routes[0].handler, "HealthCheckHandler");
        assert_eq!(g.routes[0].method, HttpMethod::Get);
        assert_eq!(g.routes[0].path, "/health");

        assert_eq!(g.routes[1].handler, "HealthCheckHandler");
        assert_eq!(g.routes[1].method, HttpMethod::Head);
        assert_eq!(g.routes[1].path, "/health");
    }

    #[test]
    fn parse_group_with_sub_groups() {
        let sexp = parse(
            r#"(admin-route-registry test
              :coordinator make_route
              :router-type "Router"

              (route-group parent
                :register register_parent
                :module "handlers::parent"
                :sub-groups (child_a child_b))

              (route-group child_a
                :register register_child_a
                :module "handlers::child_a"
                (route HandlerA GET "/path/a"))

              (route-group child_b
                :register register_child_b
                :module "handlers::child_b"
                (route HandlerB POST "/path/b")))"#,
        );
        let decl = AdminRouteRegistryDecl::from_sexp(&sexp).unwrap();

        assert_eq!(decl.groups.len(), 3);

        let parent = &decl.groups[0];
        assert_eq!(parent.name, "parent");
        assert_eq!(parent.sub_groups, vec!["child_a", "child_b"]);
        assert_eq!(parent.routes.len(), 0); // parent has no direct routes

        let child_a = &decl.groups[1];
        assert_eq!(child_a.name, "child_a");
        assert_eq!(child_a.routes.len(), 1);

        // Test top_level_groups
        let top = decl.top_level_groups();
        assert_eq!(top.len(), 1);
        assert_eq!(top[0].name, "parent");
    }

    #[test]
    fn reject_unknown_method() {
        let sexp = parse(
            r#"(admin-route-registry bad
              :coordinator make_route
              :router-type "Router"
              (route-group test
                :register register_test
                :module "handlers::test"
                (route Handler PATCH "/path")))"#,
        );
        let e = AdminRouteRegistryDecl::from_sexp(&sexp).unwrap_err();
        assert_eq!(e.field, "method");
        assert!(e.got.contains("PATCH"));
    }

    #[test]
    fn reject_missing_coordinator() {
        let sexp = parse(
            r#"(admin-route-registry bad
              :router-type "Router"
              (route-group test
                :register register_test
                :module "handlers::test"))"#,
        );
        let e = AdminRouteRegistryDecl::from_sexp(&sexp).unwrap_err();
        assert_eq!(e.field, ":coordinator");
        assert!(e.message.contains("missing required"));
    }

    #[test]
    fn total_routes_and_unique_handlers() {
        let sexp = parse(
            r#"(admin-route-registry test
              :coordinator make_route
              :router-type "Router"

              (route-group alpha
                :register register_alpha
                :module "handlers::alpha"
                (route HandlerA GET "/a")
                (route HandlerA HEAD "/a")
                (route HandlerB POST "/b"))

              (route-group beta
                :register register_beta
                :module "handlers::beta"
                (route HandlerB PUT "/c")
                (route HandlerC DELETE "/d")))"#,
        );
        let decl = AdminRouteRegistryDecl::from_sexp(&sexp).unwrap();

        assert_eq!(decl.total_routes(), 5);

        let handlers = decl.unique_handlers();
        assert_eq!(handlers.len(), 3);
        assert_eq!(handlers, vec!["HandlerA", "HandlerB", "HandlerC"]);
    }

    #[test]
    fn parse_real_lisp_file() {
        let lisp_path = "fixtures/xjpfs/.missiond/intent-admin-routes.lisp";
        let lisp_src = match std::fs::read_to_string(lisp_path) {
            Ok(s) => s,
            Err(_) => return, // skip if file not available
        };
        let node = SexpNode::parse_one(&lisp_src).expect("Failed to parse S-EXP");
        let decl = AdminRouteRegistryDecl::from_sexp(&node).expect("Failed to lower to IR");

        assert_eq!(decl.name, "admin");
        assert_eq!(decl.coordinator, "make_admin_route");

        // Verify we have a substantial number of groups and routes
        assert!(
            decl.groups.len() >= 20,
            "expected >=20 groups, got {}",
            decl.groups.len()
        );
        assert!(
            decl.total_routes() >= 110,
            "expected >=110 routes, got {}",
            decl.total_routes()
        );

        // Verify specific groups exist
        let group_names: Vec<&str> = decl.groups.iter().map(|g| g.name.as_str()).collect();
        assert!(group_names.contains(&"health"));
        assert!(group_names.contains(&"sts"));
        assert!(group_names.contains(&"system"));
        assert!(group_names.contains(&"site_replication"));
        assert!(group_names.contains(&"kms_management"));
        assert!(group_names.contains(&"oidc"));

        // Verify unique handlers
        let handlers = decl.unique_handlers();
        assert!(
            handlers.len() >= 70,
            "expected >=70 unique handlers, got {}",
            handlers.len()
        );

        eprintln!(
            "=== IR VALIDATION PASS === groups={}, routes={}, handlers={}",
            decl.groups.len(),
            decl.total_routes(),
            handlers.len()
        );
    }

    #[test]
    fn http_method_as_str_roundtrip() {
        for method_str in &["GET", "POST", "PUT", "DELETE", "HEAD"] {
            let method = HttpMethod::from_str(method_str).unwrap();
            assert_eq!(method.as_str(), *method_str);
        }
    }

    #[test]
    fn parse_all_http_methods() {
        let sexp = parse(
            r#"(admin-route-registry methods
              :coordinator make
              :router-type "R"
              (route-group all
                :register register_all
                :module "handlers::all"
                (route H1 GET "/get")
                (route H2 POST "/post")
                (route H3 PUT "/put")
                (route H4 DELETE "/delete")
                (route H5 HEAD "/head")))"#,
        );
        let decl = AdminRouteRegistryDecl::from_sexp(&sexp).unwrap();
        let routes = &decl.groups[0].routes;

        assert_eq!(routes[0].method, HttpMethod::Get);
        assert_eq!(routes[1].method, HttpMethod::Post);
        assert_eq!(routes[2].method, HttpMethod::Put);
        assert_eq!(routes[3].method, HttpMethod::Delete);
        assert_eq!(routes[4].method, HttpMethod::Head);
    }
}
