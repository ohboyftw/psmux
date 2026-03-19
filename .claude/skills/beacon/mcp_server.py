"""
Beacon Knowledge System MCP Server.

Exposes the Beacon knowledge base as MCP tools for Claude Code:
  beacon_search   - Hybrid BM25+semantic search across project docs
  beacon_index    - Build or rebuild the search index
  beacon_garden   - Run doc health checks and optional auto-fix
  beacon_stats    - Show knowledge base statistics
  beacon_reconcile - Compare Beacon docs against engram memories

Tools:
  beacon_search    - Search the knowledge base with hybrid retrieval
  beacon_index     - Build/rebuild the search index for a project
  beacon_garden    - Run doc health checks (structure, freshness, links)
  beacon_stats     - Show KB statistics (doc count, chunks, token usage)
  beacon_reconcile - Cross-reference Beacon docs with engram memory output
"""

from __future__ import annotations

import sys
from pathlib import Path

# Windows UTF-8 fix — only reconfigure stderr.
# stdout is used by FastMCP's stdio transport in binary mode;
# reconfiguring it to text/UTF-8 breaks JSON-RPC responses.
if sys.stderr and hasattr(sys.stderr, "reconfigure"):
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")

# Add scripts/ to path so beacon_index is importable
sys.path.insert(0, str(Path(__file__).parent / "scripts"))

from mcp.server.fastmcp import FastMCP

from beacon_index.chunker import index_knowledge_base
from beacon_index.retriever import HybridRetriever
from beacon_index.gardener import DocGardener
from beacon_index import semantic as sem

mcp = FastMCP("Beacon Knowledge System")

# Lazy-initialized retrievers keyed by project root
_retrievers: dict[str, HybridRetriever] = {}


def _get_retriever(project_root: str) -> HybridRetriever:
    """Get or create a HybridRetriever for the given project root, loading any saved index."""
    root = str(Path(project_root).resolve())
    if root in _retrievers:
        return _retrievers[root]

    retriever = HybridRetriever(root)
    index_dir = Path(root) / ".beacon"
    legacy_dir = Path(root) / ".serena"

    if index_dir.exists() and (index_dir / "bm25_index.json").exists():
        retriever.load_index(index_dir)
    elif legacy_dir.exists() and (legacy_dir / "bm25_index.json").exists():
        retriever.load_index(legacy_dir)

    _retrievers[root] = retriever
    return retriever


@mcp.tool()
def beacon_search(
    project_root: str,
    query: str,
    top_k: int = 5,
    filter_doc: str = "",
    filter_type: str = "",
) -> str:
    """Search the Beacon knowledge base with hybrid BM25 + semantic retrieval.

    Returns ranked document chunks matching the query. Uses Reciprocal Rank
    Fusion when semantic search is available, falls back to BM25-only otherwise.

    Args:
        project_root: Absolute path to the project root directory.
        query: Natural language search query.
        top_k: Number of results to return (default: 5).
        filter_doc: Restrict results to a specific document path (optional).
        filter_type: Restrict results to a chunk type: prose, code, table, list, frontmatter (optional).

    Returns:
        Formatted search results with scores, sources, and content snippets.
    """
    try:
        retriever = _get_retriever(project_root)

        if not retriever.chunks:
            retriever.build_index()
            index_dir = Path(project_root) / ".beacon"
            retriever.save_index(index_dir)

        kwargs = {}
        if filter_doc:
            kwargs["filter_doc"] = filter_doc
        if filter_type:
            kwargs["filter_type"] = filter_type

        return retriever.search_formatted(query, top_k=top_k, **kwargs)
    except Exception as e:
        return f"[ERROR] beacon_search failed: {type(e).__name__}: {e}"


