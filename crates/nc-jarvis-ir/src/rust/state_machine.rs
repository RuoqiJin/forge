//! State Machine IR — typed representation of `(state-machine ...)` declarations.
//!
//! Generates: state enum + transition validator + group helpers + guards + trait contract.

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
        write!(f, "StateMachine IR error in `{}`: {} (expected {:?}, got {:?})",
            self.field, self.message, self.expected, self.got)
    }
}

impl std::error::Error for IrError {}

// ---------------------------------------------------------------------------
// Declaration types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct StateDecl {
    pub name: String,
    pub doc: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TransitionDecl {
    pub from: String,
    pub to: String,
    pub trigger: Option<String>,
    pub debounce_ms: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct GroupDecl {
    pub name: String,
    pub states: Vec<String>,
    pub method: String,
    pub doc: Option<String>,
}

#[derive(Debug, Clone)]
pub struct GuardDecl {
    pub action: String,
    pub requires: Option<String>,
    pub requires_not: Option<String>,
    pub error_msg: String,
}

#[derive(Debug, Clone)]
pub struct TraitMethodDecl {
    pub name: String,
    pub params: Vec<(String, String)>,
    pub returns: String,
    pub doc: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TraitDecl {
    pub name: String,
    pub methods: Vec<TraitMethodDecl>,
}

/// Top-level `(state-machine ...)` declaration.
#[derive(Debug, Clone)]
pub struct StateMachineDecl {
    pub name: String,
    pub enum_name: String,
    pub derives: Vec<String>,
    pub target: String,
    pub states: Vec<StateDecl>,
    pub transitions: Vec<TransitionDecl>,
    pub groups: Vec<GroupDecl>,
    pub guards: Vec<GuardDecl>,
    pub trait_decl: Option<TraitDecl>,
}

// ---------------------------------------------------------------------------
// S-EXP → IR lowering
// ---------------------------------------------------------------------------

impl StateMachineDecl {
    pub fn from_sexp(node: &SexpNode) -> Result<Self, IrError> {
        let children = expect_list(node)?;
        expect_tag(node, "state-machine")?;

        if children.len() < 2 {
            return Err(err("state-machine", "missing name", &["name"], ""));
        }
        let name = expect_atom(&children[1])?.to_string();
        let enum_name = keyword_or(node, ":enum", &pascal_case(&name));
        let target = keyword_or(node, ":target", "");

        let derives = match node.keyword_node(":derive") {
            Some(n) => collect_atoms(n),
            None => vec!["Debug".into(), "Clone".into(), "Copy".into(), "PartialEq".into(), "Eq".into()],
        };

        let mut states = Vec::new();
        let mut transitions = Vec::new();
        let mut groups = Vec::new();
        let mut guards = Vec::new();
        let mut trait_decl = None;

        for child in children.iter().skip(2) {
            match child.tag() {
                Some("states") => states = parse_states(child)?,
                Some("transitions") => transitions = parse_transitions(child)?,
                Some("group") => groups.push(parse_group(child)?),
                Some("guards") => guards = parse_guards(child)?,
                Some("trait") => trait_decl = Some(parse_trait(child)?),
                _ => {}
            }
        }

        // Validate: all transition from/to must be declared states
        let state_names: Vec<&str> = states.iter().map(|s| s.name.as_str()).collect();
        for t in &transitions {
            if !state_names.contains(&t.from.as_str()) {
                return Err(err("transition", &format!("unknown 'from' state: {}", t.from),
                    &state_names.iter().map(|s| s.to_string()).collect::<Vec<_>>().iter().map(|s| s.as_str()).collect::<Vec<_>>(), &t.from));
            }
            if !state_names.contains(&t.to.as_str()) {
                return Err(err("transition", &format!("unknown 'to' state: {}", t.to),
                    &state_names.iter().map(|s| s.to_string()).collect::<Vec<_>>().iter().map(|s| s.as_str()).collect::<Vec<_>>(), &t.to));
            }
        }

        Ok(StateMachineDecl {
            name, enum_name, derives, target, states, transitions, groups, guards, trait_decl,
        })
    }
}

// ---------------------------------------------------------------------------
// Sub-parsers
// ---------------------------------------------------------------------------

fn parse_states(node: &SexpNode) -> Result<Vec<StateDecl>, IrError> {
    let children = expect_list(node)?;
    let mut states = Vec::new();
    for child in children.iter().skip(1) {
        match child {
            SexpNode::Atom(name) if !name.starts_with(':') => {
                states.push(StateDecl { name: name.clone(), doc: None });
            }
            SexpNode::List(parts) if !parts.is_empty() => {
                let name = expect_atom(&parts[0])?.to_string();
                if name.starts_with(':') { continue; }
                let doc = child.keyword(":doc").map(String::from);
                states.push(StateDecl { name, doc });
            }
            _ => {}
        }
    }
    Ok(states)
}

fn parse_transitions(node: &SexpNode) -> Result<Vec<TransitionDecl>, IrError> {
    let children = expect_list(node)?;
    let mut transitions = Vec::new();

    for child in children.iter().skip(1) {
        if let SexpNode::List(parts) = child {
            // (From -> To :trigger "..." :debounce 300ms)
            if parts.len() >= 3 {
                let from = expect_atom(&parts[0])?.to_string();
                // parts[1] should be "->"
                let to = expect_atom(&parts[2])?.to_string();
                let trigger = child.keyword(":trigger").map(String::from);
                let debounce_ms = child.keyword(":debounce").and_then(|s| {
                    s.strip_suffix("ms").and_then(|n| n.parse().ok())
                });
                transitions.push(TransitionDecl { from, to, trigger, debounce_ms });
            }
        }
    }
    Ok(transitions)
}

fn parse_group(node: &SexpNode) -> Result<GroupDecl, IrError> {
    let children = expect_list(node)?;
    if children.len() < 3 {
        return Err(err("group", "missing name and states", &["name (S1 S2)"], ""));
    }
    let name = expect_atom(&children[1])?.to_string();

    // Third element should be the list of states
    let states = match &children[2] {
        SexpNode::List(items) => items.iter().filter_map(|i| {
            if let SexpNode::Atom(s) = i { Some(s.clone()) } else { None }
        }).collect(),
        _ => vec![],
    };

    let method = node.keyword(":method").unwrap_or(&name).to_string();
    let doc = node.keyword(":doc").map(String::from);

    Ok(GroupDecl { name, states, method, doc })
}

fn parse_guards(node: &SexpNode) -> Result<Vec<GuardDecl>, IrError> {
    let children = expect_list(node)?;
    let mut guards = Vec::new();
    for child in children.iter().skip(1) {
        if let SexpNode::List(parts) = child {
            if !parts.is_empty() {
                let action = expect_atom(&parts[0])?.to_string();
                if action.starts_with(':') { continue; }
                let requires = child.keyword(":requires").map(String::from);
                let requires_not = child.keyword(":requires-not").map(String::from);
                let error_msg = child.keyword(":error").unwrap_or("guard failed").to_string();
                guards.push(GuardDecl { action, requires, requires_not, error_msg });
            }
        }
    }
    Ok(guards)
}

fn parse_trait(node: &SexpNode) -> Result<TraitDecl, IrError> {
    let children = expect_list(node)?;
    if children.len() < 2 {
        return Err(err("trait", "missing name", &["TraitName"], ""));
    }
    let name = expect_atom(&children[1])?.to_string();

    let mut methods = Vec::new();
    for child in children.iter().skip(2) {
        if let Some(tag) = child.tag() {
            if tag.starts_with(':') { continue; }
            let method_name = tag.replace('-', "_");
            let params = parse_params(child);
            let returns = child.keyword(":returns").unwrap_or("()").to_string();
            let doc = child.keyword(":doc").map(String::from);
            methods.push(TraitMethodDecl { name: method_name, params, returns, doc });
        }
    }
    Ok(TraitDecl { name, methods })
}

fn parse_params(node: &SexpNode) -> Vec<(String, String)> {
    match node.keyword_node(":params") {
        Some(SexpNode::List(items)) => items.iter().filter_map(|item| {
            if let SexpNode::List(parts) = item {
                if parts.len() >= 2 {
                    let name = expect_atom(&parts[0]).ok()?.to_string();
                    let typ = expect_atom(&parts[1]).ok()?.to_string();
                    return Some((name, typ));
                }
            }
            None
        }).collect(),
        _ => vec![],
    }
}

// ---------------------------------------------------------------------------
// Utility helpers
// ---------------------------------------------------------------------------

fn expect_tag<'a>(node: &'a SexpNode, expected: &str) -> Result<&'a str, IrError> {
    let tag = node.tag().ok_or_else(|| err("node", "expected list", &[expected], "atom"))?;
    if tag != expected { return Err(err("tag", "wrong tag", &[expected], tag)); }
    Ok(tag)
}

