//! Atom: ts-interface
//!
//! Generates a TypeScript interface definition with typed fields.
//!
//! ## What it generates
//!
//! ```typescript
//! export interface QueuePanelProps {
//!     jobs: unknown[];
//!     activeJobId?: string;
//!     /** Max items to show */
//!     maxVisible?: number;
//! }
//! ```
//!
//! ## What it does NOT generate
//!
//! - No import statements (that's `import_stmt`)
//! - No Zod schemas (that's `zod_schema`)
//! - No class bodies or methods

use crate::common::w;
use crate::quarks::doc_comment::{DocLang, doc_line};

/// A single field in a TypeScript interface.
#[derive(Debug, Clone)]
pub struct InterfaceField {
    pub name: String,
    pub ts_type: String,
    pub optional: bool,
    pub description: Option<String>,
}

/// Generate a TypeScript interface definition.
///
/// Returns the generated code as a String fragment.
pub fn gen_ts_interface(name: &str, fields: &[InterfaceField], exported: bool) -> String {
    let mut out = String::with_capacity(512);

    let export_prefix = if exported { "export " } else { "" };
    w(&mut out, 0, &format!("{export_prefix}interface {name} {{"));

    for field in fields {
        // JSDoc description
        if let Some(ref desc) = field.description {
            w(&mut out, 1, &doc_line(DocLang::TypeScript, desc));
        }

        let opt = if field.optional { "?" } else { "" };
        w(&mut out, 1, &format!("{}{}: {};", field.name, opt, field.ts_type));
    }

    w(&mut out, 0, "}");
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_interface() {
        let fields = vec![
            InterfaceField {
                name: "jobs".into(),
                ts_type: "unknown[]".into(),
                optional: false,
                description: None,
            },
            InterfaceField {
                name: "activeJobId".into(),
                ts_type: "string".into(),
                optional: true,
                description: None,
            },
            InterfaceField {
                name: "maxVisible".into(),
                ts_type: "number".into(),
                optional: true,
                description: Some("Max items to show".into()),
            },
        ];

        let code = gen_ts_interface("QueuePanelProps", &fields, true);

        assert!(code.contains("export interface QueuePanelProps {"));
        assert!(code.contains("    jobs: unknown[];"));
        assert!(code.contains("    activeJobId?: string;"));
        assert!(code.contains("    /** Max items to show */"));
        assert!(code.contains("    maxVisible?: number;"));
        assert!(code.contains("}"));
    }

    #[test]
    fn non_exported_interface() {
        let fields = vec![InterfaceField {
            name: "id".into(),
            ts_type: "string".into(),
            optional: false,
            description: None,
        }];

        let code = gen_ts_interface("InternalState", &fields, false);

        assert!(code.contains("interface InternalState {"));
        assert!(!code.contains("export"));
    }

    #[test]
    fn empty_interface() {
        let code = gen_ts_interface("Empty", &[], true);
        assert!(code.contains("export interface Empty {"));
        assert!(code.contains("}"));
    }
}
