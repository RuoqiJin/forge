//! Vue Component IR — typed representation of `(vue-component ...)` declarations.
//!
//! A Vue component declaration captures props, emits, and slots needed to
//! generate a Vue SFC `<script setup>` skeleton with `defineProps`,
//! `defineEmits`, and `defineSlots`.
//!
//! Generates: Vue SFC with typed props, defineEmits, slots, and withDefaults.

use nc_parser::SexpNode;

// ---------------------------------------------------------------------------
// Declaration types
// ---------------------------------------------------------------------------

/// Top-level Vue component declaration.
///
/// Parsed from:
/// ```lisp
/// (vue-component QueuePanel
///   :target-path "src/components/queue/QueuePanel.vue"
///   (props
///     (jobs array :required)
///     (activeJobId string)
///     (maxVisible number :default 50))
///   (emits
///     (job-selected :payload "{ jobId: string }")
///     (queue-cleared))
///   (slots
///     (header)
///     (empty-state :description "Shown when no jobs")))
/// ```
#[derive(Debug, Clone)]
pub struct VueComponentDecl {
    pub name: String,
    pub target_path: Option<String>,
    pub props: Vec<ComponentProp>,
    pub emits: Vec<ComponentEmit>,
    pub slots: Vec<ComponentSlot>,
    /// Store dependencies: (storeName, importPath)
    pub stores: Vec<StoreDep>,
    /// defineModel type, e.g. "string | undefined"
    pub model: Option<String>,
}

/// A Pinia store dependency used by the component.
#[derive(Debug, Clone)]
pub struct StoreDep {
    pub name: String,
    pub import_path: String,
}

/// A typed prop accepted by the Vue component.
#[derive(Debug, Clone)]
pub struct ComponentProp {
    pub name: String,
    /// TypeScript type string: "string", "number", "boolean", "array", "object", "function".
    pub ts_type: String,
    pub required: bool,
    pub default_value: Option<String>,
    pub description: Option<String>,
}

/// An event emitted by the Vue component.
#[derive(Debug, Clone)]
pub struct ComponentEmit {
    pub name: String,
    /// TypeScript type literal for the event payload.
    pub payload: Option<String>,
}

/// A named slot exposed by the Vue component.
#[derive(Debug, Clone)]
pub struct ComponentSlot {
    pub name: String,
    pub description: Option<String>,
}

// Prop types: any valid TypeScript type string is accepted.
// Union types ("string | number"), generics ("Record<string, T>"), etc. all pass through.

// ---------------------------------------------------------------------------
// S-EXP -> IR lowering
// ---------------------------------------------------------------------------

impl VueComponentDecl {
    /// Parse a single `(vue-component ...)` S-expression into IR.
    pub fn from_sexp(node: &SexpNode) -> Result<Self, String> {
        let children = expect_list(node)?;
        expect_tag(node, "vue-component")?;

        if children.len() < 2 {
            return Err("vue-component: missing component name".into());
        }
        let name = expect_atom(&children[1])?.to_string();

        let target_path = node.keyword(":target-path").map(String::from);

        let props = match node.find("props") {
            Some(props_node) => parse_props(props_node, &name)?,
            None => vec![],
        };

        let emits = match node.find("emits") {
            Some(emits_node) => parse_emits(emits_node, &name)?,
            None => vec![],
        };

        let slots = match node.find("slots") {
            Some(slots_node) => parse_slots(slots_node, &name)?,
            None => vec![],
        };

        let stores = match node.find("stores") {
            Some(stores_node) => parse_stores(stores_node)?,
            None => vec![],
        };

        let model = node.keyword(":model").map(String::from)
            .or_else(|| {
                node.find("model").and_then(|m| {
                    if let SexpNode::List(parts) = m {
                        parts.get(1).and_then(|n| match n {
                            SexpNode::Atom(s) => Some(s.clone()),
                            _ => None,
                        })
                    } else {
                        None
                    }
                })
            });

        Ok(VueComponentDecl {
            name,
            target_path,
            props,
            emits,
            slots,
            stores,
            model,
        })
    }
}

