//! Pinia Store code generator (Layer 2 Pattern).
//!
//! Composes atomic codegen functions to generate a Pinia store module:
//! - `import { defineStore } from 'pinia'`
//! - `defineStore(...)` with state, getters, and actions
//!
//! ## Layer 2 Principle
//!
//! This file delegates import generation to the `import_stmt` atom
//! and assembles the results into a complete `.ts` store file.

use nc_jarvis_ir::ts::PiniaStoreDecl;

use crate::common::{w, forge_header, blank};
use crate::ts::atoms::import_stmt::{gen_imports, ImportDef};

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub fn generate_pinia_store(decl: &PiniaStoreDecl) -> String {
    let mut out = String::with_capacity(4096);

    let source = decl
        .target_path
        .as_deref()
        .unwrap_or("intent-frontend.lisp");
    forge_header(&mut out, "ts", &format!("{source} (pinia-store {})", decl.name));
    blank(&mut out);

    gen_import_block(&mut out);
    blank(&mut out);
    gen_store(&mut out, decl);

    out
}

// ---------------------------------------------------------------------------
// Imports
// ---------------------------------------------------------------------------

fn gen_import_block(out: &mut String) {
    let imports = gen_imports(&[ImportDef {
        names: vec!["defineStore".into()],
        default_name: None,
        from: "pinia".into(),
        type_only: false,
    }]);
    out.push_str(&imports);
}

// ---------------------------------------------------------------------------
// Store body
// ---------------------------------------------------------------------------