@mcp.tool()
def beacon_index(project_root: str) -> str:
    """Build or rebuild the Beacon search index for a project.

    Scans all markdown files (AGENTS.md, ARCHITECTURE.md, docs/**/*.md),
    chunks them by semantic boundaries, and builds BM25 + optional semantic indexes.
    Saves the index to .beacon/ in the project root.

    Args:
        project_root: Absolute path to the project root directory.

    Returns:
        Summary of indexed documents, chunks, and mode.
    """
    try:
        # Force fresh build by removing cached retriever
        root = str(Path(project_root).resolve())
        _retrievers.pop(root, None)

        retriever = HybridRetriever(root)
        stats = retriever.build_index()

        if stats["chunks"] == 0:
            return "No markdown documents found. Ensure the project has AGENTS.md, ARCHITECTURE.md, or a docs/ directory."

        index_dir = Path(root) / ".beacon"
        retriever.save_index(index_dir)
        _retrievers[root] = retriever

        lines = [
            f"Indexed {stats['chunks']} chunks from {stats['docs']} documents",
            f"Mode: {stats['mode']}",
            f"Index saved to: {index_dir}",
            "",
            "Chunk types:",
        ]
        for ctype, count in stats.get("chunk_types", {}).items():
            lines.append(f"  {ctype}: {count}")

        lines.append("")
        lines.append("Documents:")
        for doc in stats.get("doc_paths", []):
            lines.append(f"  {doc}")

        return "\n".join(lines)
    except Exception as e:
        return f"[ERROR] beacon_index failed: {type(e).__name__}: {e}"


@mcp.tool()
def beacon_garden(
    project_root: str,
    auto_fix: bool = False,
    deep: bool = False,
    output_format: str = "markdown",
) -> str:
    """Run doc health checks on the knowledge base.

    Checks structure (required files/dirs), frontmatter validity, staleness,
    cross-link integrity, orphaned docs, and missing indexes. With --deep,
    also detects near-duplicate content and suggests cross-links.

    Args:
        project_root: Absolute path to the project root directory.
        auto_fix: Automatically fix safe issues like stale status and missing index files (default: False).
        deep: Run expensive checks including duplicate detection and cross-link suggestions (default: False).
        output_format: Output format, either "markdown" or "json" (default: "markdown").

    Returns:
        Health report with errors, warnings, and optional auto-fix summary.
    """
    try:
        gardener = DocGardener(project_root)
        report = gardener.run(auto_fix=auto_fix, deep=deep)

        if output_format == "json":
            return report.to_json()
        return report.to_markdown()
    except Exception as e:
        return f"[ERROR] beacon_garden failed: {type(e).__name__}: {e}"


@mcp.tool()
def beacon_stats(project_root: str) -> str:
    """Show knowledge base statistics for a project.

    Reports document count, chunk count, token usage, chunk type distribution,
    per-document sizes, and semantic search availability.

    Args:
        project_root: Absolute path to the project root directory.

    Returns:
        Formatted statistics overview.
    """
    try:
        root = Path(project_root)
        chunks = index_knowledge_base(root)

        if not chunks:
            return "No knowledge base found. Ensure the project has AGENTS.md, ARCHITECTURE.md, or a docs/ directory."

        docs = set(c.doc_path for c in chunks)
        types: dict[str, int] = {}
        for c in chunks:
            types[c.chunk_type] = types.get(c.chunk_type, 0) + 1

        total_tokens = sum(c.tokens_approx for c in chunks)

        doc_sizes: dict[str, int] = {}
        for c in chunks:
            doc_sizes[c.doc_path] = doc_sizes.get(c.doc_path, 0) + c.tokens_approx

        lines = [
            "Knowledge Base Statistics",
            "========================",
            f"Documents:    {len(docs)}",
            f"Chunks:       {len(chunks)}",
            f"Total tokens: ~{total_tokens:,}",
            f"Avg tokens/chunk: ~{total_tokens // len(chunks):,}",
            "",
            "Chunk Types:",
        ]
        for ctype, count in sorted(types.items(), key=lambda x: -x[1]):
            pct = count / len(chunks) * 100
            lines.append(f"  {ctype:15s} {count:4d}  ({pct:.0f}%)")

        lines.append("")
        lines.append("Documents by Size (tokens):")
        for doc, size in sorted(doc_sizes.items(), key=lambda x: -x[1]):
            lines.append(f"  {doc:40s} ~{size:5,}")

        lines.append("")
        lines.append(
            f"Semantic search: {'available' if sem.is_available() else 'not available'}"
        )
        if not sem.is_available():
            lines.append(f"  Install with: {sem.install_hint()}")

        return "\n".join(lines)
    except Exception as e:
        return f"[ERROR] beacon_stats failed: {type(e).__name__}: {e}"