/// Parse all `(vue-component ...)` declarations from a list of top-level nodes.
pub fn parse_vue_components(nodes: &[SexpNode]) -> Result<Vec<VueComponentDecl>, String> {
    nodes
        .iter()
        .filter(|n| n.tag() == Some("vue-component"))
        .map(VueComponentDecl::from_sexp)
        .collect()
}

// ---------------------------------------------------------------------------
// Sub-parsers
// ---------------------------------------------------------------------------

fn parse_props(node: &SexpNode, comp_name: &str) -> Result<Vec<ComponentProp>, String> {
    let children = expect_list(node)?;
    let mut props = Vec::new();

    for child in children.iter().skip(1) {
        // skip "props" tag
        if let SexpNode::List(parts) = child {
            if parts.len() < 2 {
                continue;
            }
            let prop_name = expect_atom(&parts[0])
                .map_err(|e| format!("vue-component `{comp_name}` prop: {e}"))?
                .to_string();
            if prop_name.starts_with(':') {
                continue;
            }
            let ts_type = expect_atom(&parts[1])
                .map_err(|e| {
                    format!("vue-component `{comp_name}` prop `{prop_name}`: {e}")
                })?
                .to_string();

            let required = has_flag(child, ":required");
            let default_value = child.keyword(":default").map(String::from);
            let description = child.keyword(":description").map(String::from);

            props.push(ComponentProp {
                name: prop_name,
                ts_type,
                required,
                default_value,
                description,
            });
        }
    }
    Ok(props)
}

fn parse_emits(node: &SexpNode, comp_name: &str) -> Result<Vec<ComponentEmit>, String> {
    let children = expect_list(node)?;
    let mut emits = Vec::new();

    for child in children.iter().skip(1) {
        // skip "emits" tag
        if let SexpNode::List(parts) = child {
            if parts.is_empty() {
                continue;
            }
            let emit_name = expect_atom(&parts[0])
                .map_err(|e| format!("vue-component `{comp_name}` emit: {e}"))?
                .to_string();
            if emit_name.starts_with(':') {
                continue;
            }
            let payload = child.keyword(":payload").map(String::from);

            emits.push(ComponentEmit {
                name: emit_name,
                payload,
            });
        }
    }
    Ok(emits)
}

fn parse_slots(node: &SexpNode, comp_name: &str) -> Result<Vec<ComponentSlot>, String> {
    let children = expect_list(node)?;
    let mut slots = Vec::new();

    for child in children.iter().skip(1) {
        // skip "slots" tag
        if let SexpNode::List(parts) = child {
            if parts.is_empty() {
                continue;
            }
            let slot_name = expect_atom(&parts[0])
                .map_err(|e| format!("vue-component `{comp_name}` slot: {e}"))?
                .to_string();
            if slot_name.starts_with(':') {
                continue;
            }
            let description = child.keyword(":description").map(String::from);

            slots.push(ComponentSlot {
                name: slot_name,
                description,
            });
        }
    }
    Ok(slots)
}

