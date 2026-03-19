#!/usr/bin/env python3
"""
Engram Memory MCP Server — local SQLite + BM25/ONNX memory for Claude Code.

Exposes Engram memory operations as MCP tools. All operations are local
with zero API calls. Typical latency: <100ms per operation.

Usage:
    py ~/.claude/skills/engram-memory-skill/mcp_server.py

Configure in Claude Code MCP settings:
    {
        "mcpServers": {
            "engram": {
                "command": "py",
                "args": ["~/.claude/skills/engram-memory-skill/mcp_server.py"],
                "env": {
                    "ENGRAM_PROJECT_ROOT": "/path/to/project"
                }
            }
        }
    }
"""

import logging
import os
import sys
from pathlib import Path

# Windows UTF-8 encoding — only reconfigure stderr.
# stdout is used by FastMCP's stdio transport in binary mode;
# reconfiguring it to text/UTF-8 breaks JSON-RPC responses.
if sys.platform == "win32":
    sys.stderr.reconfigure(encoding="utf-8")

# Add parent to path so engram_index is importable
sys.path.insert(0, str(Path(__file__).parent))

from mcp.server.fastmcp import FastMCP

logging.basicConfig(level=logging.INFO, format="%(name)s: %(message)s")
logger = logging.getLogger("engram.mcp")

# ─── Server setup ────────────────────────────────────────────────

mcp = FastMCP(
    "engram",
    instructions=(
        "Engram is the 'why' layer in a three-layer knowledge system: "
        "Serena (code context) -> Engram (decisions & reasoning) -> Beacon (documentation). "
        "Use engram_remember to store architectural decisions (type=decision), "
        "bug patterns (type=pattern), and key facts (type=fact). "
        "Use engram_recall to search memories before making decisions. "
        "Use engram_relate to explore entity relationships in the knowledge graph. "
        "Use engram_trace to record reasoning traces for complex tasks. "
        "At session end, distill Serena memories into Engram, then run beacon_reconcile "
        "to detect doc drift. See docs/references/knowledge-sync-protocol.md for full protocol."
    ),
)

# ─── Lazy-loaded memory instance ─────────────────────────────────

_memory = None


def _get_memory():
    """Get or create the EngramMemory instance (loaded once)."""
    global _memory
    if _memory is not None:
        return _memory

    from engram_index.memory import EngramMemory

    project_root = os.environ.get("ENGRAM_PROJECT_ROOT")
    if not project_root:
        raise RuntimeError(
            "ENGRAM_PROJECT_ROOT environment variable not set. "
            "Set it to the project directory path."
        )

    project_path = Path(project_root)
    if not project_path.exists():
        raise RuntimeError(f"Project root does not exist: {project_root}")

    logger.info(f"Initializing Engram for: {project_root}")
    _memory = EngramMemory(project_path)
    logger.info("Engram memory initialized.")
    return _memory


# ─── MCP Tools ───────────────────────────────────────────────────


@mcp.tool()
def engram_remember(
    content: str,
    memory_type: str = "fact",
    scope: str = "project",
) -> str:
    """
    Store a memory in Engram.

    Args:
        content: The memory content (natural language)
        memory_type: One of: fact, decision, pattern, preference, context, trace
        scope: One of: project, session, user
    """
    engram = _get_memory()

    result = engram.remember(content, memory_type=memory_type, scope=scope)

    stored_count = 0
    if isinstance(result, dict) and "results" in result:
        stored_count = len(result["results"])

    return (
        f"Remembered ({memory_type}/{scope}): "
        f"{content[:120]}{'...' if len(content) > 120 else ''}\n"
        f"Stored {stored_count} memory entries."
    )


@mcp.tool()
def engram_recall(
    query: str,
    top_k: int = 5,
    bridge: bool = False,
    since: str = "",
    until: str = "",
) -> str:
    """
    Search Engram memory for relevant context.

    Args:
        query: Natural language search query
        top_k: Number of results to return (default: 5)
        bridge: If true, also search Serena docs (combined results)
        since: ISO 8601 date/datetime filter — only memories created on or after this date (e.g. "2026-02-10")
        until: ISO 8601 date/datetime filter — only memories created on or before this date (e.g. "2026-02-16")
    """
    engram = _get_memory()
    return engram.recall_formatted(query, top_k=top_k, bridge=bridge, since=since, until=until)


