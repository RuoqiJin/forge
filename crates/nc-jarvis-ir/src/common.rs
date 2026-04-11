//! Shared IR utilities — language-agnostic helpers for S-EXP extraction.
//!
//! Used by both rust/ and ts/ IR pipelines.

/// Standard IR validation error.
#[derive(Debug, Clone)]
pub struct IrError {
    pub field: String,
    pub message: String,
    pub expected: Option<String>,
    pub got: Option<String>,
}

impl std::fmt::Display for IrError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.field, self.message)?;
        if let Some(ref exp) = self.expected {
            write!(f, " (expected: {exp})")?;
        }
        if let Some(ref got) = self.got {
            write!(f, " (got: {got})")?;
        }
        Ok(())
    }
}

impl std::error::Error for IrError {}
