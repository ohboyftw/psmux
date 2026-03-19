"""
LLM Council MCP Server — Multi-LLM code review ensemble.

Fans out code reviews to 5 LLM providers (Claude, GPT-5.3, Gemini, MiniMax M1,
Kimi K2.5) in parallel, synthesizes consensus, then merges with CodeRabbit
for a final grand synthesis report. Includes benchmarking harness.

Tools:
  council_review    - Run multi-LLM code review on a diff
  council_benchmark - Run evaluation harness against ground truth
  council_status    - Show which API keys are configured

Configure in ~/.claude/settings.json:
    {
        "mcpServers": {
            "llm-council": {
                "command": "py",
                "args": ["C:\\Users\\aravi\\.claude\\skills\\llm-council\\mcp_server.py"]
            }
        }
    }
"""

from __future__ import annotations

import json
import os
import sys
from pathlib import Path

# Windows UTF-8 fix — only reconfigure stderr.
# stdout is used by FastMCP's stdio transport in binary mode;
# reconfiguring it to text/UTF-8 breaks JSON-RPC responses.
if sys.stderr and hasattr(sys.stderr, "reconfigure"):
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")

# Add skill directory to path so council/evaluate are importable
sys.path.insert(0, str(Path(__file__).parent))

from mcp.server.fastmcp import FastMCP

mcp = FastMCP("LLM Council")


@mcp.tool()
async def council_review(
    diff: str,
    context: str = "",
    include_coderabbit: bool = True,
    output_format: str = "markdown",
) -> str:
    """Run a multi-LLM code review ensemble on a diff.

    Fans out to all configured LLM providers (Claude, GPT-5.3, Gemini,
    MiniMax M1, Kimi K2.5) in parallel, synthesizes their consensus,
    then optionally merges with CodeRabbit for a grand synthesis.

    Only models with API keys set will be activated.

    Args:
        diff: The code diff to review (unified diff format).
        context: Optional project context to help reviewers.
        include_coderabbit: Whether to include CodeRabbit review (default: True).
        output_format: "markdown" for human-readable report, "json" for structured data.

    Returns:
        Review report in the requested format.
    """
    from council import run_council, format_report, extract_findings_by_source

    result = await run_council(
        diff, context=context, include_coderabbit=include_coderabbit
    )

    if output_format == "json":
        sources = extract_findings_by_source(result)
        synth = result.grand_synthesis or result.council_synthesis
        return json.dumps(
            {
                "sources": sources,
                "recommendation": getattr(synth, "recommendation", "unknown"),
                "agreement_score": getattr(synth, "agreement_score", 0.0),
                "total_latency_ms": result.total_latency_ms,
            },
            indent=2,
        )
    return format_report(result)


@mcp.tool()
async def council_benchmark(
    cases: list[str] | None = None,
    include_coderabbit: bool = True,
    output_format: str = "markdown",
) -> str:
    """Run the evaluation harness scoring every review source against ground truth.

    Scores each source independently: individual LLMs, CodeRabbit,
    council synthesis, and grand synthesis. Uses built-in test cases
    covering SQL injection, race conditions, React memory leaks,
    auth bypass, and N+1 queries.

    Args:
        cases: Optional list of test case IDs to run (e.g. ["sql-injection", "auth-bypass"]).
               If None, runs all built-in cases.
        include_coderabbit: Whether to include CodeRabbit in benchmarks.
        output_format: "markdown" for formatted report, "json" for structured data.

    Returns:
        Benchmark report with leaderboard, heatmap, and insights.
    """
    from evaluate import (
        run_evaluation,
        format_eval_report,
        export_json,
        BUILTIN_CASES,
    )

    test_cases = BUILTIN_CASES
    if cases:
        test_cases = [tc for tc in test_cases if tc.id in cases]
        if not test_cases:
            return f"No matching test cases found. Available: {[tc.id for tc in BUILTIN_CASES]}"

    result = await run_evaluation(
        test_cases=test_cases, include_coderabbit=include_coderabbit
    )

    if output_format == "json":
        return export_json(result)
    return format_eval_report(result)


@mcp.tool()
def council_status() -> str:
    """Show which API keys are configured and which council members will be active.

    Returns:
        Status table showing each provider's key status and model.
    """
    keys = {
        "ANTHROPIC_API_KEY": ("Claude", "claude-sonnet-4-20250514", "Synthesizer + council member"),
        "OPENAI_API_KEY": ("GPT-5.2-Codex", "gpt-5.2-codex", "Council member"),
        "GOOGLE_API_KEY": ("Gemini", "gemini-2.0-flash", "Council member"),
        "MINIMAX_API_KEY": ("MiniMax M2.1", "MiniMax-M2.1", "Council member"),
        "MOONSHOT_API_KEY": ("Kimi K2.5", "kimi-k2.5", "Council member"),
        "CODERABBIT_API_KEY": ("CodeRabbit", "coderabbit-api", "Independent reviewer"),
        "OPENROUTER_API_KEY": ("OpenRouter", "any", "Universal fallback"),
    }

    lines = ["LLM Council — Provider Status\n"]
    lines.append("| Provider | Model | Role | Status |")
    lines.append("|----------|-------|------|--------|")

    active_count = 0
    for env_var, (name, model, role) in keys.items():
        has_key = bool(os.environ.get(env_var, ""))
        status = "Active" if has_key else "Not configured"
        if has_key:
            active_count += 1
        lines.append(f"| {name} | {model} | {role} | {status} |")

    lines.append(f"\n**Active providers:** {active_count}/{len(keys)}")
    if active_count < 2:
        lines.append("**Warning:** Need at least 2 LLM providers for council synthesis.")

    return "\n".join(lines)


if __name__ == "__main__":
    mcp.run(transport="stdio")