fn gen_store(out: &mut String, decl: &PiniaStoreDecl) {
    // Convert name to "use{Name}Store" convention
    let hook_name = format!(
        "use{}Store",
        capitalize_first(&decl.name)
    );

    w(out, 0, &format!(
        "export const {hook_name} = defineStore('{}', {{",
        decl.name
    ));

    // State
    if !decl.state.is_empty() {
        w(out, 1, "state: () => ({");
        for field in &decl.state {
            match &field.default_value {
                Some(default) => {
                    w(out, 2, &format!("{}: {} as {},", field.name, default, field.ts_type));
                }
                None => {
                    w(out, 2, &format!(
                        "{}: {} as {},",
                        field.name,
                        default_value_for_type(&field.ts_type),
                        field.ts_type,
                    ));
                }
            }
        }
        w(out, 1, "}),");
    }

    // Getters
    if !decl.getters.is_empty() {
        w(out, 1, "getters: {");
        for getter in &decl.getters {
            let derives_comment = if getter.derives_from.is_empty() {
                String::new()
            } else {
                format!(" derives from {} ", getter.derives_from.join(", "))
            };
            w(out, 2, &format!(
                "{}(): {} {{ /* TODO:{} */ return {} as {}; }},",
                getter.name,
                getter.return_type,
                derives_comment,
                default_return_value(&getter.return_type),
                getter.return_type,
            ));
        }
        w(out, 1, "},");
    }

    // Actions
    if !decl.actions.is_empty() {
        w(out, 1, "actions: {");
        for action in &decl.actions {
            let params: Vec<String> = action
                .params
                .iter()
                .map(|p| format!("{}: {}", p.name, p.ts_type))
                .collect();
            let params_str = params.join(", ");
            let return_type = action.return_type.as_deref().unwrap_or("void");
            w(out, 2, &format!(
                "{}({}): {} {{ /* TODO */ }},",
                action.name, params_str, return_type
            ));
        }
        w(out, 1, "},");
    }

    w(out, 0, "});");
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

use crate::quarks::ident::capitalize_first;

/// Provide a sensible zero-value for common TS return types.
fn default_return_value(return_type: &str) -> &str {
    match return_type {
        "boolean" => "false",
        "number" => "0",
        "string" => "''",
        _ => "undefined",
    }
}

/// Provide a sensible default initializer value for TS types.
fn default_value_for_type(ts_type: &str) -> &str {
    match ts_type {
        "boolean" => "false",
        "number" => "0",
        "string" => "''",
        _ if ts_type.contains("null") => "null",
        _ if ts_type.starts_with("Record<") => "{}",
        _ if ts_type.ends_with("[]") => "[]",
        _ => "undefined",
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use nc_jarvis_ir::ts::{StoreField, StoreGetter, StoreAction, ActionParam};

    #[test]
    fn full_store() {
        let decl = PiniaStoreDecl {
            name: "executionStore".into(),
            target_path: Some("src/stores/executionStore.ts".into()),
            state: vec![
                StoreField {
                    name: "activeJobId".into(),
                    ts_type: "string | null".into(),
                    default_value: Some("null".into()),
                },
                StoreField {
                    name: "nodeProgressStates".into(),
                    ts_type: "Record<string, NodeProgressState>".into(),
                    default_value: Some("{}".into()),
                },
                StoreField {
                    name: "isExecuting".into(),
                    ts_type: "boolean".into(),
                    default_value: Some("false".into()),
                },
            ],
            getters: vec![
                StoreGetter {
                    name: "hasActiveJob".into(),
                    return_type: "boolean".into(),
                    derives_from: vec!["activeJobId".into()],
                },
                StoreGetter {
                    name: "progressPercent".into(),
                    return_type: "number".into(),
                    derives_from: vec!["nodeProgressStates".into()],
                },
            ],
            actions: vec![
                StoreAction {
                    name: "startExecution".into(),
                    params: vec![ActionParam {
                        name: "jobId".into(),
                        ts_type: "string".into(),
                    }],
                    return_type: Some("void".into()),
                },
                StoreAction {
                    name: "updateProgress".into(),
                    params: vec![
                        ActionParam {
                            name: "nodeId".into(),
                            ts_type: "string".into(),
                        },
                        ActionParam {
                            name: "value".into(),
                            ts_type: "number".into(),
                        },
                    ],
                    return_type: Some("void".into()),
                },
                StoreAction {
                    name: "clearExecution".into(),
                    params: vec![],
                    return_type: Some("void".into()),
                },
            ],
        };

        let code = generate_pinia_store(&decl);

        // Header
        assert!(code.contains("GENERATED BY FORGE - DO NOT EDIT"));
        assert!(code.contains("src/stores/executionStore.ts (pinia-store executionStore)"));

        // Import
        assert!(code.contains("import { defineStore } from 'pinia';"));

        // Store definition
        assert!(code.contains("export const useExecutionStoreStore = defineStore('executionStore', {"));

        // State
        assert!(code.contains("state: () => ({"));
        assert!(code.contains("activeJobId: null as string | null,"));
        assert!(code.contains("isExecuting: false as boolean,"));

        // Getters
        assert!(code.contains("hasActiveJob(): boolean {"));
        assert!(code.contains("progressPercent(): number {"));

        // Actions
        assert!(code.contains("startExecution(jobId: string): void { /* TODO */ },"));
        assert!(code.contains("updateProgress(nodeId: string, value: number): void { /* TODO */ },"));
        assert!(code.contains("clearExecution(): void { /* TODO */ },"));

        // Closing
        assert!(code.contains("});"));
    }

    #[test]
    fn minimal_store() {
        let decl = PiniaStoreDecl {
            name: "counterStore".into(),
            target_path: None,
            state: vec![StoreField {
                name: "count".into(),
                ts_type: "number".into(),
                default_value: Some("0".into()),
            }],
            getters: vec![],
            actions: vec![],
        };

        let code = generate_pinia_store(&decl);
        assert!(code.contains("useCounterStoreStore"));
        assert!(code.contains("state: () => ({"));
        assert!(code.contains("count: 0 as number,"));
        assert!(!code.contains("getters:"));
        assert!(!code.contains("actions:"));
    }

    #[test]
    fn action_without_return_type() {
        let decl = PiniaStoreDecl {
            name: "testStore".into(),
            target_path: None,
            state: vec![],
            getters: vec![],
            actions: vec![StoreAction {
                name: "doSomething".into(),
                params: vec![],
                return_type: None,
            }],
        };

        let code = generate_pinia_store(&decl);
        assert!(code.contains("doSomething(): void { /* TODO */ },"));
    }

    #[test]
    fn capitalize_first_works() {
        assert_eq!(capitalize_first("execution"), "Execution");
        assert_eq!(capitalize_first("a"), "A");
        assert_eq!(capitalize_first(""), "");
    }
}