fn parse_stores(node: &SexpNode) -> Result<Vec<StoreDep>, String> {
    let children = expect_list(node)?;
    let mut stores = Vec::new();
    for child in children.iter().skip(1) {
        if let SexpNode::List(parts) = child {
            if parts.len() >= 2 {
                let name = expect_atom(&parts[0])?.to_string();
                if name.starts_with(':') { continue; }
                let import_path = expect_atom(&parts[1])?.to_string();
                stores.push(StoreDep { name, import_path });
            }
        }
    }
    Ok(stores)
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

fn has_flag(node: &SexpNode, key: &str) -> bool {
    if let SexpNode::List(children) = node {
        children
            .iter()
            .any(|c| matches!(c, SexpNode::Atom(s) if s == key))
    } else {
        false
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
    fn parse_full_vue_component() {
        let sexp = parse(
            r#"(vue-component QueuePanel
              :target-path "src/components/queue/QueuePanel.vue"
              (props
                (jobs array :required)
                (activeJobId string)
                (maxVisible number :default 50))
              (emits
                (job-selected :payload "{ jobId: string }")
                (queue-cleared))
              (slots
                (header)
                (empty-state :description "Shown when no jobs")))"#,
        );
        let comp = VueComponentDecl::from_sexp(&sexp).unwrap();

        assert_eq!(comp.name, "QueuePanel");
        assert_eq!(
            comp.target_path,
            Some("src/components/queue/QueuePanel.vue".into())
        );

        // props
        assert_eq!(comp.props.len(), 3);
        assert_eq!(comp.props[0].name, "jobs");
        assert_eq!(comp.props[0].ts_type, "array");
        assert!(comp.props[0].required);
        assert_eq!(comp.props[0].default_value, None);

        assert_eq!(comp.props[1].name, "activeJobId");
        assert_eq!(comp.props[1].ts_type, "string");
        assert!(!comp.props[1].required);

        assert_eq!(comp.props[2].name, "maxVisible");
        assert_eq!(comp.props[2].ts_type, "number");
        assert_eq!(comp.props[2].default_value, Some("50".into()));

        // emits
        assert_eq!(comp.emits.len(), 2);
        assert_eq!(comp.emits[0].name, "job-selected");
        assert_eq!(
            comp.emits[0].payload,
            Some("{ jobId: string }".into())
        );
        assert_eq!(comp.emits[1].name, "queue-cleared");
        assert_eq!(comp.emits[1].payload, None);

        // slots
        assert_eq!(comp.slots.len(), 2);
        assert_eq!(comp.slots[0].name, "header");
        assert_eq!(comp.slots[0].description, None);
        assert_eq!(comp.slots[1].name, "empty-state");
        assert_eq!(
            comp.slots[1].description,
            Some("Shown when no jobs".into())
        );
    }

    #[test]
    fn parse_minimal_vue_component() {
        let sexp = parse(r#"(vue-component EmptyComponent)"#);
        let comp = VueComponentDecl::from_sexp(&sexp).unwrap();

        assert_eq!(comp.name, "EmptyComponent");
        assert_eq!(comp.target_path, None);
        assert!(comp.props.is_empty());
        assert!(comp.emits.is_empty());
        assert!(comp.slots.is_empty());
    }

    #[test]
    fn parse_props_only() {
        let sexp = parse(
            r#"(vue-component StatusBadge
              :target-path "src/components/StatusBadge.vue"
              (props
                (status string :required :description "Current status label")
                (variant string :default "default")))"#,
        );
        let comp = VueComponentDecl::from_sexp(&sexp).unwrap();

        assert_eq!(comp.props.len(), 2);
        assert!(comp.props[0].required);
        assert_eq!(
            comp.props[0].description,
            Some("Current status label".into())
        );
        assert_eq!(comp.props[1].default_value, Some("default".into()));
        assert!(comp.emits.is_empty());
        assert!(comp.slots.is_empty());
    }

    #[test]
    fn parse_stores_and_model() {
        let sexp = parse(
            r#"(vue-component SelectInput
              (model "string | undefined")
              (stores
                (useSettingStore "@/platform/settings")
                (useCommandStore "@/stores/commandStore"))
              (props
                (label string)))"#,
        );
        let comp = VueComponentDecl::from_sexp(&sexp).unwrap();
        assert_eq!(comp.model, Some("string | undefined".into()));
        assert_eq!(comp.stores.len(), 2);
        assert_eq!(comp.stores[0].name, "useSettingStore");
        assert_eq!(comp.stores[0].import_path, "@/platform/settings");
    }

    #[test]
    fn accept_arbitrary_prop_type() {
        let sexp = parse(
            r#"(vue-component FlexComp
              (props
                (data "Record<string, unknown>" :required)
                (variant "'lg' | 'md'" :default "lg")))"#,
        );
        let comp = VueComponentDecl::from_sexp(&sexp).unwrap();
        assert_eq!(comp.props[0].ts_type, "Record<string, unknown>");
        assert_eq!(comp.props[1].ts_type, "'lg' | 'md'");
    }

    #[test]
    fn parse_multiple_vue_components() {
        let input = r#"
          (vue-component CompA
            (props
              (title string :required)))

          (vue-component CompB
            (emits
              (clicked)))

          ; non-vue-component should be ignored
          (canvas-node NodeX
            :category "util")
        "#;
        let nodes = SexpNode::parse(input).unwrap();
        let comps = parse_vue_components(&nodes).unwrap();

        assert_eq!(comps.len(), 2);
        assert_eq!(comps[0].name, "CompA");
        assert_eq!(comps[1].name, "CompB");
    }
}
