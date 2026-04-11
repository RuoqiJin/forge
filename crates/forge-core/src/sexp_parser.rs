//! Minimal S-expression parser — recursive descent, zero dependencies.
//! Adapted from nc-parser for the Forge.

use std::fmt;

/// An S-expression node: either an atom (string) or a list of nodes.
#[derive(Debug, Clone, PartialEq)]
pub enum SexpNode {
    Atom(String),
    List(Vec<SexpNode>),
}

impl fmt::Display for SexpNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SexpNode::Atom(s) => {
                if s.contains(' ') || s.contains('"') || s.is_empty() {
                    write!(f, "\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
                } else {
                    write!(f, "{}", s)
                }
            }
            SexpNode::List(items) => {
                write!(f, "(")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{}", item)?;
                }
                write!(f, ")")
            }
        }
    }
}

impl SexpNode {
    /// Parse a string into a list of top-level S-expression nodes.
    pub fn parse(input: &str) -> Result<Vec<SexpNode>, String> {
        let tokens = tokenize(input);
        let mut pos = 0;
        let mut results = Vec::new();
        while pos < tokens.len() {
            let (node, next) = parse_node(&tokens, pos)?;
            results.push(node);
            pos = next;
        }
        Ok(results)
    }

    /// Convenience: parse exactly one top-level form.
    pub fn parse_one(input: &str) -> Result<SexpNode, String> {
        let nodes = Self::parse(input)?;
        nodes.into_iter().next().ok_or_else(|| "empty input".into())
    }

    /// Get the atom string value, stripping quotes if present.
    pub fn as_atom(&self) -> Option<&str> {
        match self {
            SexpNode::Atom(s) => {
                if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
                    Some(&s[1..s.len() - 1])
                } else {
                    Some(s)
                }
            }
            SexpNode::List(_) => None,
        }
    }

    /// Get ALL child nodes if this is a list (including the tag).
    /// For the tag-skipping version, use `body()`.
    pub fn children(&self) -> &[SexpNode] {
        match self {
            SexpNode::List(items) => items,
            SexpNode::Atom(_) => &[],
        }
    }

    /// Get the tag (first atom in a list).
    pub fn tag(&self) -> Option<&str> {
        self.children().first().and_then(|n| n.as_atom())
    }

    // -- Query API (Jarvis-compatible) ----------------------------------------

    /// The second atom of a List (the identifier).
    /// `(pillar memory ...)` -> `Some("memory")`
    pub fn id(&self) -> Option<&str> {
        self.children().get(1).and_then(|n| n.as_atom())
    }

    /// All child nodes of a List, skipping the tag atom.
    /// Returns empty vec for Atom or empty list.
    pub fn body(&self) -> Vec<&SexpNode> {
        match self {
            SexpNode::List(items) if !items.is_empty() => items.iter().skip(1).collect(),
            _ => vec![],
        }
    }

    /// Find the first child List whose tag matches.
    pub fn find(&self, tag: &str) -> Option<&SexpNode> {
        match self {
            SexpNode::List(items) => items.iter().find(|child| child.tag() == Some(tag)),
            _ => None,
        }
    }

    /// Find all child Lists whose tag matches.
    pub fn find_all(&self, tag: &str) -> Vec<&SexpNode> {
        match self {
            SexpNode::List(items) => items.iter().filter(|child| child.tag() == Some(tag)).collect(),
            _ => vec![],
        }
    }

    /// Find `(tag "value")` child and return the value string.
    /// `(purpose "data capture").str_val("purpose")` -> `Some("data capture")`
    pub fn str_val(&self, tag: &str) -> Option<&str> {
        let child = self.find(tag)?;
        child.children().get(1).and_then(|n| n.as_atom())
    }

    /// Find `(tag a b c)` child and return all atoms after the tag.
    /// `(events A B C).list_val("events")` -> `["A", "B", "C"]`
    pub fn list_val(&self, tag: &str) -> Vec<&str> {
        match self.find(tag) {
            Some(SexpNode::List(items)) => items
                .iter()
                .skip(1) // skip the tag atom
                .filter_map(|child| child.as_atom())
                .collect(),
            _ => vec![],
        }
    }

    /// Shorthand for `str_val("human-label")`.
    pub fn human_label(&self) -> Option<&str> {
        self.str_val("human-label")
    }

    /// Shorthand for `str_val("human-desc")`.
    pub fn human_desc(&self) -> Option<&str> {
        self.str_val("human-desc")
    }

    /// Pretty-print the S-expression with indentation.
    pub fn to_pretty(&self, indent: usize) -> String {
        match self {
            SexpNode::Atom(s) => {
                if s.contains(' ') || s.is_empty() {
                    // Already quoted atoms (starting with ") pass through
                    if s.starts_with('"') && s.ends_with('"') {
                        s.clone()
                    } else {
                        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
                    }
                } else {
                    s.clone()
                }
            }
            SexpNode::List(items) => {
                if items.is_empty() {
                    return "()".into();
                }
                // Short list heuristic: all atoms and total < 80 chars -> inline
                let inline = format!("{}", self);
                if inline.len() < 80 && items.iter().all(|i| matches!(i, SexpNode::Atom(_))) {
                    return inline;
                }
                // Multi-line with indentation
                let child_pad = "  ".repeat(indent + 1);
                let mut out = format!("({}", items[0].to_pretty(indent + 1));
                for item in items.iter().skip(1) {
                    match item {
                        SexpNode::List(_) => {
                            out.push_str(&format!(
                                "\n{}{}",
                                child_pad,
                                item.to_pretty(indent + 1)
                            ));
                        }
                        SexpNode::Atom(_) => {
                            let last_line_len =
                                out.lines().last().map(|l| l.len()).unwrap_or(0);
                            let atom_str = item.to_pretty(indent + 1);
                            if last_line_len + 1 + atom_str.len() < 80 {
                                out.push(' ');
                                out.push_str(&atom_str);
                            } else {
                                out.push_str(&format!("\n{}{}", child_pad, atom_str));
                            }
                        }
                    }
                }
                out.push(')');
                out
            }
        }
    }
}

