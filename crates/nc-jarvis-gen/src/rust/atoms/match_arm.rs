//! Atom: match-arm
//!
//! Generates Rust `match` expressions with typed arms.
//! Used for enum dispatch, error codes, state transitions, event handling.
//!
//! ## What it generates
//!
//! ```text
//! match self {
//!     Self::Pending => "pending",
//!     Self::Active => "active",
//!     _ => "unknown",
//! }
//! ```

use super::w;

/// A single arm in a match expression.
#[derive(Debug, Clone)]
pub struct MatchArmDef {
    /// Pattern to match, e.g., `Self::Pending` or `"pending"` or `0x0100`.
    pub pattern: String,
    /// Body of the arm, e.g., `write!(f, "pending")` or `true`.
    pub body: String,
    /// Optional inline comment appended after the arm.
    pub doc: Option<String>,
}

/// Input for match expression generation.
pub struct MatchExprInput {
    /// The subject being matched, e.g., `self` or `s` or `code`.
    pub subject: String,
    /// Match arms.
    pub arms: Vec<MatchArmDef>,
    /// Optional catch-all `_ => ...` body.
    pub fallback: Option<String>,
    /// Base indentation level.
    pub indent: usize,
}

/// Generate a match expression.
///
/// Returns the generated code as a String fragment.
pub fn gen_match_expr(input: &MatchExprInput) -> String {
    let mut out = String::with_capacity(256);
    let base = input.indent;

    // match keyword
    w(&mut out, base, &format!("match {} {{", input.subject));

    for arm in &input.arms {
        // Multi-line body: generate a block arm
        if arm.body.contains('\n') {
            let line = format!("{} => {{", arm.pattern);
            if let Some(ref doc) = arm.doc {
                w(&mut out, base + 1, &format!("{line} // {doc}"));
            } else {
                w(&mut out, base + 1, &line);
            }
            for body_line in arm.body.lines() {
                if body_line.is_empty() {
                    out.push('\n');
                } else {
                    w(&mut out, base + 2, body_line);
                }
            }
            w(&mut out, base + 1, "}");
        } else {
            // Single-line arm
            let line = format!("{} => {},", arm.pattern, arm.body);
            if let Some(ref doc) = arm.doc {
                w(&mut out, base + 1, &format!("{line} // {doc}"));
            } else {
                w(&mut out, base + 1, &line);
            }
        }
    }

    // Fallback arm
    if let Some(ref fallback) = input.fallback {
        if fallback.contains('\n') {
            w(&mut out, base + 1, "_ => {");
            for line in fallback.lines() {
                if line.is_empty() {
                    out.push('\n');
                } else {
                    w(&mut out, base + 2, line);
                }
            }
            w(&mut out, base + 1, "}");
        } else {
            w(&mut out, base + 1, &format!("_ => {fallback},"));
        }
    }

    // Close match
    w(&mut out, base, "}");
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_match() {
        let input = MatchExprInput {
            subject: "self".into(),
            arms: vec![
                MatchArmDef {
                    pattern: "Self::Pending".into(),
                    body: "\"pending\"".into(),
                    doc: None,
                },
                MatchArmDef {
                    pattern: "Self::Active".into(),
                    body: "\"active\"".into(),
                    doc: None,
                },
            ],
            fallback: None,
            indent: 0,
        };

        let code = gen_match_expr(&input);

        assert!(code.contains("match self {"));
        assert!(code.contains("Self::Pending => \"pending\","));
        assert!(code.contains("Self::Active => \"active\","));
        assert!(!code.contains("_ =>"));

        let open = code.matches('{').count();
        let close = code.matches('}').count();
        assert_eq!(open, close, "unbalanced: {open} vs {close}");
    }

    #[test]
    fn match_with_fallback() {
        let input = MatchExprInput {
            subject: "s".into(),
            arms: vec![
                MatchArmDef {
                    pattern: "\"pending\"".into(),
                    body: "Self::Pending".into(),
                    doc: None,
                },
                MatchArmDef {
                    pattern: "\"active\"".into(),
                    body: "Self::Active".into(),
                    doc: None,
                },
            ],
            fallback: Some("return Err(\"unknown\".into())".into()),
            indent: 0,
        };

        let code = gen_match_expr(&input);

        assert!(code.contains("match s {"));
        assert!(code.contains("\"pending\" => Self::Pending,"));
        assert!(code.contains("_ => return Err(\"unknown\".into()),"));
    }

    #[test]
    fn match_with_inline_comments() {
        let input = MatchExprInput {
            subject: "code".into(),
            arms: vec![
                MatchArmDef {
                    pattern: "0x0100".into(),
                    body: "\"not found\"".into(),
                    doc: Some("resource missing".into()),
                },
                MatchArmDef {
                    pattern: "0x0200".into(),
                    body: "\"conflict\"".into(),
                    doc: Some("version mismatch".into()),
                },
            ],
            fallback: Some("\"unknown\"".into()),
            indent: 0,
        };

        let code = gen_match_expr(&input);

        assert!(code.contains("0x0100 => \"not found\", // resource missing"));
        assert!(code.contains("0x0200 => \"conflict\", // version mismatch"));
        assert!(code.contains("_ => \"unknown\","));
    }

    #[test]
    fn empty_arms() {
        let input = MatchExprInput {
            subject: "x".into(),
            arms: vec![],
            fallback: None,
            indent: 0,
        };

        let code = gen_match_expr(&input);

        assert!(code.contains("match x {"));
        assert!(code.contains("}"));

        let open = code.matches('{').count();
        let close = code.matches('}').count();
        assert_eq!(open, close);
    }

    #[test]
    fn indented_match() {
        let input = MatchExprInput {
            subject: "self".into(),
            arms: vec![MatchArmDef {
                pattern: "Self::A".into(),
                body: "1".into(),
                doc: None,
            }],
            fallback: Some("0".into()),
            indent: 2,
        };

        let code = gen_match_expr(&input);

        // match keyword at indent 2 (8 spaces)
        assert!(code.contains("        match self {"));
        // arm at indent 3 (12 spaces)
        assert!(code.contains("            Self::A => 1,"));
        // fallback at indent 3
        assert!(code.contains("            _ => 0,"));
        // closing brace at indent 2
        assert!(code.contains("        }"));
    }

    #[test]
    fn multiline_body_arm() {
        let input = MatchExprInput {
            subject: "event".into(),
            arms: vec![MatchArmDef {
                pattern: "Event::Click { x, y }".into(),
                body: "let pos = (x, y);\nhandle_click(pos)".into(),
                doc: None,
            }],
            fallback: Some("{}".into()),
            indent: 0,
        };

        let code = gen_match_expr(&input);

        assert!(code.contains("Event::Click { x, y } => {"));
        assert!(code.contains("        let pos = (x, y);"));
        assert!(code.contains("        handle_click(pos)"));
        // Block arm closing brace
        let lines: Vec<&str> = code.lines().collect();
        // Should have a closing } for the block arm at indent 1
        assert!(lines.iter().any(|l| l.trim() == "}" && l.starts_with("    ") && !l.starts_with("        ")));
    }

    #[test]
    fn multiline_body_with_comment() {
        let input = MatchExprInput {
            subject: "state".into(),
            arms: vec![MatchArmDef {
                pattern: "State::Init".into(),
                body: "setup();\nState::Ready".into(),
                doc: Some("initialization phase".into()),
            }],
            fallback: None,
            indent: 0,
        };

        let code = gen_match_expr(&input);

        assert!(code.contains("State::Init => { // initialization phase"));
        assert!(code.contains("        setup();"));
        assert!(code.contains("        State::Ready"));
    }

    #[test]
    fn multiline_fallback() {
        let input = MatchExprInput {
            subject: "v".into(),
            arms: vec![],
            fallback: Some("eprintln!(\"unexpected\");\npanic!(\"abort\")".into()),
            indent: 0,
        };

        let code = gen_match_expr(&input);

        assert!(code.contains("_ => {"));
        assert!(code.contains("        eprintln!(\"unexpected\");"));
        assert!(code.contains("        panic!(\"abort\")"));

        let open = code.matches('{').count();
        let close = code.matches('}').count();
        assert_eq!(open, close);
    }
}
