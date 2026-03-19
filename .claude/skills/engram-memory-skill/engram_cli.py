#!/usr/bin/env python3
"""
engram — CLI for the Engram Memory Skill.

Commands:
    init      Initialize memory for a project
    remember  Store a memory
    recall    Search memory
    relate    Explore entity graph
    trace     Store a reasoning trace
    reflect   Run memory maintenance
    stats     Show memory statistics
    export    Export portable snapshot
    import    Import snapshot

Usage:
    python engram_cli.py init /path/to/project
    python engram_cli.py remember /path/to/project "We chose JWT for auth"
    python engram_cli.py recall /path/to/project "authentication approach"
    python engram_cli.py recall /path/to/project "auth" --bridge
    python engram_cli.py relate /path/to/project "Auth Service"
    python engram_cli.py stats /path/to/project
"""

import argparse
import json
import logging
import sys
from pathlib import Path

# Add parent to path so engram_index is importable
sys.path.insert(0, str(Path(__file__).parent))

from engram_index.config import load_config, detect_project_root, compute_project_hash
from engram_index.config import detect_project_name


def cmd_init(args):
    """Initialize memory for a project."""
    root = detect_project_root(Path(args.project_root))
    engram_dir = root / ".engram"

    if engram_dir.exists() and not args.force:
        print(f"Engram already initialized at {engram_dir}")
        print("Use --force to reinitialize.")
        return

    engram_dir.mkdir(parents=True, exist_ok=True)
    (engram_dir / "export").mkdir(exist_ok=True)

    # Generate config
    project_hash = compute_project_hash(root)
    project_name = detect_project_name(root)

    config_content = f"""# Engram Memory Configuration
# Project: {project_name}
# Hash: {project_hash}
version: "2.0"
project_hash: "{project_hash}"
project_name: "{project_name}"

# Search configuration
search:
  use_semantic: true   # Set false for BM25-only (no ONNX deps needed)

# Entity graph backend
graph:
  provider: sqlite     # Default: zero deps
  # provider: redisgraph  # Optional: needs Redis Stack
  # config:
  #   host: localhost
  #   port: 6379
  #   graph_name: engram

max_context_memories: 10
user_id: "${{ENGRAM_USER_ID:-default}}"

serena:
  enabled: auto
  search_on_recall: true
"""

    config_path = engram_dir / "config.yaml"
    config_path.write_text(config_content)

    # Check for Serena
    serena_detected = (root / ".serena").exists()

    print(f"Engram initialized at {engram_dir}")
    print(f"  Project: {project_name}")
    print(f"  Hash: {project_hash}")
    print(f"  Config: {config_path}")
    print(f"  Serena: {'detected' if serena_detected else 'not found'}")
    print()
    print("Add .engram/ to your .gitignore")
    print()

    # Check optional dependencies
    try:
        import numpy  # noqa: F401
        print("  numpy: installed")
    except ImportError:
        print("  numpy: not installed (optional, for semantic search)")

    try:
        import onnxruntime  # noqa: F401
        print("  onnxruntime: installed")
    except ImportError:
        print("  onnxruntime: not installed (optional, for semantic search)")

    try:
        import tokenizers  # noqa: F401
        print("  tokenizers: installed")
    except ImportError:
        print("  tokenizers: not installed (optional, for semantic search)")

    # Check ONNX model
    from engram_index.embeddings import MODEL_ONNX_PATH
    if MODEL_ONNX_PATH.exists():
        print(f"  ONNX model: found at {MODEL_ONNX_PATH.parent}")
    else:
        print(f"  ONNX model: not found (BM25-only search)")
        print(f"    Download: py beacon/scripts/download_onnx_model.py")


def cmd_remember(args):
    """Store a memory."""
    from engram_index.memory import EngramMemory

    engram = EngramMemory(args.project_root)

    content = " ".join(args.content)
    result = engram.remember(
        content,
        memory_type=args.type,
        scope=args.scope,
    )

    print(f"Remembered ({args.type}/{args.scope}):")
    print(f"  {content[:120]}{'...' if len(content) > 120 else ''}")
    if isinstance(result, dict) and "results" in result:
        print(f"  Stored {len(result['results'])} memory entries")


def cmd_recall(args):
    """Search memory."""
    from engram_index.memory import EngramMemory

    engram = EngramMemory(args.project_root)
    query = " ".join(args.query)

    output = engram.recall_formatted(
        query,
        top_k=args.top_k,
        bridge=args.bridge,
    )
    print(output)


def cmd_relate(args):
    """Explore entity relationships."""
    from engram_index.memory import EngramMemory

    engram = EngramMemory(args.project_root)
    entity = " ".join(args.entity)

    result = engram.relate(entity, max_depth=args.depth)

    print(f"## Entity: {entity}\n")

    if result.get("memories"):
        print("### Related Memories\n")
        for m in result["memories"]:
            print(f"  [{m.get('score', 0):.3f}] {m.get('memory', '')[:100]}")

    if result.get("relations"):
        print("\n### Relationships\n")
        for r in result["relations"]:
            if isinstance(r, dict):
                print(f"  {r.get('source', '?')} -> {r.get('relation_type', '?')} -> {r.get('target', '?')}")
            else:
                print(f"  {r}")

    if result.get("entities"):
        print("\n### Connected Entities\n")
        for e in result["entities"]:
            print(f"  {e['name']} (depth: {e.get('depth', 0)}, memories: {e.get('memory_count', 0)})")


