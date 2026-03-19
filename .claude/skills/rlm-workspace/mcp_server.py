"""
RLM Workspace MCP Server.

Two-mode architecture:
  - Agent-driven: Claude Code loads content and writes Python to explore it via REPL
  - Autonomous: Full RLM library runs its own iterative analysis loop

Tools:
  rlm_load     - Load file(s) into workspace as Python variable
  rlm_exec     - Execute Python code in sandboxed REPL
  rlm_vars     - List loaded variables with metadata
  rlm_analyze  - Run autonomous RLM analysis (requires rlms package)
  rlm_status   - Show workspace state and config
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

# Add skill directory to path so rlm_workspace package is importable
sys.path.insert(0, str(Path(__file__).parent))

from mcp.server.fastmcp import FastMCP

from rlm_workspace.workspace import Workspace

mcp = FastMCP("RLM Workspace")

_workspace: Workspace | None = None


def _get_workspace() -> Workspace:
    """Lazy-initialize the workspace singleton."""
    global _workspace
    if _workspace is None:
        _workspace = Workspace()
    return _workspace


@mcp.tool()
def rlm_load(
    path: str,
    var_name: str = "context",
    recursive: bool = False,
    glob_pattern: str = "",
) -> str:
    """Load file(s) into the RLM workspace as a Python variable.

    The loaded content becomes available in rlm_exec as a variable.

    Args:
        path: Absolute or relative path to a file or directory.
        var_name: Variable name in the REPL namespace (default: "context").
        recursive: If path is a directory, scan subdirectories recursively.
        glob_pattern: Filter files by glob pattern (e.g. "*.py", "*.rs").

    Returns:
        Summary of what was loaded (file count, total size, variable name).
    """
    ws = _get_workspace()
    try:
        meta = ws.load(path, var_name=var_name, recursive=recursive, glob_pattern=glob_pattern)
        if meta.get("error"):
            return meta["error"]
        return (
            f"Loaded {meta['files']} file(s) ({meta['total_chars']:,} chars, "
            f"{meta['total_lines']:,} lines) as variable '{meta['var_name']}'"
        )
    except FileNotFoundError as e:
        return f"[ERROR] {e}"
    except Exception as e:
        return f"[ERROR] {type(e).__name__}: {e}"


@mcp.tool()
def rlm_exec(code: str) -> str:
    """Execute Python code in the sandboxed workspace REPL.

    The REPL has access to all variables loaded via rlm_load. Variables
    persist across calls. Use print() to see output.

    Allowed imports: re, json, math, statistics, collections, itertools,
    functools, textwrap, difflib, hashlib, datetime, pathlib, string,
    ast, csv, dataclasses, typing, enum, copy, io, and more stdlib modules.

    Blocked: os.system, subprocess, eval/exec, file writing.

    Args:
        code: Python code to execute.

    Returns:
        Captured stdout output and/or error messages.
    """
    ws = _get_workspace()
    return ws.execute(code)


@mcp.tool()
def rlm_vars() -> str:
    """List all variables in the workspace with types and sizes.

    Returns:
        Formatted list of variables with metadata.
    """
    ws = _get_workspace()
    return ws.get_vars()


@mcp.tool()
def rlm_analyze(path: str, question: str, max_iterations: int = 30) -> str:
    """Run full autonomous RLM analysis on a file or directory.

    Uses the rlm library's iterative REPL loop with a configured LLM backend
    (default: Ollama/qwen3:8b). The LLM writes and executes exploration code
    autonomously until it can answer the question.

    Requires the 'rlms' package. If not installed, returns instructions.

    For more control, use rlm_load + rlm_exec instead (agent-driven mode).

    Args:
        path: Path to file or directory to analyze.
        question: Question to answer about the content.
        max_iterations: Maximum REPL iterations (default: 30).

    Returns:
        Complete analysis result or error message.
    """
    ws = _get_workspace()
    return ws.analyze(path, question, max_iterations=max_iterations)


@mcp.tool()
def rlm_status() -> str:
    """Show workspace state and backend configuration.

    Returns:
        Formatted status including backend config, loaded variables,
        and feature availability.
    """
    ws = _get_workspace()
    status = ws.status()
    return json.dumps(status, indent=2)


if __name__ == "__main__":
    mcp.run(transport="stdio")
