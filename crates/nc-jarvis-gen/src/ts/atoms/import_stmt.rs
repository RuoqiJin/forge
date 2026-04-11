//! Atom: import-stmt
//!
//! Generates TypeScript import statements (named, default, and type-only).
//!
//! ## What it generates
//!
//! ```typescript
//! import { defineStore } from 'pinia';
//! import type { NodeProgressState } from '@/types';
//! import React from 'react';
//! ```
//!
//! ## What it does NOT generate
//!
//! - No `require()` calls (ESM only)
//! - No dynamic imports (only static)
//! - No side-effect-only imports like `import './styles.css'`

/// A single import statement definition.
#[derive(Debug, Clone)]
pub struct ImportDef {
    /// Named imports, e.g. `["defineStore", "storeToRefs"]`.
    pub names: Vec<String>,
    /// Default import name, e.g. `Some("React")`.
    pub default_name: Option<String>,
    /// Module path, e.g. `"pinia"`, `"@/types"`.
    pub from: String,
    /// Whether to use `import type { ... }` syntax.
    pub type_only: bool,
}

/// Generate TypeScript import statements from a list of import definitions.
///
/// Returns the complete import block as a String. Each import is on its own line.
pub fn gen_imports(imports: &[ImportDef]) -> String {
    let mut out = String::with_capacity(256);

    for imp in imports {
        let type_kw = if imp.type_only { "type " } else { "" };

        match (&imp.default_name, imp.names.is_empty()) {
            // Default import only: `import React from 'react';`
            (Some(default), true) => {
                out.push_str(&format!("import {type_kw}{default} from '{}';\n", imp.from));
            }
            // Named imports only: `import { a, b } from 'mod';`
            (None, false) => {
                let names = imp.names.join(", ");
                out.push_str(&format!(
                    "import {type_kw}{{ {names} }} from '{}';\n",
                    imp.from
                ));
            }
            // Both default and named: `import React, { useState } from 'react';`
            (Some(default), false) => {
                let names = imp.names.join(", ");
                out.push_str(&format!(
                    "import {type_kw}{default}, {{ {names} }} from '{}';\n",
                    imp.from
                ));
            }
            // No default, no names — skip (side-effect imports not supported)
            (None, true) => {}
        }
    }

    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_imports() {
        let imports = vec![ImportDef {
            names: vec!["defineStore".into()],
            default_name: None,
            from: "pinia".into(),
            type_only: false,
        }];

        let code = gen_imports(&imports);
        assert_eq!(code, "import { defineStore } from 'pinia';\n");
    }

    #[test]
    fn type_only_import() {
        let imports = vec![ImportDef {
            names: vec!["NodeProgressState".into()],
            default_name: None,
            from: "@/types".into(),
            type_only: true,
        }];

        let code = gen_imports(&imports);
        assert_eq!(code, "import type { NodeProgressState } from '@/types';\n");
    }

    #[test]
    fn default_import() {
        let imports = vec![ImportDef {
            names: vec![],
            default_name: Some("React".into()),
            from: "react".into(),
            type_only: false,
        }];

        let code = gen_imports(&imports);
        assert_eq!(code, "import React from 'react';\n");
    }

    #[test]
    fn default_plus_named() {
        let imports = vec![ImportDef {
            names: vec!["useState".into(), "useEffect".into()],
            default_name: Some("React".into()),
            from: "react".into(),
            type_only: false,
        }];

        let code = gen_imports(&imports);
        assert_eq!(
            code,
            "import React, { useState, useEffect } from 'react';\n"
        );
    }

    #[test]
    fn multiple_imports() {
        let imports = vec![
            ImportDef {
                names: vec!["defineStore".into()],
                default_name: None,
                from: "pinia".into(),
                type_only: false,
            },
            ImportDef {
                names: vec!["NodeProgressState".into()],
                default_name: None,
                from: "@/types".into(),
                type_only: true,
            },
        ];

        let code = gen_imports(&imports);
        assert!(code.contains("import { defineStore } from 'pinia';"));
        assert!(code.contains("import type { NodeProgressState } from '@/types';"));
    }

    #[test]
    fn empty_import_skipped() {
        let imports = vec![ImportDef {
            names: vec![],
            default_name: None,
            from: "./styles.css".into(),
            type_only: false,
        }];

        let code = gen_imports(&imports);
        assert!(code.is_empty());
    }
}
