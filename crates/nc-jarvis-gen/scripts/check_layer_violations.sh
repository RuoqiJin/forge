#!/usr/bin/env bash
# Architecture guardrail: enforce L2 → L1 atom composition
# Pattern files must not directly generate struct/enum/impl via w() calls.
# Whitelisted exceptions documented in Gemini audit (2026-04-03).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PATTERNS_DIR="$SCRIPT_DIR/../src/rust/patterns"

if [ ! -d "$PATTERNS_DIR" ]; then
    echo "ERROR: patterns directory not found: $PATTERNS_DIR"
    exit 1
fi

# Whitelisted files (legitimate exceptions per Gemini audit):
#   error_enum.rs   — thiserror tuple variants not supported by enum_def atom
#   rpc_gateway.rs  — tagged union tuple enums
#   crud.rs         — residual impl shell wrapping SQL methods
#   mcp_tool.rs     — derive line stripping workaround
#   pure_utility.rs — unit struct `pub struct FooImpl;` (semicolon form not in struct_def)
#   crud_rusqlite.rs — residual impl shell wrapping SQLite methods (same as crud.rs)
WHITELIST="error_enum.rs|rpc_gateway.rs|crud.rs|crud_rusqlite.rs|mcp_tool.rs|pure_utility.rs"

# Search for direct struct/enum/impl generation via w() or format!()
violations=$(grep -rn \
    -e 'w(.*"pub struct ' \
    -e 'w(.*"pub enum ' \
    -e 'w(.*"impl ' \
    -e 'format!("pub struct ' \
    -e 'format!("pub enum ' \
    -e 'format!("impl ' \
    "$PATTERNS_DIR" 2>/dev/null \
    | grep -Ev "/($WHITELIST):" \
    || true)

if [ -n "$violations" ]; then
    echo "LAYER VIOLATION: Pattern files must not directly generate struct/enum/impl."
    echo "Use L1 atoms (struct_def, enum_def, impl_block) instead."
    echo ""
    echo "Violations found:"
    echo "$violations"
    echo ""
    echo "To whitelist a file, add it to WHITELIST in this script with a justification."
    exit 1
fi

echo "Layer violation check passed"
exit 0
