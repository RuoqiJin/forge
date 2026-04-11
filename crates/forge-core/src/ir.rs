//! Jarvis Domain IR — typed whitelist for event-driven daemon components.
//!
//! Validates intent.lisp S-expressions against a strict schema.
//! Invalid declarations are rejected with structured `IrError` feedback
//! designed for LLM self-correction.

use crate::sexp_parser::SexpNode;

// ============================================================================
// Domain Enums (the whitelist)
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum PillarKind {
    Memory,
    Control,
    Tools,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub enum JarvisEvent {
    MessagesIngested,
    SessionStateChanged,
    UserMessageReceived,
    TextComplete,
    ContextReady,
    ToolCalled,
    CodebaseMutated,
    Custom(String),
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub enum StateDep {
    Storage,
    SessionManager,
    EventBus,
    TopologyManager,
    RealityMirror,
}

// ============================================================================
// IR Types
// ============================================================================

#[derive(Debug, Clone, serde::Serialize)]
pub struct ComponentDecl {
    pub pillar: PillarKind,
    pub name: String,
    pub role: String,
    pub depends_on: Vec<String>,
    pub listens_to: Vec<JarvisEvent>,
    pub state_deps: Vec<StateDep>,
    pub invariants: Vec<String>,
    pub data_flow: Option<String>,
    pub logic_fns: Vec<LogicFnDecl>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct LogicFnDecl {
    pub name: String,
    pub inputs: Vec<(String, String)>,
    pub output: String,
    pub steps: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct FlowDecl {
    pub name: String,
    pub steps: Vec<String>,
}

// ============================================================================
// Validation Error
// ============================================================================

#[derive(Debug, Clone, serde::Serialize)]
pub struct IrError {
    pub field: String,
    pub message: String,
    pub expected: Vec<String>,
    pub got: String,
}

impl std::fmt::Display for IrError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "IR error in `{}`: {} (expected {:?}, got {:?})",
            self.field, self.message, self.expected, self.got)
    }
}

impl std::error::Error for IrError {}

// ============================================================================
// Lowering: SexpNode → IR
// ============================================================================

impl PillarKind {
    pub fn from_str(s: &str) -> Result<Self, IrError> {
        match s {
            "memory" => Ok(Self::Memory),
            "control" => Ok(Self::Control),
            "tools" => Ok(Self::Tools),
            other => Err(IrError {
                field: "pillar".into(),
                message: "invalid pillar kind".into(),
                expected: vec!["memory".into(), "control".into(), "tools".into()],
                got: other.into(),
            }),
        }
    }
}

impl JarvisEvent {
    pub fn from_str(s: &str) -> Result<Self, IrError> {
        match s {
            "MessagesIngested" => Ok(Self::MessagesIngested),
            "SessionStateChanged" => Ok(Self::SessionStateChanged),
            "UserMessageReceived" => Ok(Self::UserMessageReceived),
            "TextComplete" => Ok(Self::TextComplete),
            "ContextReady" => Ok(Self::ContextReady),
            "ToolCalled" => Ok(Self::ToolCalled),
            "CodebaseMutated" => Ok(Self::CodebaseMutated),
            other if other.starts_with("Custom:") => Ok(Self::Custom(other[7..].to_string())),
            other => Err(IrError {
                field: "event".into(),
                message: "unknown event type".into(),
                expected: vec![
                    "MessagesIngested", "SessionStateChanged", "UserMessageReceived",
                    "TextComplete", "ContextReady", "ToolCalled", "CodebaseMutated", "Custom:<name>",
                ].into_iter().map(String::from).collect(),
                got: other.into(),
            }),
        }
    }
}

impl StateDep {
    pub fn from_str(s: &str) -> Result<Self, IrError> {
        match s {
            "storage" => Ok(Self::Storage),
            "session-manager" | "session_manager" => Ok(Self::SessionManager),
            "event-bus" | "event_bus" => Ok(Self::EventBus),
            "topology-manager" | "topology_manager" => Ok(Self::TopologyManager),
            "reality-mirror" | "reality_mirror" => Ok(Self::RealityMirror),
            other => Err(IrError {
                field: "state".into(),
                message: "unknown state dependency".into(),
                expected: vec!["storage", "session-manager", "event-bus", "topology-manager", "reality-mirror"]
                    .into_iter().map(String::from).collect(),
                got: other.into(),
            }),
        }
    }
}

impl ComponentDecl {
    pub fn from_sexp(node: &SexpNode, pillar: PillarKind) -> Result<Self, IrError> {
        let children = match node {
            SexpNode::List(items) => items,
            _ => return Err(IrError {
                field: "component".into(),
                message: "must be a list".into(),
                expected: vec!["(component name ...)".into()],
                got: format!("{node:?}"),
            }),
        };

        if children.first().and_then(|n| n.as_atom()) != Some("component") {
            return Err(IrError {
                field: "component".into(),
                message: "expected (component ...) form".into(),
                expected: vec!["(component name ...)".into()],
                got: "(other)".into(),
            });
        }

        let name = children.get(1).and_then(|n| n.as_atom())
            .ok_or_else(|| IrError {
                field: "component/name".into(),
                message: "missing name".into(),
                expected: vec!["(component <name> ...)".into()],
                got: "(missing)".into(),
            })?.to_string();

        let mut role = None;
        let mut depends_on = Vec::new();
        let mut listens_to = Vec::new();
        let mut state_deps = Vec::new();
        let mut invariants = Vec::new();
        let mut data_flow = None;
        let mut logic_fns = Vec::new();

        for child in children.iter().skip(2) {
            if let SexpNode::List(items) = child {
                match items.first().and_then(|n| n.as_atom()).unwrap_or("") {
                    "role" => role = items.get(1).and_then(|n| n.as_atom()).map(String::from),
                    "depends-on" => {
                        for d in items.iter().skip(1) {
                            if let Some(s) = d.as_atom() { depends_on.push(s.to_string()); }
                        }
                    }
                    "listens-to" => {
                        for e in items.iter().skip(1) {
                            if let Some(s) = e.as_atom() { listens_to.push(JarvisEvent::from_str(s)?); }
                        }
                    }
                    "state" => {
                        for d in items.iter().skip(1) {
                            if let Some(s) = d.as_atom() { state_deps.push(StateDep::from_str(s)?); }
                        }
                    }
                    "invariants" => {
                        for i in items.iter().skip(1) {
                            if let Some(s) = i.as_atom() { invariants.push(s.to_string()); }
                        }
                    }
                    "data-flow" => data_flow = items.get(1).and_then(|n| n.as_atom()).map(String::from),
                    "logic" => {
                        if let Ok(f) = LogicFnDecl::from_sexp(child) { logic_fns.push(f); }
                    }
                    _ => {}
                }
            }
        }

        let role = role.ok_or_else(|| IrError {
            field: format!("component/{name}/role"),
            message: "missing required field: role".into(),
            expected: vec!["(role \"description\")".into()],
            got: "(missing)".into(),
        })?;

        Ok(Self { pillar, name, role, depends_on, listens_to, state_deps, invariants, data_flow, logic_fns })
    }
}

impl LogicFnDecl {
    pub fn from_sexp(node: &SexpNode) -> Result<Self, IrError> {
        let children = match node {
            SexpNode::List(items) => items,
            _ => return Err(IrError {
                field: "logic".into(), message: "must be a list".into(),
                expected: vec!["(logic name ...)".into()], got: format!("{node:?}"),
            }),
        };

        let name = children.get(1).and_then(|n| n.as_atom())
            .unwrap_or("unnamed").to_string();

        let mut inputs = Vec::new();
        let mut output = "Result<()>".to_string();
        let mut steps = Vec::new();

        for child in children.iter().skip(2) {
            if let SexpNode::List(items) = child {
                match items.first().and_then(|n| n.as_atom()).unwrap_or("") {
                    "input" => {
                        for pair in items.iter().skip(1) {
                            if let SexpNode::List(p) = pair {
                                let pn = p.first().and_then(|n| n.as_atom()).unwrap_or("_");
                                let pt = p.get(1).and_then(|n| n.as_atom()).unwrap_or("()");
                                inputs.push((pn.to_string(), pt.to_string()));
                            }
                        }
                    }
                    "output" => { if let Some(s) = items.get(1).and_then(|n| n.as_atom()) { output = s.to_string(); } }
                    "steps" => {
                        for s in items.iter().skip(1) {
                            if let Some(v) = s.as_atom() { steps.push(v.to_string()); }
                        }
                    }
                    _ => {}
                }
            }
        }

        Ok(Self { name, inputs, output, steps })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_component_from_sexp() {
        let input = r#"(component message-persister
            (role "Persists messages to PostgreSQL")
            (depends-on storage)
            (listens-to MessagesIngested UserMessageReceived)
            (state storage event-bus))"#;
        let nodes = SexpNode::parse(input).unwrap();
        let decl = ComponentDecl::from_sexp(&nodes[0], PillarKind::Memory).unwrap();
        assert_eq!(decl.name, "message-persister");
        assert_eq!(decl.listens_to.len(), 2);
    }

    #[test]
    fn test_missing_role_rejected() {
        let input = r#"(component no-role (depends-on storage))"#;
        let nodes = SexpNode::parse(input).unwrap();
        assert!(ComponentDecl::from_sexp(&nodes[0], PillarKind::Memory).is_err());
    }

    #[test]
    fn test_invalid_event_rejected() {
        let input = r#"(component bad (role "test") (listens-to SkynetAwake))"#;
        let nodes = SexpNode::parse(input).unwrap();
        assert!(ComponentDecl::from_sexp(&nodes[0], PillarKind::Control).is_err());
    }
}
