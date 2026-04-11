//! Pinia Store IR — typed representation of `(pinia-store ...)` declarations.
//!
//! A Pinia store declaration captures state fields, computed getters, and
//! actions needed to generate a complete Pinia `defineStore(...)` module.
//!
//! Generates: TypeScript module with `defineStore(...)` call, typed state,
//! getters, and actions.

use nc_parser::SexpNode;

// ---------------------------------------------------------------------------
// Declaration types
// ---------------------------------------------------------------------------

/// Top-level Pinia store declaration.
///
/// Parsed from:
/// ```lisp
/// (pinia-store executionStore
///   :target-path "src/stores/executionStore.ts"
///   (state
///     (activeJobId "string | null" :default "null")
///     (nodeProgressStates "Record<string, NodeProgressState>" :default "{}")
///     (isExecuting boolean :default false))
///   (getters
///     (hasActiveJob boolean :derives-from (activeJobId))
///     (progressPercent number :derives-from (nodeProgressStates)))
///   (actions
///     (startExecution :params ((jobId string)) :returns void)
///     (updateProgress :params ((nodeId string) (value number)) :returns void)
///     (clearExecution :returns void)))
/// ```
#[derive(Debug, Clone)]
pub struct PiniaStoreDecl {
    pub name: String,
    pub target_path: Option<String>,
    pub state: Vec<StoreField>,
    pub getters: Vec<StoreGetter>,
    pub actions: Vec<StoreAction>,
}

/// A reactive state field in the Pinia store.
#[derive(Debug, Clone)]
pub struct StoreField {
    pub name: String,
    /// TypeScript type string, e.g. "string | null", "boolean".
    pub ts_type: String,
    pub default_value: Option<String>,
}

/// A computed getter on the Pinia store.
#[derive(Debug, Clone)]
pub struct StoreGetter {
    pub name: String,
    /// Return type, e.g. "boolean", "number".
    pub return_type: String,
    /// State fields this getter derives from.
    pub derives_from: Vec<String>,
}

/// An action method on the Pinia store.
#[derive(Debug, Clone)]
pub struct StoreAction {
    pub name: String,
    pub params: Vec<ActionParam>,
    /// Return type, e.g. "void", "Promise<void>".
    pub return_type: Option<String>,
}

/// A parameter for a Pinia action.
#[derive(Debug, Clone)]
pub struct ActionParam {
    pub name: String,
    pub ts_type: String,
}

// ---------------------------------------------------------------------------
// S-EXP -> IR lowering
// ---------------------------------------------------------------------------

impl PiniaStoreDecl {
    /// Parse a single `(pinia-store ...)` S-expression into IR.
    pub fn from_sexp(node: &SexpNode) -> Result<Self, String> {
        let children = expect_list(node)?;
        expect_tag(node, "pinia-store")?;

        if children.len() < 2 {
            return Err("pinia-store: missing store name".into());
        }
        let name = expect_atom(&children[1])?.to_string();

        let target_path = node.keyword(":target-path").map(String::from);

        let state = match node.find("state") {
            Some(state_node) => parse_state(state_node, &name)?,
            None => vec![],
        };

        let getters = match node.find("getters") {
            Some(getters_node) => parse_getters(getters_node, &name)?,
            None => vec![],
        };

        let actions = match node.find("actions") {
            Some(actions_node) => parse_actions(actions_node, &name)?,
            None => vec![],
        };

        Ok(PiniaStoreDecl {
            name,
            target_path,
            state,
            getters,
            actions,
        })
    }
}

/// Parse all `(pinia-store ...)` declarations from a list of top-level nodes.
pub fn parse_pinia_stores(nodes: &[SexpNode]) -> Result<Vec<PiniaStoreDecl>, String> {
    nodes
        .iter()
        .filter(|n| n.tag() == Some("pinia-store"))
        .map(PiniaStoreDecl::from_sexp)
        .collect()
}

// ---------------------------------------------------------------------------
// Sub-parsers
// ---------------------------------------------------------------------------

fn parse_state(node: &SexpNode, store_name: &str) -> Result<Vec<StoreField>, String> {
    let children = expect_list(node)?;
    let mut fields = Vec::new();

    for child in children.iter().skip(1) {
        // skip "state" tag
        if let SexpNode::List(parts) = child {
            if parts.len() < 2 {
                continue;
            }
            let field_name = expect_atom(&parts[0])
                .map_err(|e| format!("pinia-store `{store_name}` state: {e}"))?
                .to_string();
            if field_name.starts_with(':') {
                continue;
            }
            let ts_type = expect_atom(&parts[1])
                .map_err(|e| {
                    format!("pinia-store `{store_name}` state `{field_name}`: {e}")
                })?
                .to_string();

            let default_value = child.keyword(":default").map(String::from);

            fields.push(StoreField {
                name: field_name,
                ts_type,
                default_value,
            });
        }
    }
    Ok(fields)
}