fn expect_list(node: &SexpNode) -> Result<&Vec<SexpNode>, IrError> {
    match node {
        SexpNode::List(c) => Ok(c),
        SexpNode::Atom(s) => Err(err("node", "expected list", &["(...)"], s)),
    }
}

fn expect_atom<'a>(node: &'a SexpNode) -> Result<&'a str, IrError> {
    match node {
        SexpNode::Atom(s) => Ok(s),
        SexpNode::List(_) => Err(err("node", "expected atom", &["atom"], "(…)")),
    }
}

fn keyword_or(node: &SexpNode, key: &str, default: &str) -> String {
    node.keyword(key).map(String::from).unwrap_or_else(|| default.to_string())
}

fn collect_atoms(node: &SexpNode) -> Vec<String> {
    match node {
        SexpNode::List(items) => items.iter().filter_map(|i| {
            if let SexpNode::Atom(s) = i { if !s.starts_with(':') { return Some(s.clone()); } }
            None
        }).collect(),
        SexpNode::Atom(s) => vec![s.clone()],
    }
}

fn pascal_case(s: &str) -> String {
    s.split(&['-', '_'][..]).map(|w| {
        let mut c = w.chars();
        match c.next() {
            None => String::new(),
            Some(f) => f.to_uppercase().chain(c).collect(),
        }
    }).collect()
}

