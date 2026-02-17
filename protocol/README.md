# protocol/

Nex_Alignment fork and MCP (Model Context Protocol) assets for Kaizen MAX.

## Purpose
This directory holds:

1. **Alignment rules** - Nex_Alignment tenets injected into system prompts and agent templates.
2. **MCP tool definitions** - Tool schemas for the ZeroClaw gateway.
3. **Collision matrix** - Mapping of ZeroClaw native gates vs. Nex_Alignment MCP tools (Phase B deliverable).

## Integration Strategy (from Section 9 of implementation_plan.md)

1. Inventory ZeroClaw native review/approval tools.
2. Inventory Nex_Alignment MCP tools.
3. Build collision matrix.
4. If ZeroClaw has equivalent native gates -> deprecate overlapping MCP tools.
5. If missing -> bridge Nex MCP tools into gateway/tool layer.
6. If still missing -> implement optional adapters in `compat/`.

## Directory Structure
```
protocol/
  mcp/              # MCP tool definitions and schemas
  alignment/        # Nex_Alignment rules and tenets
  collision_matrix/  # ZeroClaw vs Nex comparison (Phase B)
```