fn parse_getters(node: &SexpNode, store_name: &str) -> Result<Vec<StoreGetter>, String> {
    let children = expect_list(node)?;
    let mut getters = Vec::new();

    for child in children.iter().skip(1) {
        // skip "getters" tag
        if let SexpNode::List(parts) = child {
            if parts.len() < 2 {
                continue;
            }
            let getter_name = expect_atom(&parts[0])
                .map_err(|e| format!("pinia-store `{store_name}` getter: {e}"))?
                .to_string();
            if getter_name.starts_with(':') {
                continue;
            }
            let return_type = expect_atom(&parts[1])
                .map_err(|e| {
                    format!("pinia-store `{store_name}` getter `{getter_name}`: {e}")
                })?
                .to_string();

            let derives_from = parse_derives_from(child);

            getters.push(StoreGetter {
                name: getter_name,
                return_type,
                derives_from,
            });
        }
    }
    Ok(getters)
}

/// Extract atoms from `:derives-from (field1 field2)`.
fn parse_derives_from(node: &SexpNode) -> Vec<String> {
    match node.keyword_node(":derives-from") {
        Some(SexpNode::List(items)) => items
            .iter()
            .filter_map(|item| match item {
                SexpNode::Atom(s) => Some(s.clone()),
                _ => None,
            })
            .collect(),
        _ => vec![],
    }
}

fn parse_actions(node: &SexpNode, store_name: &str) -> Result<Vec<StoreAction>, String> {
    let children = expect_list(node)?;
    let mut actions = Vec::new();

    for child in children.iter().skip(1) {
        // skip "actions" tag
        if let SexpNode::List(parts) = child {
            if parts.is_empty() {
                continue;
            }
            let action_name = expect_atom(&parts[0])
                .map_err(|e| format!("pinia-store `{store_name}` action: {e}"))?
                .to_string();
            if action_name.starts_with(':') {
                continue;
            }

            let params = parse_action_params(child, store_name, &action_name)?;
            let return_type = child.keyword(":returns").map(String::from);

            actions.push(StoreAction {
                name: action_name,
                params,
                return_type,
            });
        }
    }
    Ok(actions)
}

/// Extract params from `:params ((name type) (name type))`.
fn parse_action_params(
    node: &SexpNode,
    store_name: &str,
    action_name: &str,
) -> Result<Vec<ActionParam>, String> {
    let params_node = match node.keyword_node(":params") {
        Some(n) => n,
        None => return Ok(vec![]),
    };
    let items = expect_list(params_node)?;
    let mut params = Vec::new();

    for item in items {
        if let SexpNode::List(pair) = item {
            if pair.len() < 2 {
                continue;
            }
            let param_name = expect_atom(&pair[0])
                .map_err(|e| {
                    format!("pinia-store `{store_name}` action `{action_name}` param: {e}")
                })?
                .to_string();
            let ts_type = expect_atom(&pair[1])
                .map_err(|e| {
                    format!(
                        "pinia-store `{store_name}` action `{action_name}` param `{param_name}`: {e}"
                    )
                })?
                .to_string();

            params.push(ActionParam {
                name: param_name,
                ts_type,
            });
        }
    }
    Ok(params)
}

// ---------------------------------------------------------------------------
// Utility helpers
// ---------------------------------------------------------------------------

fn expect_tag<'a>(node: &'a SexpNode, expected: &str) -> Result<&'a str, String> {
    let tag = node
        .tag()
        .ok_or_else(|| format!("expected list with tag `{expected}`, got atom"))?;
    if tag != expected {
        return Err(format!("expected tag `{expected}`, got `{tag}`"));
    }
    Ok(tag)
}

fn expect_list(node: &SexpNode) -> Result<&Vec<SexpNode>, String> {
    match node {
        SexpNode::List(c) => Ok(c),
        SexpNode::Atom(s) => Err(format!("expected list, got atom `{s}`")),
    }
}