fn err(field: &str, msg: &str, expected: &[&str], got: &str) -> IrError {
    IrError {
        field: field.into(), message: msg.into(),
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
    fn parse_minimal_state_machine() {
        let sexp = parse(r#"(state-machine pty-state
          :enum SessionState
          (states (Starting) (Idle) (Exited))
          (transitions
            (Starting -> Idle :trigger "prompt")
            (Idle -> Exited :trigger "exit")))"#);
        let sm = StateMachineDecl::from_sexp(&sexp).unwrap();
        assert_eq!(sm.name, "pty-state");
        assert_eq!(sm.enum_name, "SessionState");
        assert_eq!(sm.states.len(), 3);
        assert_eq!(sm.transitions.len(), 2);
        assert_eq!(sm.transitions[0].from, "Starting");
        assert_eq!(sm.transitions[0].to, "Idle");
    }

    #[test]
    fn parse_groups_and_guards() {
        let sexp = parse(r#"(state-machine sm
          :enum State
          (states (A) (B) (C))
          (transitions (A -> B) (B -> C) (C -> A))
          (group processing (A B)
            :method is_processing
            :doc "Active states")
          (guards
            (send :requires A :error "must be A")
            (kill :requires-not C :error "cannot kill in C")))"#);
        let sm = StateMachineDecl::from_sexp(&sexp).unwrap();
        assert_eq!(sm.groups.len(), 1);
        assert_eq!(sm.groups[0].states, vec!["A", "B"]);
        assert_eq!(sm.groups[0].method, "is_processing");
        assert_eq!(sm.guards.len(), 2);
        assert_eq!(sm.guards[0].requires, Some("A".into()));
        assert_eq!(sm.guards[1].requires_not, Some("C".into()));
    }

    #[test]
    fn parse_debounce() {
        let sexp = parse(r#"(state-machine sm
          :enum S
          (states (A) (B))
          (transitions (A -> B :debounce 300ms)))"#);
        let sm = StateMachineDecl::from_sexp(&sexp).unwrap();
        assert_eq!(sm.transitions[0].debounce_ms, Some(300));
    }

    #[test]
    fn reject_invalid_transition_state() {
        let sexp = parse(r#"(state-machine sm
          :enum S
          (states (A) (B))
          (transitions (A -> C)))"#);
        let err = StateMachineDecl::from_sexp(&sexp).unwrap_err();
        assert_eq!(err.field, "transition");
        assert!(err.message.contains("unknown 'to' state: C"));
    }

    #[test]
    fn parse_full_pattern_file() {
        let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let path = std::path::Path::new(&manifest)
            .parent().unwrap().parent().unwrap()
            .join("patterns/jarvis-state-machine.lisp");
        let src = std::fs::read_to_string(&path).unwrap();
        let node = SexpNode::parse_one(&src).unwrap();
        let sm = StateMachineDecl::from_sexp(&node).unwrap();

        assert_eq!(sm.enum_name, "SessionState");
        assert_eq!(sm.states.len(), 7);
        assert!(sm.transitions.len() >= 20, "expected 20+ transitions, got {}", sm.transitions.len());
        assert_eq!(sm.groups.len(), 1);
        assert_eq!(sm.groups[0].method, "is_processing");
        assert_eq!(sm.guards.len(), 2);
        assert!(sm.trait_decl.is_some());
        assert_eq!(sm.trait_decl.as_ref().unwrap().methods.len(), 2);
    }
}