fn tokenize(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut chars = input.chars().peekable();

    while let Some(&ch) = chars.peek() {
        match ch {
            '(' | ')' => {
                tokens.push(ch.to_string());
                chars.next();
            }
            ';' => {
                // Line comment
                while let Some(&c) = chars.peek() {
                    chars.next();
                    if c == '\n' { break; }
                }
            }
            '"' => {
                let mut s = String::from('"');
                chars.next();
                while let Some(&c) = chars.peek() {
                    chars.next();
                    s.push(c);
                    if c == '"' { break; }
                    if c == '\\' {
                        if let Some(&esc) = chars.peek() {
                            s.push(esc);
                            chars.next();
                        }
                    }
                }
                tokens.push(s);
            }
            c if c.is_whitespace() => { chars.next(); }
            _ => {
                let mut atom = String::new();
                while let Some(&c) = chars.peek() {
                    if c == '(' || c == ')' || c == ';' || c == '"' || c.is_whitespace() {
                        break;
                    }
                    atom.push(c);
                    chars.next();
                }
                tokens.push(atom);
            }
        }
    }
    tokens
}

fn parse_node(tokens: &[String], pos: usize) -> Result<(SexpNode, usize), String> {
    if pos >= tokens.len() {
        return Err("unexpected end of input".into());
    }
    if tokens[pos] == "(" {
        let mut items = Vec::new();
        let mut i = pos + 1;
        while i < tokens.len() && tokens[i] != ")" {
            let (node, next) = parse_node(tokens, i)?;
            items.push(node);
            i = next;
        }
        if i >= tokens.len() {
            return Err("unmatched opening '('".into());
        }
        Ok((SexpNode::List(items), i + 1))
    } else if tokens[pos] == ")" {
        Err("unexpected ')'".into())
    } else {
        Ok((SexpNode::Atom(tokens[pos].clone()), pos + 1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple() {
        let nodes = SexpNode::parse("(hello world)").unwrap();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].tag(), Some("hello"));
    }

    #[test]
    fn test_parse_nested() {
        let nodes = SexpNode::parse("(a (b c) d)").unwrap();
        assert_eq!(nodes[0].children().len(), 3);
    }

    #[test]
    fn test_quoted_strings() {
        let node = SexpNode::parse_one(r#"(role "does stuff")"#).unwrap();
        assert_eq!(node.children()[1].as_atom(), Some("does stuff"));
    }
}
