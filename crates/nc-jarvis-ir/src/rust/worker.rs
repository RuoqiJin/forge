//! Worker IR — typed representation of `(worker ...)` declarations.
//!
//! Two patterns:
//! - `event-listener`: subscribes to EventBus, dispatches to trait handler
//! - `cron-worker`: time-driven polling loop, dispatches to trait handler
//!
//! Both use Generation Gap: generated async shell + hand-written trait impl.

use nc_parser::SexpNode;
use std::fmt;

// ---------------------------------------------------------------------------
// Error type (shared with crud)
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
            "Worker IR error in `{}`: {} (expected {:?}, got {:?})",
            self.field, self.message, self.expected, self.got
        )
    }
}

impl std::error::Error for IrError {}

// ---------------------------------------------------------------------------
// Worker pattern kinds — the whitelist
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum WorkerPattern {
    EventListener,
    CronWorker,
}

impl WorkerPattern {
    pub fn from_str(s: &str) -> Result<Self, IrError> {
        match s {
            "event-listener" => Ok(Self::EventListener),
            "cron-worker" => Ok(Self::CronWorker),
            other => Err(IrError {
                field: ":pattern".into(),
                message: "unknown worker pattern".into(),
                expected: vec!["event-listener".into(), "cron-worker".into()],
                got: other.into(),
            }),
        }
    }
}

/// Schedule method for cron workers.
#[derive(Debug, Clone, PartialEq)]
pub enum ScheduleMethod {
    /// `tokio::time::sleep()` in a loop
    Sleep,
    /// `tokio::time::interval().tick()`
    Interval,
}

/// Error handling strategy.
#[derive(Debug, Clone, PartialEq)]
pub enum ErrorStrategy {
    LogAndContinue,
    SilentContinue,
    FailFast,
}

/// Shutdown trigger.
#[derive(Debug, Clone, PartialEq)]
pub enum ShutdownTrigger {
    BroadcastClose,
    None,
}

// ---------------------------------------------------------------------------
// Declaration types
// ---------------------------------------------------------------------------

/// A dependency injected into the worker constructor.
#[derive(Debug, Clone)]
pub struct DepDecl {
    pub name: String,
    pub rust_type: String,
}

/// An event the worker listens to (event-listener pattern).
#[derive(Debug, Clone)]
pub struct EventListenDecl {
    pub event_name: String,
    pub fields: Vec<String>,
}

/// A trait method generated for the worker logic.
#[derive(Debug, Clone)]
pub struct TraitMethodDecl {
    pub name: String,
    pub trigger: String, // event name or "tick"
    pub params: Vec<(String, String)>, // (name, type)
    pub returns: String,
}

/// Generated trait for the worker's business logic.
#[derive(Debug, Clone)]
pub struct TraitDecl {
    pub name: String,
    pub methods: Vec<TraitMethodDecl>,
}

/// Mutable state field managed by the generated shell.
#[derive(Debug, Clone)]
pub struct StateFieldDecl {
    pub name: String,
    pub rust_type: String,
    pub default: Option<String>,
    pub init_expr: Option<String>,
}

/// An event the worker can emit.
#[derive(Debug, Clone)]
pub struct EmitDecl {
    pub event_name: String,
    pub fields: Vec<String>,
}

/// Schedule configuration for cron workers.
#[derive(Debug, Clone)]
pub struct ScheduleDecl {
    pub interval_secs: u64,
    pub method: ScheduleMethod,
}

/// An init function to call on startup (e.g., load_cursors).
#[derive(Debug, Clone)]
pub struct InitDecl {
    pub fn_name: String,
    pub doc: Option<String>,
}

