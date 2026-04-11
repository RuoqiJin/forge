//! Atom: py-class-def
//!
//! Generates a Python class definition with class attributes.
//! Used for ComfyUI `_ForgeBase` node classes (inheritance isolation).

use crate::common::w;
use crate::quarks::doc_comment::{DocLang, doc_line};

pub struct PyClassDefInput {
    pub class_name: String,
    pub base_class: String,
    pub docstring: Option<String>,
    pub attrs: Vec<(String, String)>,
}

/// Generate a Python class definition with class-level attributes.
pub fn gen_py_class_def(input: &PyClassDefInput) -> String {
    let mut out = String::with_capacity(512);

    w(
        &mut out,
        0,
        &format!("class {}({}):", input.class_name, input.base_class),
    );

    if let Some(ref doc) = input.docstring {
        w(&mut out, 1, &doc_line(DocLang::Python, doc));
    }

    // Blank line after docstring
    if input.docstring.is_some() {
        out.push('\n');
    }

    for (name, value) in &input.attrs {
        w(&mut out, 1, &format!("{name} = {value}"));
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
    fn basic_class() {
        let code = gen_py_class_def(&PyClassDefInput {
            class_name: "CLIPTextEncode_ForgeBase".into(),
            base_class: "ComfyNodeABC".into(),
            docstring: Some("Encodes text using CLIP.".into()),
            attrs: vec![
                ("CATEGORY".into(), "\"conditioning\"".into()),
                ("FUNCTION".into(), "\"encode\"".into()),
            ],
        });

        assert!(code.contains("class CLIPTextEncode_ForgeBase(ComfyNodeABC):"));
        assert!(code.contains("    \"\"\"Encodes text using CLIP.\"\"\""));
        assert!(code.contains("    CATEGORY = \"conditioning\""));
        assert!(code.contains("    FUNCTION = \"encode\""));
    }

    #[test]
    fn no_docstring() {
        let code = gen_py_class_def(&PyClassDefInput {
            class_name: "Test_ForgeBase".into(),
            base_class: "ComfyNodeABC".into(),
            docstring: None,
            attrs: vec![("CATEGORY".into(), "\"test\"".into())],
        });

        assert!(!code.contains("\"\"\""));
        assert!(code.contains("    CATEGORY = \"test\""));
    }
}
