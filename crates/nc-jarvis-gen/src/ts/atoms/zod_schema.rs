//! Atom: zod-schema
//!
//! Generates a Zod runtime validation schema definition.
//!
//! ## What it generates
//!
//! ```typescript
//! export const QueuePanelPropsSchema = z.object({
//!     jobs: z.array(z.unknown()),
//!     activeJobId: z.string().optional(),
//!     maxVisible: z.number().default(50),
//! });
//! ```
//!
//! ## What it does NOT generate
//!
//! - No import statements for `z` (caller adds `import { z } from 'zod'`)
//! - No TypeScript interfaces (that's `ts_interface`)
//! - No `z.infer<>` type aliases (caller composes if needed)

use crate::common::w;

/// A single field in a Zod object schema.
#[derive(Debug, Clone)]
pub struct ZodField {
    pub name: String,
    /// Zod type expression, e.g. "z.string()", "z.number()", "z.array(z.unknown())".
    pub zod_type: String,
    pub optional: bool,
    pub default_value: Option<String>,
    pub min: Option<f64>,
    pub max: Option<f64>,
}

/// Generate a Zod schema definition.
///
/// Returns the generated code as a String fragment.
pub fn gen_zod_schema(name: &str, fields: &[ZodField]) -> String {
    let mut out = String::with_capacity(512);

    w(&mut out, 0, &format!("export const {name}Schema = z.object({{"));

    for field in fields {
        let mut chain = field.zod_type.clone();

        // Append .min()/.max() constraints
        if let Some(min) = field.min {
            chain.push_str(&format!(".min({min})"));
        }
        if let Some(max) = field.max {
            chain.push_str(&format!(".max({max})"));
        }

        // Append .optional() or .default()
        if let Some(ref default_val) = field.default_value {
            chain.push_str(&format!(".default({default_val})"));
        } else if field.optional {
            chain.push_str(".optional()");
        }

        w(&mut out, 1, &format!("{}: {},", field.name, chain));
    }

    w(&mut out, 0, "});");
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_schema() {
        let fields = vec![
            ZodField {
                name: "jobs".into(),
                zod_type: "z.array(z.unknown())".into(),
                optional: false,
                default_value: None,
                min: None,
                max: None,
            },
            ZodField {
                name: "activeJobId".into(),
                zod_type: "z.string()".into(),
                optional: true,
                default_value: None,
                min: None,
                max: None,
            },
            ZodField {
                name: "maxVisible".into(),
                zod_type: "z.number()".into(),
                optional: false,
                default_value: Some("50".into()),
                min: None,
                max: None,
            },
        ];

        let code = gen_zod_schema("QueuePanelProps", &fields);

        assert!(code.contains("export const QueuePanelPropsSchema = z.object({"));
        assert!(code.contains("    jobs: z.array(z.unknown()),"));
        assert!(code.contains("    activeJobId: z.string().optional(),"));
        assert!(code.contains("    maxVisible: z.number().default(50),"));
        assert!(code.contains("});"));
    }

    #[test]
    fn schema_with_constraints() {
        let fields = vec![ZodField {
            name: "steps".into(),
            zod_type: "z.number()".into(),
            optional: false,
            default_value: Some("20".into()),
            min: Some(1.0),
            max: Some(10000.0),
        }];

        let code = gen_zod_schema("SamplerConfig", &fields);
        assert!(code.contains("z.number().min(1).max(10000).default(20)"));
    }

    #[test]
    fn empty_schema() {
        let code = gen_zod_schema("EmptyConfig", &[]);
        assert!(code.contains("export const EmptyConfigSchema = z.object({"));
        assert!(code.contains("});"));
    }
}