/// Top-level worker declaration.
#[derive(Debug, Clone)]
pub struct WorkerDecl {
    pub name: String,
    pub pattern: WorkerPattern,
    pub struct_name: String,
    pub constructor: String,
    pub target: Option<String>,
    pub deps: Vec<DepDecl>,
    pub listens_to: Vec<EventListenDecl>,
    pub schedule: Option<ScheduleDecl>,
    pub state_fields: Vec<StateFieldDecl>,
    pub init: Option<InitDecl>,
    pub trait_decl: Option<TraitDecl>,
    pub emits: Vec<EmitDecl>,
    pub shutdown: ShutdownTrigger,
    pub error_strategy: ErrorStrategy,
}

// ---------------------------------------------------------------------------
// S-EXP → IR lowering
// ---------------------------------------------------------------------------

impl WorkerDecl {
    pub fn from_sexp(node: &SexpNode) -> Result<Self, IrError> {
        let children = expect_list(node)?;
        if children.len() < 2 {
            return Err(err("worker", "missing worker name", &["worker_name"], ""));
        }

        expect_tag(node, "worker")?;
        let name = expect_atom(&children[1])?.to_string();

        let pattern_str = keyword_required(node, ":pattern")?;
        let pattern = WorkerPattern::from_str(&pattern_str)?;

        let struct_name = keyword_or(node, ":struct", &name);
        let constructor = keyword_or(node, ":constructor", "new");
        let target = node.keyword(":target").map(String::from);

        let deps = parse_deps(node);
        let listens_to = parse_listens_to(node);
        let schedule = parse_schedule(node)?;
        let state_fields = parse_state(node);
        let init = parse_init(node);
        let trait_decl = parse_trait(node)?;
        let emits = parse_emits(node);
        let shutdown = parse_shutdown(node);
        let error_strategy = parse_error_strategy(node);

        Ok(WorkerDecl {
            name,
            pattern,
            struct_name,
            constructor,
            target,
            deps,
            listens_to,
            schedule,
            state_fields,
            init,
            trait_decl,
            emits,
            shutdown,
            error_strategy,
        })
    }
}

/// Parse all `(worker ...)` declarations from a top-level S-EXP list.
pub fn parse_workers(nodes: &[SexpNode]) -> Result<Vec<WorkerDecl>, IrError> {
    nodes
        .iter()
        .filter(|n| n.tag() == Some("worker"))
        .map(WorkerDecl::from_sexp)
        .collect()
}

// ---------------------------------------------------------------------------
// Sub-parsers
// ---------------------------------------------------------------------------

fn parse_deps(parent: &SexpNode) -> Vec<DepDecl> {
    let node = match parent.keyword_node(":deps") {
        Some(n) => n,
        None => return vec![],
    };
    let items = match node {
        SexpNode::List(items) => items,
        _ => return vec![],
    };
    items
        .iter()
        .filter_map(|item| {
            if let SexpNode::List(parts) = item {
                if parts.len() >= 2 {
                    let name = expect_atom(&parts[0]).ok()?.to_string();
                    let rust_type = expect_atom(&parts[1]).ok()?.to_string();
                    return Some(DepDecl { name, rust_type });
                }
            }
            None
        })
        .collect()
}

fn parse_listens_to(parent: &SexpNode) -> Vec<EventListenDecl> {
    let children = match parent {
        SexpNode::List(c) => c,
        _ => return vec![],
    };
    for child in children {
        if child.tag() == Some("listens-to") {
            if let SexpNode::List(items) = child {
                return items
                    .iter()
                    .skip(1)
                    .filter_map(|item| {
                        let event_name = if let SexpNode::Atom(s) = item {
                            s.clone()
                        } else {
                            item.tag()?.to_string()
                        };
                        let fields = match item.keyword_node(":fields") {
                            Some(n) => collect_atoms(n),
                            None => vec![],
                        };
                        Some(EventListenDecl { event_name, fields })
                    })
                    .collect();
            }
        }
    }
    vec![]
}

