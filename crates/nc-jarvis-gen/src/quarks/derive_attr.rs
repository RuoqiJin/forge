//! Quark: derive-attr
//!
//! Language-specific attribute/annotation generation.
//! Rust: `#[derive(...)]`, `#[serde(...)]`.
//! TypeScript: decorators (future).
//!
//! Moved from `struct_def::DeriveSet` to enable cross-atom reuse.

/// A set of derive traits to apply to a Rust struct/enum.
#[derive(Debug, Clone)]
pub struct DeriveSet {
    pub debug: bool,
    pub clone: bool,
    pub serialize: bool,
    pub deserialize: bool,
    pub partial_eq: bool,
    pub custom: Vec<String>,
}

impl Default for DeriveSet {
    fn default() -> Self {
        Self {
            debug: true,
            clone: true,
            serialize: false,
            deserialize: false,
            partial_eq: false,
            custom: vec![],
        }
    }
}

impl DeriveSet {
    /// Preset for serde input structs (API arg extraction).
    pub fn deserialize_input() -> Self {
        Self {
            debug: true,
            clone: true,
            serialize: false,
            deserialize: true,
            partial_eq: false,
            custom: vec![],
        }
    }

    /// Preset for serde output structs (API response).
    pub fn serialize_output() -> Self {
        Self {
            debug: true,
            clone: true,
            serialize: true,
            deserialize: false,
            partial_eq: false,
            custom: vec![],
        }
    }

    /// Preset for serde bidirectional (stored in DB, sent over wire).
    pub fn serde_both() -> Self {
        Self {
            debug: true,
            clone: true,
            serialize: true,
            deserialize: true,
            partial_eq: false,
            custom: vec![],
        }
    }

    /// Generate the `#[derive(...)]` line. Returns empty string if no traits.
    pub fn to_derive_line(&self) -> String {
        let mut traits = Vec::new();
        if self.debug {
            traits.push("Debug");
        }
        if self.clone {
            traits.push("Clone");
        }
        if self.serialize {
            traits.push("Serialize");
        }
        if self.deserialize {
            traits.push("Deserialize");
        }
        if self.partial_eq {
            traits.push("PartialEq");
        }
        for c in &self.custom {
            traits.push(c);
        }
        if traits.is_empty() {
            String::new()
        } else {
            format!("#[derive({})]", traits.join(", "))
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
    fn default_derive() {
        let d = DeriveSet::default();
        assert_eq!(d.to_derive_line(), "#[derive(Debug, Clone)]");
    }

    #[test]
    fn deserialize_input_derive() {
        let d = DeriveSet::deserialize_input();
        assert_eq!(d.to_derive_line(), "#[derive(Debug, Clone, Deserialize)]");
    }

    #[test]
    fn serde_both_derive() {
        let d = DeriveSet::serde_both();
        assert_eq!(d.to_derive_line(), "#[derive(Debug, Clone, Serialize, Deserialize)]");
    }

    #[test]
    fn empty_derive() {
        let d = DeriveSet {
            debug: false,
            clone: false,
            serialize: false,
            deserialize: false,
            partial_eq: false,
            custom: vec![],
        };
        assert_eq!(d.to_derive_line(), "");
    }

    #[test]
    fn custom_derive() {
        let d = DeriveSet {
            debug: true,
            clone: false,
            serialize: false,
            deserialize: false,
            partial_eq: false,
            custom: vec!["Hash".into(), "Eq".into()],
        };
        assert_eq!(d.to_derive_line(), "#[derive(Debug, Hash, Eq)]");
    }
}
