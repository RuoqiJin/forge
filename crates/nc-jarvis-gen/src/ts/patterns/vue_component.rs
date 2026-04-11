//! Vue Component code generator (Layer 2 Pattern).
//!
//! Composes atomic codegen functions to generate a Vue SFC skeleton:
//! - `<script setup lang="ts">` with typed props, withDefaults, defineEmits
//! - `<template>` with named slots and a TODO placeholder
//!
//! ## Layer 2 Principle
//!
//! This file delegates interface generation to the `ts_interface` atom
//! and assembles the results into a complete `.vue` file.

use nc_jarvis_ir::ts::VueComponentDecl;

use crate::common::{w, forge_header, blank};
use crate::ts::atoms::ts_interface::{gen_ts_interface, InterfaceField};

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub fn generate_vue_component(decl: &VueComponentDecl) -> String {
    let mut out = String::with_capacity(4096);

    let source = decl
        .target_path
        .as_deref()
        .unwrap_or("intent-frontend.lisp");
    forge_header(&mut out, "ts", &format!("{source} (vue-component {})", decl.name));
    blank(&mut out);

    gen_script_setup(&mut out, decl);
    blank(&mut out);
    gen_template(&mut out, decl);

    out
}

// ---------------------------------------------------------------------------
// <script setup lang="ts">
// ---------------------------------------------------------------------------

fn gen_script_setup(out: &mut String, decl: &VueComponentDecl) {
    w(out, 0, "<script setup lang=\"ts\">");

    // Store imports
    if !decl.stores.is_empty() {
        for store in &decl.stores {
            w(out, 0, &format!(
                "import {{ {} }} from '{}';",
                store.name, store.import_path
            ));
        }
        blank(out);
    }

    // Store instantiation
    if !decl.stores.is_empty() {
        for store in &decl.stores {
            // useSettingStore → settingStore
            let var_name = store.name
                .strip_prefix("use")
                .map(|s| {
                    let mut c = s.chars();
                    match c.next() {
                        Some(ch) => ch.to_lowercase().collect::<String>() + c.as_str(),
                        None => String::new(),
                    }
                })
                .unwrap_or_else(|| store.name.clone());
            w(out, 0, &format!("const {var_name} = {}();", store.name));
        }
        blank(out);
    }

    // defineModel
    if let Some(ref model_type) = decl.model {
        w(out, 0, &format!("const modelValue = defineModel<{model_type}>();"));
        blank(out);
    }

    // Props interface via atom
    if !decl.props.is_empty() {
        let fields: Vec<InterfaceField> = decl
            .props
            .iter()
            .map(|p| InterfaceField {
                name: p.name.clone(),
                ts_type: p.ts_type.clone(),
                optional: !p.required,
                description: p.description.clone(),
            })
            .collect();

        let interface_name = format!("{}Props", decl.name);
        let interface_code = gen_ts_interface(&interface_name, &fields, false);
        out.push_str(&interface_code);
        blank(out);

        // withDefaults + defineProps
        let has_defaults = decl.props.iter().any(|p| p.default_value.is_some());
        if has_defaults {
            w(out, 0, &format!(
                "const props = withDefaults(defineProps<{interface_name}>(), {{"
            ));
            for prop in &decl.props {
                if let Some(ref default_val) = prop.default_value {
                    w(out, 1, &format!("{}: {},", prop.name, default_val));
                }
            }
            w(out, 0, "});");
        } else {
            w(out, 0, &format!("const props = defineProps<{interface_name}>();"));
        }
    }

    // defineEmits
    if !decl.emits.is_empty() {
        blank(out);
        w(out, 0, "const emit = defineEmits<{");
        for emit in &decl.emits {
            match &emit.payload {
                Some(payload) => {
                    w(out, 1, &format!("'{}': [payload: {}];", emit.name, payload));
                }
                None => {
                    w(out, 1, &format!("'{}': [];", emit.name));
                }
            }
        }
        w(out, 0, "}>();");
    }

    w(out, 0, "</script>");
}

// ---------------------------------------------------------------------------
// <template>
// ---------------------------------------------------------------------------

