"""
RLM Workspace: MCP server providing a sandboxed Python REPL for
large-context code exploration and analysis.

Two modes:
  - Agent-driven: Claude Code writes Python to explore loaded content
  - Autonomous: rlm library runs its own iterative analysis loop

Modules:
    config     - Configuration loading and validation
    repl       - Sandboxed Python execution environment
    workspace  - Stateful workspace manager
"""

__version__ = "2.0.0"
