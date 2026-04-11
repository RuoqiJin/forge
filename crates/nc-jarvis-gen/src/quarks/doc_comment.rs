//! Quark: doc-comment
//!
//! Language-aware doc comment generation. Same semantic intent,
//! different syntax per target language.
//!
//! Consolidates hardcoded `/// {doc}` (Rust) and `/** {desc} */` (TS)
//! patterns from struct_def, enum_def, ts_interface, etc.

/// Target language for doc comment formatting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocLang {
    /// `/// text`
    Rust,
    /// `/** text */`
    TypeScript,
    /// `"""text"""`
    Python,
}

/// Format a single-line doc comment.
///
/// - Rust: `/// {text}`
/// - TypeScript: `/** {text} */`
/// - Python: `"""{text}"""`
pub fn doc_line(lang: DocLang, text: &str) -> String {
    match lang {
        DocLang::Rust => format!("/// {text}"),
        DocLang::TypeScript => format!("/** {text} */"),
        DocLang::Python => format!("\"\"\"{text}\"\"\""),
    }
}

/// Format a multi-line doc comment from a slice of lines.
///
/// - Rust: each line prefixed with `/// `
/// - TypeScript: `/**` + ` * line` per line + ` */`
/// - Python: `"""` + lines + `"""`
pub fn doc_block(lang: DocLang, lines: &[&str]) -> String {
    match lang {
        DocLang::Rust => lines
            .iter()
            .map(|line| format!("/// {line}"))
            .collect::<Vec<_>>()
            .join("\n"),
        DocLang::TypeScript => {
            let mut out = String::from("/**\n");
            for line in lines {
                out.push_str(&format!(" * {line}\n"));
            }
            out.push_str(" */");
            out
        }
        DocLang::Python => {
            let mut out = String::from("\"\"\"\n");
            for line in lines {
                out.push_str(line);
                out.push('\n');
            }
            out.push_str("\"\"\"");
            out
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_single_line() {
        assert_eq!(doc_line(DocLang::Rust, "Hello world"), "/// Hello world");
    }

    #[test]
    fn ts_single_line() {
        assert_eq!(doc_line(DocLang::TypeScript, "Hello world"), "/** Hello world */");
    }

    #[test]
    fn rust_multi_line() {
        let result = doc_block(DocLang::Rust, &["Line 1", "Line 2"]);
        assert_eq!(result, "/// Line 1\n/// Line 2");
    }

    #[test]
    fn ts_multi_line() {
        let result = doc_block(DocLang::TypeScript, &["Line 1", "Line 2"]);
        assert!(result.starts_with("/**\n"));
        assert!(result.contains(" * Line 1\n"));
        assert!(result.contains(" * Line 2\n"));
        assert!(result.ends_with(" */"));
    }

    #[test]
    fn empty_block() {
        assert_eq!(doc_block(DocLang::Rust, &[]), "");
        assert_eq!(doc_block(DocLang::TypeScript, &[]), "/**\n */");
        assert_eq!(doc_block(DocLang::Python, &[]), "\"\"\"\n\"\"\"");
    }

    #[test]
    fn python_single_line() {
        assert_eq!(doc_line(DocLang::Python, "Hello world"), "\"\"\"Hello world\"\"\"");
    }

    #[test]
    fn python_multi_line() {
        let result = doc_block(DocLang::Python, &["Line 1", "Line 2"]);
        assert!(result.starts_with("\"\"\"\n"));
        assert!(result.contains("Line 1\n"));
        assert!(result.contains("Line 2\n"));
        assert!(result.ends_with("\"\"\""));
    }
}