fn parse_schedule(parent: &SexpNode) -> Result<Option<ScheduleDecl>, IrError> {
    let children = match parent {
        SexpNode::List(c) => c,
        _ => return Ok(None),
    };
    for child in children {
        if child.tag() == Some("schedule") {
            let interval_str = child.keyword(":interval").unwrap_or("0s");
            let interval_secs = parse_duration_secs(interval_str)?;
            let method = match child.keyword(":method") {
                Some("interval") => ScheduleMethod::Interval,
                _ => ScheduleMethod::Sleep,
            };
            return Ok(Some(ScheduleDecl {
                interval_secs,
                method,
            }));
        }
    }
    Ok(None)
}

fn parse_duration_secs(s: &str) -> Result<u64, IrError> {
    let s = s.trim();
    if let Some(num) = s.strip_suffix('s') {
        num.parse().map_err(|_| err(":interval", "invalid seconds", &["5s", "60s"], s))
    } else if let Some(num) = s.strip_suffix('m') {
        let mins: u64 = num
            .parse()
            .map_err(|_| err(":interval", "invalid minutes", &["5m"], s))?;
        Ok(mins * 60)
    } else {
        s.parse()
            .map_err(|_| err(":interval", "invalid duration", &["5s", "60s", "5m"], s))
    }
}

fn parse_state(parent: &SexpNode) -> Vec<StateFieldDecl> {
    let children = match parent {
        SexpNode::List(c) => c,
        _ => return vec![],
    };
    for child in children {
        if child.tag() == Some("state") {
            if let SexpNode::List(items) = child {
                return items
                    .iter()
                    .skip(1)
                    .filter_map(|item| {
                        if let SexpNode::List(parts) = item {
                            if parts.len() >= 2 {
                                let name = expect_atom(&parts[0]).ok()?.to_string();
                                let rust_type = expect_atom(&parts[1]).ok()?.to_string();
                                let default = item.keyword(":default").map(String::from);
                                let init_expr = item.keyword(":init").map(String::from);
                                return Some(StateFieldDecl {
                                    name,
                                    rust_type,
                                    default,
                                    init_expr,
                                });
                            }
                        }
                        None
                    })
                    .collect();
            }
        }
    }
    vec![]
}

fn parse_init(parent: &SexpNode) -> Option<InitDecl> {
    let children = match parent {
        SexpNode::List(c) => c,
        _ => return None,
    };
    for child in children {
        if child.tag() == Some("init") {
            if let SexpNode::List(parts) = child {
                if parts.len() >= 2 {
                    let fn_name = expect_atom(&parts[1]).ok()?.to_string();
                    let doc = child.keyword(":doc").map(String::from);
                    return Some(InitDecl { fn_name, doc });
                }
            }
        }
    }
    None
}

fn parse_trait(parent: &SexpNode) -> Result<Option<TraitDecl>, IrError> {
    let children = match parent {
        SexpNode::List(c) => c,
        _ => return Ok(None),
    };
    for child in children {
        if child.tag() == Some("trait") {
            if let SexpNode::List(items) = child {
                if items.len() < 2 {
                    return Err(err("trait", "missing trait name", &["TraitName"], ""));
                }
                let name = expect_atom(&items[1])
                    .map_err(|_| err("trait", "trait name must be atom", &["TraitName"], ""))?
                    .to_string();
                let mut methods = Vec::new();
                for item in items.iter().skip(2) {
                    if item.tag() == Some("on-event") || item.tag() == Some("on-tick") {
                        let trigger = if item.tag() == Some("on-tick") {
                            "tick".to_string()
                        } else if let SexpNode::List(parts) = item {
                            if parts.len() >= 2 {
                                expect_atom(&parts[1]).unwrap_or("unknown").to_string()
                            } else {
                                "unknown".to_string()
                            }
                        } else {
                            "unknown".to_string()
                        };

                        let params = parse_trait_params(item);
                        let returns = item
                            .keyword(":returns")
                            .unwrap_or("()")
                            .to_string();

                        let method_name = if trigger == "tick" {
                            "on_tick".to_string()
                        } else {
                            format!(
                                "on_{}",
                                trigger
                                    .chars()
                                    .map(|c| if c.is_uppercase() {
                                        format!("_{}", c.to_lowercase())
                                    } else {
                                        c.to_string()
                                    })
                                    .collect::<String>()
                                    .trim_start_matches('_')
                                    .to_string()
                            )
                        };

                        methods.push(TraitMethodDecl {
                            name: method_name,
                            trigger,
                            params,
                            returns,
                        });
                    }
                }
                return Ok(Some(TraitDecl { name, methods }));
            }
        }
    }
    Ok(None)
}

