//! Atom: arg-extract
//!
//! Generates type-safe argument extraction from `serde_json::Value`.
//! Replaces hand-written `args.get("x").and_then(|v| v.as_str())` boilerplate.
//!
//! ## What it generates
//!
//! ```text
//! let category = args.get("category")
//!     .and_then(|v| v.as_str())
//!     .ok_or_else(|| anyhow!("missing required field: category"))?
//!     .to_string();
//!
//! let limit: i64 = args.get("limit")
//!     .and_then(|v| v.as_i64())
//!     .unwrap_or(10);
//! ```
//!
//! ## Pairs naturally with `tool-schema`
//!
//! Same Lisp declaration produces both the JSON Schema (via tool-schema atom)
//! and the extraction code (via this atom). One declaration, two outputs.

use super::w;
use super::tool_schema::{JsonType, ToolParam};

/// Input to the arg-extract atom.
pub struct ArgExtractInput {
    /// Variable name holding the `serde_json::Value` args.
    pub args_var: String,
    /// Parameters to extract.
    pub params: Vec<ToolParam>,
}

/// Generate argument extraction code from a `Value`.
///
/// Each param becomes a `let name: Type = ...;` binding.
/// Required params use `?` (fail-fast). Optional params use `unwrap_or`.
pub fn gen_arg_extract(input: &ArgExtractInput) -> String {
    let mut out = String::with_capacity(512);
    let args = &input.args_var;

    for param in &input.params {
        let extractor = param.json_type.serde_extractor();
        let rust_type = param.json_type.rust_owned_type();

        if param.required {
            // Required: fail-fast with descriptive error
            match param.json_type {
                JsonType::String => {
                    w(
                        &mut out,
                        0,
                        &format!(
                            "let {name} = {args}.get(\"{name}\").and_then(|v| v.{extractor}).ok_or_else(|| anyhow::anyhow!(\"missing required field: {name}\"))?.to_string();",
                            name = param.name, args = args, extractor = extractor
                        ),
                    );
                }
                JsonType::Integer | JsonType::Number | JsonType::Boolean => {
                    w(
                        &mut out,
                        0,
                        &format!(
                            "let {name}: {rust_type} = {args}.get(\"{name}\").and_then(|v| v.{extractor}).ok_or_else(|| anyhow::anyhow!(\"missing required field: {name}\"))?;",
                            name = param.name, rust_type = rust_type, args = args, extractor = extractor
                        ),
                    );
                }
                JsonType::Array | JsonType::Object => {
                    // Reference types — clone to owned
                    w(
                        &mut out,
                        0,
                        &format!(
                            "let {name} = {args}.get(\"{name}\").and_then(|v| v.{extractor}).ok_or_else(|| anyhow::anyhow!(\"missing required field: {name}\"))?.clone();",
                            name = param.name, args = args, extractor = extractor
                        ),
                    );
                }
            }
        } else {
            // Optional: with or without default
            match (&param.json_type, &param.default) {
                (JsonType::String, Some(default)) => {
                    w(
                        &mut out,
                        0,
                        &format!(
                            "let {name} = {args}.get(\"{name}\").and_then(|v| v.as_str()).unwrap_or(\"{default}\").to_string();",
                            name = param.name, args = args, default = default
                        ),
                    );
                }
                (JsonType::String, None) => {
                    w(
                        &mut out,
                        0,
                        &format!(
                            "let {name}: Option<String> = {args}.get(\"{name}\").and_then(|v| v.as_str()).map(String::from);",
                            name = param.name, args = args
                        ),
                    );
                }
                (JsonType::Integer, Some(default)) => {
                    w(
                        &mut out,
                        0,
                        &format!(
                            "let {name}: i64 = {args}.get(\"{name}\").and_then(|v| v.as_i64()).unwrap_or({default});",
                            name = param.name, args = args, default = default
                        ),
                    );
                }
                (JsonType::Integer, None) => {
                    w(
                        &mut out,
                        0,
                        &format!(
                            "let {name}: Option<i64> = {args}.get(\"{name}\").and_then(|v| v.as_i64());",
                            name = param.name, args = args
                        ),
                    );
                }
                (JsonType::Number, Some(default)) => {
                    w(
                        &mut out,
                        0,
                        &format!(
                            "let {name}: f64 = {args}.get(\"{name}\").and_then(|v| v.as_f64()).unwrap_or({default});",
                            name = param.name, args = args, default = default
                        ),
                    );
                }
                (JsonType::Number, None) => {
                    w(
                        &mut out,
                        0,
                        &format!(
                            "let {name}: Option<f64> = {args}.get(\"{name}\").and_then(|v| v.as_f64());",
                            name = param.name, args = args
                        ),
                    );
                }
                (JsonType::Boolean, Some(default)) => {
                    w(
                        &mut out,
                        0,
                        &format!(
                            "let {name}: bool = {args}.get(\"{name}\").and_then(|v| v.as_bool()).unwrap_or({default});",
                            name = param.name, args = args, default = default
                        ),
                    );
                }
                (JsonType::Boolean, None) => {
                    w(
                        &mut out,
                        0,
                        &format!(
                            "let {name}: Option<bool> = {args}.get(\"{name}\").and_then(|v| v.as_bool());",
                            name = param.name, args = args
                        ),
                    );
                }
                (JsonType::Array | JsonType::Object, _) => {
                    w(
                        &mut out,
                        0,
                        &format!(
                            "let {name} = {args}.get(\"{name}\").cloned().unwrap_or(serde_json::Value::Null);",
                            name = param.name, args = args
                        ),
                    );
                }
            }
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
    fn required_string() {
        let input = ArgExtractInput {
            args_var: "args".into(),
            params: vec![ToolParam {
                name: "key".into(),
                json_type: JsonType::String,
                required: true,
                description: None,
                enum_values: vec![],
                default: None,
            }],
        };

        let code = gen_arg_extract(&input);
        assert!(code.contains("let key = args.get(\"key\").and_then(|v| v.as_str()).ok_or_else("));
        assert!(code.contains("missing required field: key"));
        assert!(code.contains(".to_string()"));
    }

    #[test]
    fn optional_with_default() {
        let input = ArgExtractInput {
            args_var: "args".into(),
            params: vec![ToolParam {
                name: "limit".into(),
                json_type: JsonType::Integer,
                required: false,
                description: None,
                enum_values: vec![],
                default: Some("10".into()),
            }],
        };

        let code = gen_arg_extract(&input);
        assert!(code.contains("let limit: i64 = args.get(\"limit\").and_then(|v| v.as_i64()).unwrap_or(10)"));
    }

    #[test]
    fn optional_no_default() {
        let input = ArgExtractInput {
            args_var: "params".into(),
            params: vec![ToolParam {
                name: "query".into(),
                json_type: JsonType::String,
                required: false,
                description: None,
                enum_values: vec![],
                default: None,
            }],
        };

        let code = gen_arg_extract(&input);
        assert!(code.contains("let query: Option<String> = params.get(\"query\").and_then(|v| v.as_str()).map(String::from)"));
    }

    #[test]
    fn mixed_required_and_optional() {
        let input = ArgExtractInput {
            args_var: "args".into(),
            params: vec![
                ToolParam { name: "action".into(), json_type: JsonType::String, required: true, description: None, enum_values: vec![], default: None },
                ToolParam { name: "id".into(), json_type: JsonType::String, required: false, description: None, enum_values: vec![], default: None },
                ToolParam { name: "verbose".into(), json_type: JsonType::Boolean, required: false, description: None, enum_values: vec![], default: Some("false".into()) },
            ],
        };

        let code = gen_arg_extract(&input);
        assert!(code.contains("let action = args.get(\"action\")"));
        assert!(code.contains("ok_or_else")); // required
        assert!(code.contains("let id: Option<String>")); // optional no default
        assert!(code.contains("let verbose: bool")); // optional with default
        assert!(code.contains("unwrap_or(false)"));
    }
}