def cmd_trace(args):
    """Store a reasoning trace."""
    from engram_index.memory import EngramMemory

    engram = EngramMemory(args.project_root)

    if args.file:
        with open(args.file) as f:
            data = json.load(f)
        engram.trace(
            task=data.get("task", "imported trace"),
            reasoning=data.get("reasoning", []),
            outcome=data.get("outcome", "unknown"),
            tools_used=data.get("tools_used", []),
            duration_minutes=data.get("duration_minutes"),
            related_entities=data.get("related_entities", []),
        )
    else:
        task = " ".join(args.task)
        engram.trace(
            task=task,
            reasoning=args.steps or ["(no steps provided)"],
            outcome=args.outcome,
            tools_used=args.tools.split(",") if args.tools else [],
        )

    print("Trace stored.")


def cmd_reflect(args):
    """Run memory maintenance / reflection."""
    from engram_index.memory import EngramMemory

    engram = EngramMemory(args.project_root)

    # Get stats as a basic reflection
    stats = engram.stats()

    print("# Engram Reflection Report\n")
    print(f"Project: {stats['project']}")
    print(f"Memories stored: {stats['memories_stored']}")
    print(f"Serena integration: {'yes' if stats['serena_integration'] else 'no'}")
    print()

    if stats.get("memories_by_type"):
        print("Memories by type:")
        for t, count in stats["memories_by_type"].items():
            print(f"  {t}: {count}")
        print()

    history = stats.get("history", {})
    print(f"Total operations: {history.get('total_operations', 0)}")
    print(f"Total traces: {history.get('total_traces', 0)}")

    config = stats.get("config", {})
    graph = stats.get("graph", {})
    print(f"\nBackend: {config.get('backend', 'sqlite (local)')}")
    print(f"Search: {config.get('search', 'bm25')}")
    print(
        f"Graph: {config.get('graph', 'sqlite')} "
        f"({graph.get('entities', 0)} entities, {graph.get('relations', 0)} relations)"
    )


def cmd_stats(args):
    """Show memory statistics."""
    cmd_reflect(args)  # Same output for now


def cmd_export(args):
    """Export portable snapshot."""
    from engram_index.memory import EngramMemory
    from engram_index.export import export_snapshot

    engram = EngramMemory(args.project_root)
    path = export_snapshot(engram, output_path=args.output)
    print(f"Snapshot exported to: {path}")


def cmd_import(args):
    """Import snapshot."""
    from engram_index.memory import EngramMemory
    from engram_index.export import import_snapshot

    engram = EngramMemory(args.project_root)
    result = import_snapshot(engram, args.input)
    print(f"Imported: {result['memories']} memories, {result['traces']} traces")
    if result['errors']:
        print(f"Errors: {result['errors']}")


def main():
    parser = argparse.ArgumentParser(
        description="Engram Memory Skill CLI",
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument("-v", "--verbose", action="store_true")
    subparsers = parser.add_subparsers(dest="command", required=True)

    # init
    p_init = subparsers.add_parser("init", help="Initialize memory for a project")
    p_init.add_argument("project_root")
    p_init.add_argument("--force", action="store_true", help="Reinitialize even if exists")

    # remember
    p_rem = subparsers.add_parser("remember", help="Store a memory")
    p_rem.add_argument("project_root")
    p_rem.add_argument("content", nargs="+")
    p_rem.add_argument("--type", default="fact",
                       choices=["fact", "decision", "pattern", "preference", "context", "trace"])
    p_rem.add_argument("--scope", default="project", choices=["project", "session", "user"])

    # recall
    p_rec = subparsers.add_parser("recall", help="Search memory")
    p_rec.add_argument("project_root")
    p_rec.add_argument("query", nargs="+")
    p_rec.add_argument("-k", "--top-k", type=int, default=5)
    p_rec.add_argument("--type", dest="memory_type", help="Filter by memory type")
    p_rec.add_argument("--scope", help="Filter by scope")
    p_rec.add_argument("--bridge", action="store_true", help="Also search Serena docs")

    # relate
    p_rel = subparsers.add_parser("relate", help="Explore entity graph")
    p_rel.add_argument("project_root")
    p_rel.add_argument("entity", nargs="+")
    p_rel.add_argument("--depth", type=int, default=2)

    # trace
    p_trace = subparsers.add_parser("trace", help="Store a reasoning trace")
    p_trace.add_argument("project_root")
    p_trace.add_argument("task", nargs="*")
    p_trace.add_argument("--file", help="Import trace from JSON file")
    p_trace.add_argument("--steps", nargs="+", help="Reasoning steps")
    p_trace.add_argument("--outcome", default="success")
    p_trace.add_argument("--tools", help="Comma-separated tool names")

    # reflect
    p_reflect = subparsers.add_parser("reflect", help="Run memory maintenance")
    p_reflect.add_argument("project_root")
    p_reflect.add_argument("--consolidate", action="store_true")
    p_reflect.add_argument("--detect-patterns", action="store_true")

    # stats
    p_stats = subparsers.add_parser("stats", help="Show memory statistics")
    p_stats.add_argument("project_root")

    # export
    p_export = subparsers.add_parser("export", help="Export portable snapshot")
    p_export.add_argument("project_root")
    p_export.add_argument("-o", "--output", help="Output file path")

    # import
    p_import = subparsers.add_parser("import", help="Import snapshot")
    p_import.add_argument("project_root")
    p_import.add_argument("-i", "--input", required=True, help="Snapshot file path")

    args = parser.parse_args()

    if args.verbose:
        logging.basicConfig(level=logging.DEBUG)
    else:
        logging.basicConfig(level=logging.INFO, format="%(message)s")

    commands = {
        "init": cmd_init,
        "remember": cmd_remember,
        "recall": cmd_recall,
        "relate": cmd_relate,
        "trace": cmd_trace,
        "reflect": cmd_reflect,
        "stats": cmd_stats,
        "export": cmd_export,
        "import": cmd_import,
    }
    commands[args.command](args)


if __name__ == "__main__":
    main()
