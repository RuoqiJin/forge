# Forge

**A formal architecture descriptor toolkit for AI coding agents.**

Reduce your AI coding agent's navigation steps by 55% with a single architecture file.

---

## Overview

Forge generates, validates, and consumes **formal architecture descriptors** -- structured files called `intent.lisp` that capture a project's architectural topology in a machine-parseable format. When injected into an AI coding agent's context, these descriptors eliminate the exploratory file-walking and grep cycles that dominate agent time budgets.

- **`forge survey`** -- Scans a codebase via tree-sitter AST extraction and generates a layered `intent.lisp` descriptor using an LLM-assisted pipeline.
- **`forge build`** -- Reads `.missiond/*.lisp` declarations and generates compilable source code (Rust, TypeScript, Python) through a deterministic IR-to-codegen pipeline.
- **`forge check` / `forge lint`** -- Validates descriptor structure, enforces granularity level constraints, and detects governance violations without writing any files.

## Quick Start

```bash
cargo install forge-cli
forge survey /path/to/your/project
```

This produces a `.jarvis/intent.lisp` file describing your project's architecture. Add it to your AI agent's context window to reduce navigation overhead.

## How It Works

Forge operates on a three-level granularity hierarchy:

| Level | Scope | Max Lines |
|-------|-------|-----------|
| L1-Blueprint | Service metadata, pillar names, design constraints | 200 |
| L2-Topology | State machines, event flows, dependency graphs | 500 |
| L3-Implementation | Table schemas, API contracts, function signatures | 1500 |

The `forge survey` command scans a codebase using tree-sitter, assembles a structured prompt with the extracted symbols, invokes an LLM (Claude) to produce the descriptor, and validates the output through the built-in S-expression parser.

### Example: Condensed `intent.lisp`

```lisp
(intent my-service
  (version "1.0")
  (granularity L1-Blueprint)

  (design-constraints
    (strict-isolation (core daemon))
    (communication (must-use http-api))
    (db-access (only daemon)))

  (pillar core
    (purpose "Domain logic and data models")
    (component user-store
      (role "CRUD operations for user entities")
      (invariants "No direct SQL outside storage module")
      (data-flow "Request -> validate -> query -> Response")))

  (pillar api
    (purpose "HTTP handlers and request routing")
    (component router
      (role "Axum route registration and middleware"))))
```

The format is S-expressions. This is a deliberate choice for parseability and token compression, not because LLMs prefer S-expressions over YAML, JSON, or any other format. Our experimental results show **format does not significantly affect LLM comprehension** -- the value is in the formalization itself.

## Paper

> **Formal Architecture Descriptors as Navigation Primitives for AI Coding Agents**
> [arXiv:2604.13108](https://arxiv.org/abs/2604.13108) · ICSE 2027 NIER (submitted)

**Key finding:** Architecture context reduces AI coding agent navigation steps by 55% (Cohen's d = 1.70). The effect is attributable to formalization, not format choice -- S-expressions, YAML, and JSON descriptors produce statistically equivalent improvements in agent performance. S-expressions are chosen here for their parseability and superior token-to-information density, not for any LLM-intrinsic advantage.

This repository is the research artifact for the paper. It contains the descriptor toolkit, the survey prompt (used for LLM-assisted descriptor generation), and the complete IR/codegen pipeline evaluated in the study.

## Crate Overview

| Crate | Role |
|-------|------|
| `forge-core` | Core library: S-expression parser, tree-sitter AST scanner, reality mirror, intent graph, governance lint engine, survey pipeline, universe graph, cascade repair orchestrator |
| `forge-cli` | CLI binary (`forge`): stateless compiler frontend dispatching `build`, `check`, `lint`, `survey`, `audit`, `cascade`, and universe commands |
| `forge-daemon` | HTTP API server for the Code Map visualization UI (Axum + PostgreSQL) |
| `nc-jarvis-ir` | Typed intermediate representation: validates S-expression declarations into target-specific IR structs (Rust, TypeScript, Python) |
| `nc-jarvis-gen` | Code generators: IR declarations to compilable source via a three-layer architecture (quarks, atoms, patterns) |

### Three-Layer Codegen Architecture

Code generation follows a strict compositional hierarchy:

- **Quarks (L0)** -- Minimal code fragments (a single field, a match arm, an import line).
- **Atoms (L1)** -- One complete construct (a struct definition, a function body, a Zod schema).
- **Patterns (L2)** -- A complete file assembled from atoms (a CRUD gateway module, a state machine, a Vue SFC).

L2 pattern files must compose from L1 atoms; direct struct/enum/impl generation at the pattern level is rejected by a CI guardrail.

### Generation Gap

Every generated file is split into a `generated.rs` (frozen, overwritten on each build) and a `custom.rs` (hand-written, never touched by Forge). This separation ensures that re-running `forge build` never destroys manual customizations.

## CLI Commands

```
forge survey [PROJECT_ROOT] [--level L1|L2|L3] [--check] [--dry-run]
forge build  [PROJECT_ROOT] [--dry-run] [--output-dir PATH]
forge check  [PROJECT_ROOT]
forge lint   [PROJECT_ROOT]
forge audit  [PROJECT_ROOT] [--json]
forge cascade MANIFEST --service ID --changed iface1,iface2 [--max-cycles N] [--dry-run]
forge universe MANIFEST [--format text|json]
forge blast-radius MANIFEST --service ID --changed iface1,iface2
forge lint-universe MANIFEST
forge distill LOG_DIR [--output FILE] [--format text|json]
forge evolve  LOG_DIR [--output FILE] [--min-utility 0.3] [--format text|json]
```

## Building from Source

```bash
git clone https://github.com/RuoqiJin/jarvis-forge.git
cd jarvis-forge
cargo build --release
```

The CLI binary is at `target/release/forge`.

## Repository Structure

```
crates/
  forge-core/       # Core library
  forge-cli/        # CLI binary
  forge-daemon/     # HTTP API server
  nc-jarvis-ir/     # Typed IR layer
  nc-jarvis-gen/    # Code generators (Rust, TypeScript, Python)
prompts/
  cartographer.md   # LLM prompt for survey descriptor generation
.jarvis/
  intent.lisp       # This project's own architecture descriptor (698 lines)
```

## License

MIT