fn parse_trait_params(node: &SexpNode) -> Vec<(String, String)> {
    let params_node = match node.keyword_node(":params") {
        Some(n) => n,
        None => return vec![],
    };
    match params_node {
        SexpNode::List(items) => items
            .iter()
            .filter_map(|item| {
                if let SexpNode::List(parts) = item {
                    if parts.len() >= 2 {
                        let name = expect_atom(&parts[0]).ok()?.to_string();
                        let rust_type = expect_atom(&parts[1]).ok()?.to_string();
                        return Some((name, rust_type));
                    }
                }
                None
            })
            .collect(),
        _ => vec![],
    }
}

fn parse_emits(parent: &SexpNode) -> Vec<EmitDecl> {
    let children = match parent {
        SexpNode::List(c) => c,
        _ => return vec![],
    };
    for child in children {
        if child.tag() == Some("emits") {
            if let SexpNode::List(items) = child {
                return items
                    .iter()
                    .skip(1)
                    .filter_map(|item| {
                        let event_name = item.tag()?.to_string();
                        let fields = match item.keyword_node(":fields") {
                            Some(n) => collect_atoms(n),
                            None => vec![],
                        };
                        Some(EmitDecl { event_name, fields })
                    })
                    .collect();
            }
        }
    }
    vec![]
}

fn parse_shutdown(parent: &SexpNode) -> ShutdownTrigger {
    let children = match parent {
        SexpNode::List(c) => c,
        _ => return ShutdownTrigger::None,
    };
    for child in children {
        if child.tag() == Some("shutdown") {
            return match child.keyword(":on") {
                Some("broadcast-close") => ShutdownTrigger::BroadcastClose,
                _ => ShutdownTrigger::None,
            };
        }
    }
    ShutdownTrigger::None
}

fn parse_error_strategy(parent: &SexpNode) -> ErrorStrategy {
    let children = match parent {
        SexpNode::List(c) => c,
        _ => return ErrorStrategy::LogAndContinue,
    };
    for child in children {
        if child.tag() == Some("error-strategy") {
            if let SexpNode::List(items) = child {
                if items.len() >= 2 {
                    return match expect_atom(&items[1]).unwrap_or("log-and-continue") {
                        "log-and-continue" => ErrorStrategy::LogAndContinue,
                        "silent-continue" => ErrorStrategy::SilentContinue,
                        "fail-fast" => ErrorStrategy::FailFast,
                        _ => ErrorStrategy::LogAndContinue,
                    };
                }
            }
        }
    }
    ErrorStrategy::LogAndContinue
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
        SexpNode::List(_) => Err(err("node", "expected atom", &["atom"], "(…)")),
    }
}

fn keyword_required(node: &SexpNode, key: &str) -> Result<String, IrError> {
    node.keyword(key)
        .map(String::from)
        .ok_or_else(|| err(key, &format!("missing required {key}"), &["value"], ""))
}