@mcp.tool()
def beacon_reconcile(
    project_root: str,
    topic: str,
    engram_memories: str = "",
    top_k: int = 5,
) -> str:
    """Cross-reference Beacon docs with engram memory output for a topic.

    Searches Beacon for the topic, then compares results against engram memories
    (if provided) to identify gaps, contradictions, and stale information.

    Use this to check whether what's documented matches what's been decided/learned.
    Call engram recall separately and pass its output as engram_memories.

    Args:
        project_root: Absolute path to the project root directory.
        topic: The topic or question to reconcile across both systems.
        engram_memories: Output from an engram recall command (optional). If empty, only Beacon results are returned.
        top_k: Number of Beacon results to compare (default: 5).

    Returns:
        Reconciliation report showing Beacon docs, engram memories, and analysis.
    """
    try:
        retriever = _get_retriever(project_root)

        if not retriever.chunks:
            retriever.build_index()
            index_dir = Path(project_root) / ".beacon"
            retriever.save_index(index_dir)

        results = retriever.search(topic, top_k=top_k)

        lines = [
            f"# Reconciliation Report: {topic}",
            "",
            f"## Beacon Documents ({len(results)} results)",
            "",
        ]

        if not results:
            lines.append("No Beacon results found for this topic.")
        else:
            for i, r in enumerate(results, 1):
                lines.append(f"### [{i}] {r.chunk.doc_path}")
                if r.chunk.heading_path:
                    lines.append(f"Section: {' > '.join(r.chunk.heading_path)}")
                lines.append(f"Score: {r.score:.3f} | Source: {r.source}")
                status = r.chunk.frontmatter.get("status", "unknown")
                last_verified = r.chunk.frontmatter.get("last_verified", "unknown")
                lines.append(f"Status: {status} | Last verified: {last_verified}")
                content = r.chunk.content
                if len(content) > 800:
                    content = content[:800] + "\n... [truncated]"
                lines.append(f"```\n{content}\n```")
                lines.append("")

        lines.append("## Engram Memories")
        lines.append("")

        if not engram_memories or not engram_memories.strip():
            lines.append("No engram memories provided. Call `engram recall` separately and pass the output here.")
        else:
            lines.append(engram_memories.strip())

        lines.append("")
        lines.append("## Reconciliation Guidance")
        lines.append("")

        if results and engram_memories and engram_memories.strip():
            doc_paths = [r.chunk.doc_path for r in results]
            lines.append(
                "Both Beacon docs and engram memories are available. Review for:"
            )
            lines.append(
                "- **Contradictions**: Do engram decisions conflict with documented architecture?"
            )
            lines.append(
                "- **Gaps**: Are there engram decisions not yet captured in docs?"
            )
            lines.append(
                "- **Staleness**: Are Beacon docs outdated compared to recent engram traces?"
            )
            lines.append("")
            lines.append(f"Beacon docs found: {', '.join(doc_paths)}")
        elif results and not engram_memories:
            lines.append(
                "Only Beacon docs available. To get a full reconciliation, "
                "run `engram recall` for this topic and pass the output as engram_memories."
            )
        elif engram_memories and not results:
            lines.append(
                "Only engram memories available. Beacon has no docs on this topic. "
                "Consider creating documentation to capture these decisions."
            )
        else:
            lines.append(
                "Neither system has information on this topic. "
                "This may be a new area that needs both documentation and decision tracking."
            )

        return "\n".join(lines)
    except Exception as e:
        return f"[ERROR] beacon_reconcile failed: {type(e).__name__}: {e}"


if __name__ == "__main__":
    mcp.run(transport="stdio")