@mcp.tool()
def engram_relate(entity: str, max_depth: int = 2) -> str:
    """
    Explore entity relationships in the memory graph.

    Args:
        entity: Entity name to explore
        max_depth: How many relationship hops to traverse (default: 2)
    """
    engram = _get_memory()
    result = engram.relate(entity, max_depth=max_depth)

    parts = [f"## Entity: {entity}\n"]

    if result.get("memories"):
        parts.append("### Related Memories\n")
        for m in result["memories"]:
            score = m.get("score", 0)
            text = m.get("memory", "")[:100]
            parts.append(f"  [{score:.3f}] {text}")

    if result.get("relations"):
        parts.append("\n### Relationships\n")
        for r in result["relations"]:
            if isinstance(r, dict):
                parts.append(
                    f"  {r.get('source', '?')} -> "
                    f"{r.get('relation_type', '?')} -> "
                    f"{r.get('target', '?')}"
                )
            else:
                parts.append(f"  {r}")

    if result.get("entities"):
        parts.append("\n### Connected Entities\n")
        for e in result["entities"]:
            parts.append(
                f"  {e['name']} (depth: {e.get('depth', 0)}, "
                f"memories: {e.get('memory_count', 0)})"
            )

    return "\n".join(parts)


@mcp.tool()
def engram_trace(
    task: str,
    steps: list[str],
    outcome: str = "success",
    tools_used: str = "",
) -> str:
    """
    Store a reasoning trace in Engram.

    Args:
        task: Description of the task
        steps: List of reasoning steps taken
        outcome: One of: success, failure, partial
        tools_used: Comma-separated list of tools used
    """
    engram = _get_memory()

    tools = [t.strip() for t in tools_used.split(",") if t.strip()] if tools_used else []

    engram.trace(
        task=task,
        reasoning=steps,
        outcome=outcome,
        tools_used=tools,
    )

    return f"Trace stored: {task} ({outcome}, {len(steps)} steps)"


@mcp.tool()
def engram_history(
    entity: str,
    since: str = "",
    until: str = "",
    limit: int = 20,
) -> str:
    """
    Get temporal history for an entity — how memories about it evolved over time.

    Args:
        entity: Entity name to trace history for
        since: ISO 8601 date/datetime filter (e.g. "2026-02-10")
        until: ISO 8601 date/datetime filter (e.g. "2026-02-16")
        limit: Max results (default: 20)
    """
    engram = _get_memory()
    results = engram.engram_history(entity, since=since, until=until, limit=limit)

    if not results:
        return f"No history found for entity: {entity}"

    parts = [f"## History: {entity}\n"]
    if since or until:
        range_str = f"{since or '...'} to {until or '...'}"
        parts.append(f"Time range: {range_str}\n")
    parts.append(f"Results: {len(results)}\n")

    for i, r in enumerate(results, 1):
        parts.append(
            f"### {i}. [{r['memory_type']}] {r['created_at']}\n"
            f"{r['content']}\n"
        )

    return "\n".join(parts)


@mcp.tool()
def engram_stats() -> str:
    """Get Engram memory statistics for the current project."""
    engram = _get_memory()
    stats = engram.stats()

    parts = [
        f"# Engram Stats\n",
        f"Project: {stats['project']}",
        f"Memories stored: {stats['memories_stored']}",
        f"Serena integration: {'yes' if stats['serena_integration'] else 'no'}",
        "",
    ]

    if stats.get("memories_by_type"):
        parts.append("Memories by type:")
        for t, count in stats["memories_by_type"].items():
            parts.append(f"  {t}: {count}")
        parts.append("")

    history = stats.get("history", {})
    parts.append(f"Total operations: {history.get('total_operations', 0)}")
    parts.append(f"Total traces: {history.get('total_traces', 0)}")

    if history.get("by_type"):
        parts.append("\nHistory by type:")
        for t, count in history["by_type"].items():
            parts.append(f"  {t}: {count}")

    if history.get("by_operation"):
        parts.append("\nOperations:")
        for op, count in history["by_operation"].items():
            parts.append(f"  {op}: {count}")

    config = stats.get("config", {})
    graph = stats.get("graph", {})
    parts.append(
        f"\nBackend: {config.get('backend', 'sqlite (local)')}"
    )
    parts.append(f"Search: {config.get('search', 'bm25')}")
    parts.append(
        f"Graph: {config.get('graph', 'sqlite')} "
        f"({graph.get('entities', 0)} entities, {graph.get('relations', 0)} relations)"
    )

    return "\n".join(parts)


# ─── Entry point ─────────────────────────────────────────────────

if __name__ == "__main__":
    mcp.run()
