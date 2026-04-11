//! Quark: ident
//!
//! Identifier case conversion for code generation.
//! Pure functions operating on `&str` → `String`.
//!
//! Consolidates 6+ duplicated `to_pascal_case`/`pascal_case` implementations
//! previously scattered across patterns and IR modules.

/// Convert `"my-module"` or `"my_module"` to `"MyModule"`.
///
/// Splits on `-` and `_`, capitalizes each part's first character.
pub fn to_pascal_case(s: &str) -> String {
    s.split(|c: char| c == '-' || c == '_')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(c) => {
                    let upper: String = c.to_uppercase().collect();
                    upper + chars.as_str()
                }
                None => String::new(),
            }
        })
        .collect()
}

/// Convert `"MyModule"` or `"SSEStream"` to `"my_module"` or `"s_s_e_stream"`.
///
/// Inserts `_` before each uppercase letter (except the first character).
pub fn to_snake_case(s: &str) -> String {
    let mut result = String::with_capacity(s.len() + 4);
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            result.push('_');
        }
        result.push(c.to_lowercase().next().unwrap_or(c));
    }
    result
}

/// Convert `"my-module"` or `"my_module"` to `"myModule"`.
///
/// Like `to_pascal_case` but the first character stays lowercase.
pub fn to_camel_case(s: &str) -> String {
    let pascal = to_pascal_case(s);
    let mut chars = pascal.chars();
    match chars.next() {
        Some(c) => {
            let lower: String = c.to_lowercase().collect();
            lower + chars.as_str()
        }
        None => String::new(),
    }
}

/// Convert `"MyModule"` to `"my-module"`.
///
/// Inserts `-` before each uppercase letter (except the first) and lowercases all.
pub fn to_kebab_case(s: &str) -> String {
    let mut result = String::with_capacity(s.len() + 4);
    for (i, ch) in s.chars().enumerate() {
        if ch.is_uppercase() {
            if i > 0 {
                result.push('-');
            }
            result.push(ch.to_lowercase().next().unwrap());
        } else {
            result.push(ch);
        }
    }
    result
}

/// Capitalize the first character only, no word splitting.
///
/// `"execution"` → `"Execution"`, `"a"` → `"A"`, `""` → `""`.
pub fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// Apply a prefix swap: `"jarvis_board_delete"` with `("jarvis_", "execute_")` → `"execute_board_delete"`.
///
/// If the string doesn't start with `from`, returns the original unchanged.
pub fn swap_prefix(s: &str, from: &str, to: &str) -> String {
    if let Some(rest) = s.strip_prefix(from) {
        format!("{to}{rest}")
    } else {
        s.to_string()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pascal_case_from_kebab() {
        assert_eq!(to_pascal_case("semantic-helpers"), "SemanticHelpers");
        assert_eq!(to_pascal_case("math"), "Math");
        assert_eq!(to_pascal_case("multi-word-name"), "MultiWordName");
    }

    #[test]
    fn pascal_case_from_snake() {
        assert_eq!(to_pascal_case("my_module"), "MyModule");
        assert_eq!(to_pascal_case("deploy_status"), "DeployStatus");
    }

    #[test]
    fn pascal_case_edge_cases() {
        assert_eq!(to_pascal_case(""), "");
        assert_eq!(to_pascal_case("a"), "A");
        assert_eq!(to_pascal_case("-"), "");
    }

    #[test]
    fn snake_case_conversion() {
        assert_eq!(to_snake_case("DeployStatus"), "deploy_status");
        assert_eq!(to_snake_case("Pending"), "pending");
        assert_eq!(to_snake_case("LostContact"), "lost_contact");
        assert_eq!(to_snake_case("SSEStream"), "s_s_e_stream");
    }

    #[test]
    fn camel_case_conversion() {
        assert_eq!(to_camel_case("my-module"), "myModule");
        assert_eq!(to_camel_case("deploy_status"), "deployStatus");
        assert_eq!(to_camel_case("a"), "a");
        assert_eq!(to_camel_case(""), "");
    }

    #[test]
    fn kebab_case_conversion() {
        assert_eq!(to_kebab_case("QueuePanel"), "queue-panel");
        assert_eq!(to_kebab_case("NodeProgressBar"), "node-progress-bar");
        assert_eq!(to_kebab_case("A"), "a");
    }

    #[test]
    fn capitalize_first_works() {
        assert_eq!(capitalize_first("execution"), "Execution");
        assert_eq!(capitalize_first("a"), "A");
        assert_eq!(capitalize_first(""), "");
    }

    #[test]
    fn prefix_swap() {
        assert_eq!(swap_prefix("jarvis_board_delete", "jarvis_", "execute_"), "execute_board_delete");
        assert_eq!(swap_prefix("mission_kb_query", "mission_", "execute_"), "execute_kb_query");
        assert_eq!(swap_prefix("no_match", "jarvis_", "execute_"), "no_match");
    }
}