fn keyword_or(node: &SexpNode, key: &str, default: &str) -> String {
    node.keyword(key)
        .map(String::from)
        .unwrap_or_else(|| default.to_string())
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
    fn parse_event_listener() {
        let sexp = parse(
            r#"(worker message-persister
              :pattern event-listener
              :struct MessagePersister
              :constructor start
              :deps ((storage "Arc<Storage>") (event_bus EventBus))
              (listens-to
                (UserMessageReceived :fields (session_id content))
                (TextComplete :fields (session_id content)))
              (trait MessagePersisterLogic
                (on-event UserMessageReceived
                  :params ((session_id Uuid) (content String))
                  :returns "()")
                (on-event TextComplete
                  :params ((session_id Uuid) (content String))
                  :returns "()"))
              (shutdown :on broadcast-close)
              (error-strategy log-and-continue))"#,
        );
        let w = WorkerDecl::from_sexp(&sexp).unwrap();
        assert_eq!(w.name, "message-persister");
        assert_eq!(w.pattern, WorkerPattern::EventListener);
        assert_eq!(w.struct_name, "MessagePersister");
        assert_eq!(w.constructor, "start");
        assert_eq!(w.deps.len(), 2);
        assert_eq!(w.listens_to.len(), 2);
        assert_eq!(w.listens_to[0].event_name, "UserMessageReceived");
        assert_eq!(w.listens_to[0].fields, vec!["session_id", "content"]);
        assert_eq!(w.shutdown, ShutdownTrigger::BroadcastClose);

        let tr = w.trait_decl.as_ref().unwrap();
        assert_eq!(tr.name, "MessagePersisterLogic");
        assert_eq!(tr.methods.len(), 2);
        assert_eq!(tr.methods[0].trigger, "UserMessageReceived");
        assert_eq!(tr.methods[0].params.len(), 2);
    }

    #[test]
    fn parse_cron_worker() {
        let sexp = parse(
            r#"(worker git-watcher
              :pattern cron-worker
              :struct GitWatcher
              :deps ((event_tx "broadcast::Sender<Event>") (project_root PathBuf))
              (schedule :interval 5s :method sleep)
              (state
                (last_hash "Option<String>" :default None))
              (trait GitWatcherLogic
                (on-tick
                  :params ((state "&mut GitWatcherState"))
                  :returns "()"))
              (emits (CodebaseMutated :fields (commit_hash reason)))
              (error-strategy silent-continue))"#,
        );
        let w = WorkerDecl::from_sexp(&sexp).unwrap();
        assert_eq!(w.name, "git-watcher");
        assert_eq!(w.pattern, WorkerPattern::CronWorker);
        assert_eq!(w.deps.len(), 2);

        let sched = w.schedule.as_ref().unwrap();
        assert_eq!(sched.interval_secs, 5);
        assert_eq!(sched.method, ScheduleMethod::Sleep);

        assert_eq!(w.state_fields.len(), 1);
        assert_eq!(w.state_fields[0].name, "last_hash");
        assert_eq!(w.state_fields[0].default, Some("None".into()));

        assert_eq!(w.emits.len(), 1);
        assert_eq!(w.emits[0].event_name, "CodebaseMutated");
        assert_eq!(w.error_strategy, ErrorStrategy::SilentContinue);

        let tr = w.trait_decl.as_ref().unwrap();
        assert_eq!(tr.name, "GitWatcherLogic");
        assert_eq!(tr.methods.len(), 1);
        assert_eq!(tr.methods[0].trigger, "tick");
    }

    #[test]
    fn parse_60s_interval() {
        let sexp = parse(
            r#"(worker extraction
              :pattern cron-worker
              :struct ExtractionWorker
              :deps ((storage "Arc<Storage>"))
              (schedule :interval 60s :method interval)
              (trait ExtractionLogic
                (on-tick :returns "()"))
              (error-strategy log-and-continue))"#,
        );
        let w = WorkerDecl::from_sexp(&sexp).unwrap();
        let sched = w.schedule.as_ref().unwrap();
        assert_eq!(sched.interval_secs, 60);
        assert_eq!(sched.method, ScheduleMethod::Interval);
    }

    #[test]
    fn reject_invalid_pattern() {
        let sexp = parse(r#"(worker x :pattern graphql-subscription)"#);
        let err = WorkerDecl::from_sexp(&sexp).unwrap_err();
        assert_eq!(err.field, ":pattern");
        assert!(err.got.contains("graphql-subscription"));
    }
}