fn gen_template(out: &mut String, decl: &VueComponentDecl) {
    // Convert PascalCase to kebab-case for the CSS class
    let kebab = to_kebab_case(&decl.name);

    w(out, 0, "<template>");
    w(out, 1, &format!("<div class=\"{kebab}\">"));

    if decl.slots.is_empty() {
        w(out, 2, "<!-- TODO: implement component body -->");
    } else {
        for (i, slot) in decl.slots.iter().enumerate() {
            if slot.name == "default" {
                w(out, 2, "<slot />");
            } else {
                w(out, 2, &format!("<slot name=\"{}\" />", slot.name));
            }
            if i == 0 && decl.slots.len() > 1 {
                w(out, 2, "<!-- TODO: implement component body -->");
            }
        }
    }

    w(out, 1, "</div>");
    w(out, 0, "</template>");
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

use crate::quarks::ident::to_kebab_case;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use nc_jarvis_ir::ts::{ComponentProp, ComponentEmit, ComponentSlot};

    #[test]
    fn full_component() {
        let decl = VueComponentDecl {
            name: "QueuePanel".into(),
            target_path: Some("intent-frontend.lisp".into()),
            props: vec![
                ComponentProp {
                    name: "jobs".into(),
                    ts_type: "unknown[]".into(),
                    required: true,
                    description: None,
                    default_value: None,
                },
                ComponentProp {
                    name: "activeJobId".into(),
                    ts_type: "string".into(),
                    required: false,
                    description: None,
                    default_value: None,
                },
                ComponentProp {
                    name: "maxVisible".into(),
                    ts_type: "number".into(),
                    required: false,
                    description: Some("Max items to show".into()),
                    default_value: Some("50".into()),
                },
            ],
            emits: vec![
                ComponentEmit {
                    name: "job-selected".into(),
                    payload: Some("{ jobId: string }".into()),
                },
                ComponentEmit {
                    name: "queue-cleared".into(),
                    payload: None,
                },
            ],
            slots: vec![
                ComponentSlot { name: "header".into(), description: None },
                ComponentSlot { name: "empty-state".into(), description: None },
            ],
            stores: vec![],
            model: None,
        };

        let code = generate_vue_component(&decl);

        // Header
        assert!(code.contains("GENERATED BY FORGE - DO NOT EDIT"));
        assert!(code.contains("intent-frontend.lisp (vue-component QueuePanel)"));

        // Script setup
        assert!(code.contains("<script setup lang=\"ts\">"));
        assert!(code.contains("interface QueuePanelProps {"));
        assert!(code.contains("    jobs: unknown[];"));
        assert!(code.contains("    activeJobId?: string;"));
        assert!(code.contains("    /** Max items to show */"));
        assert!(code.contains("    maxVisible?: number;"));
        assert!(code.contains("withDefaults(defineProps<QueuePanelProps>(), {"));
        assert!(code.contains("    maxVisible: 50,"));

        // Emits
        assert!(code.contains("const emit = defineEmits<{"));
        assert!(code.contains("    'job-selected': [payload: { jobId: string }];"));
        assert!(code.contains("    'queue-cleared': [];"));

        // Template
        assert!(code.contains("<div class=\"queue-panel\">"));
        assert!(code.contains("<slot name=\"header\" />"));
        assert!(code.contains("<slot name=\"empty-state\" />"));
        assert!(code.contains("</script>"));
        assert!(code.contains("</template>"));
    }

    #[test]
    fn minimal_component() {
        let decl = VueComponentDecl {
            name: "EmptyWidget".into(),
            target_path: None,
            props: vec![],
            emits: vec![],
            slots: vec![],
            stores: vec![],
            model: None,
        };

        let code = generate_vue_component(&decl);
        assert!(code.contains("<script setup lang=\"ts\">"));
        assert!(code.contains("</script>"));
        assert!(code.contains("<div class=\"empty-widget\">"));
        assert!(code.contains("<!-- TODO: implement component body -->"));
    }

    #[test]
    fn to_kebab_case_conversion() {
        assert_eq!(to_kebab_case("QueuePanel"), "queue-panel");
        assert_eq!(to_kebab_case("NodeProgressBar"), "node-progress-bar");
        assert_eq!(to_kebab_case("A"), "a");
    }
}