fn expect_atom<'a>(node: &'a SexpNode) -> Result<&'a str, String> {
    match node {
        SexpNode::Atom(s) => Ok(s),
        SexpNode::List(_) => Err("expected atom, got list".into()),
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
    fn parse_full_pinia_store() {
        let sexp = parse(
            r#"(pinia-store executionStore
              :target-path "src/stores/executionStore.ts"
              (state
                (activeJobId "string | null" :default "null")
                (nodeProgressStates "Record<string, NodeProgressState>" :default "{}")
                (isExecuting boolean :default false))
              (getters
                (hasActiveJob boolean :derives-from (activeJobId))
                (progressPercent number :derives-from (nodeProgressStates)))
              (actions
                (startExecution :params ((jobId string)) :returns void)
                (updateProgress :params ((nodeId string) (value number)) :returns void)
                (clearExecution :returns void)))"#,
        );
        let store = PiniaStoreDecl::from_sexp(&sexp).unwrap();

        assert_eq!(store.name, "executionStore");
        assert_eq!(
            store.target_path,
            Some("src/stores/executionStore.ts".into())
        );

        // state
        assert_eq!(store.state.len(), 3);
        assert_eq!(store.state[0].name, "activeJobId");
        assert_eq!(store.state[0].ts_type, "string | null");
        assert_eq!(store.state[0].default_value, Some("null".into()));

        assert_eq!(store.state[1].name, "nodeProgressStates");
        assert_eq!(store.state[1].ts_type, "Record<string, NodeProgressState>");
        assert_eq!(store.state[1].default_value, Some("{}".into()));

        assert_eq!(store.state[2].name, "isExecuting");
        assert_eq!(store.state[2].ts_type, "boolean");
        assert_eq!(store.state[2].default_value, Some("false".into()));

        // getters
        assert_eq!(store.getters.len(), 2);
        assert_eq!(store.getters[0].name, "hasActiveJob");
        assert_eq!(store.getters[0].return_type, "boolean");
        assert_eq!(store.getters[0].derives_from, vec!["activeJobId"]);

        assert_eq!(store.getters[1].name, "progressPercent");
        assert_eq!(store.getters[1].return_type, "number");
        assert_eq!(
            store.getters[1].derives_from,
            vec!["nodeProgressStates"]
        );

        // actions
        assert_eq!(store.actions.len(), 3);

        assert_eq!(store.actions[0].name, "startExecution");
        assert_eq!(store.actions[0].params.len(), 1);
        assert_eq!(store.actions[0].params[0].name, "jobId");
        assert_eq!(store.actions[0].params[0].ts_type, "string");
        assert_eq!(store.actions[0].return_type, Some("void".into()));

        assert_eq!(store.actions[1].name, "updateProgress");
        assert_eq!(store.actions[1].params.len(), 2);
        assert_eq!(store.actions[1].params[0].name, "nodeId");
        assert_eq!(store.actions[1].params[1].name, "value");
        assert_eq!(store.actions[1].params[1].ts_type, "number");

        assert_eq!(store.actions[2].name, "clearExecution");
        assert!(store.actions[2].params.is_empty());
        assert_eq!(store.actions[2].return_type, Some("void".into()));
    }

    #[test]
    fn parse_minimal_pinia_store() {
        let sexp = parse(r#"(pinia-store emptyStore)"#);
        let store = PiniaStoreDecl::from_sexp(&sexp).unwrap();

        assert_eq!(store.name, "emptyStore");
        assert_eq!(store.target_path, None);
        assert!(store.state.is_empty());
        assert!(store.getters.is_empty());
        assert!(store.actions.is_empty());
    }

    #[test]
    fn parse_state_only() {
        let sexp = parse(
            r#"(pinia-store counterStore
              :target-path "src/stores/counter.ts"
              (state
                (count number :default 0)
                (label string)))"#,
        );
        let store = PiniaStoreDecl::from_sexp(&sexp).unwrap();

        assert_eq!(store.state.len(), 2);
        assert_eq!(store.state[0].name, "count");
        assert_eq!(store.state[0].default_value, Some("0".into()));
        assert_eq!(store.state[1].name, "label");
        assert_eq!(store.state[1].ts_type, "string");
        assert_eq!(store.state[1].default_value, None);
        assert!(store.getters.is_empty());
        assert!(store.actions.is_empty());
    }

    #[test]
    fn parse_actions_without_params() {
        let sexp = parse(
            r#"(pinia-store uiStore
              (actions
                (reset :returns void)
                (toggle :returns boolean)))"#,
        );
        let store = PiniaStoreDecl::from_sexp(&sexp).unwrap();

        assert_eq!(store.actions.len(), 2);
        assert!(store.actions[0].params.is_empty());
        assert_eq!(store.actions[0].return_type, Some("void".into()));
        assert_eq!(store.actions[1].return_type, Some("boolean".into()));
    }

    #[test]
    fn parse_multiple_pinia_stores() {
        let input = r#"
          (pinia-store storeA
            (state
              (x number :default 0)))

          (pinia-store storeB
            (actions
              (doSomething :returns void)))

          ; non-pinia-store should be ignored
          (vue-component Sidebar)
        "#;
        let nodes = SexpNode::parse(input).unwrap();
        let stores = parse_pinia_stores(&nodes).unwrap();

        assert_eq!(stores.len(), 2);
        assert_eq!(stores[0].name, "storeA");
        assert_eq!(stores[1].name, "storeB");
    }
}
