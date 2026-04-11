//! # Quarks (Layer 0) — Indivisible Semantic Primitives
//!
//! Quarks are the smallest semantic building blocks in the Forge code generator.
//! They return string fragments or type signatures — never compilable code units.
//!
//! ## Design Principles
//!
//! 1. **Cross-language** — quarks sit above `rust/` and `ts/`, usable by both
//! 2. **Pure functions** — no state, no side effects, deterministic
//! 3. **Non-compilable** — a quark output is a fragment, not valid code
//! 4. **Semantic boundary** — quarks express types and naming, not syntax tokens
//!
//! ## Quarks vs Atoms vs Molecules
//!
//! | Layer | Unit | Example | Compilable? |
//! |-------|------|---------|-------------|
//! | Quark | type_map, ident | `"i64"`, `"MyModule"` | No |
//! | Atom | struct_def, enum_def | `pub struct Foo { ... }` | Yes |
//! | Pattern | mcp_tool, crud | Complete `.rs` file | Yes |

pub mod type_map;
pub mod ident;
pub mod doc_comment;
pub mod derive_attr;
