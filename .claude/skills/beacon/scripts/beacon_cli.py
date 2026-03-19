#!/usr/bin/env python3
"""
beacon — CLI for the Beacon Knowledge System.

Commands:
    index     Build/rebuild the search index
    search    Search the knowledge base
    garden    Run doc health checks
    stats     Show knowledge base statistics

Usage:
    python beacon_cli.py index /path/to/project
    python beacon_cli.py search /path/to/project "how does auth work"
    python beacon_cli.py garden /path/to/project --deep --fix
    python beacon_cli.py stats /path/to/project
"""

import argparse
import json
import logging
import sys
from pathlib import Path

# Fix Windows cp1252 encoding crash when outputting unicode
if sys.stdout and hasattr(sys.stdout, 'reconfigure'):
    sys.stdout.reconfigure(encoding='utf-8', errors='replace')

# Add parent to path so beacon_index is importable
sys.path.insert(0, str(Path(__file__).parent))

from beacon_index.chunker import index_knowledge_base
from beacon_index.bm25 import BM25Index
from beacon_index.retriever import HybridRetriever
from beacon_index.gardener import DocGardener
from beacon_index import semantic as sem


def cmd_index(args):
    """Build or rebuild the search index."""
    retriever = HybridRetriever(args.project_root, use_semantic=not args.no_semantic)
    stats = retriever.build_index()

    print(f"Indexed {stats['chunks']} chunks from {stats['docs']} documents")
    print(f"Mode: {stats['mode']}")
    print(f"\nChunk types:")
    for ctype, count in stats.get("chunk_types", {}).items():
        print(f"  {ctype}: {count}")
    print(f"\nDocuments:")
    for doc in stats.get("doc_paths", []):
        print(f"  {doc}")

    # Save index
    index_dir = Path(args.project_root) / ".beacon"
    retriever.save_index(index_dir)
    print(f"\nIndex saved to {index_dir}")


def cmd_search(args):
    """Search the knowledge base."""
    index_dir = Path(args.project_root) / ".beacon"

    retriever = HybridRetriever(args.project_root, use_semantic=not args.no_semantic)

    # Try loading existing index — check both .beacon and legacy .serena
    legacy_dir = Path(args.project_root) / ".serena"
    if index_dir.exists() and (index_dir / "bm25_index.json").exists():
        retriever.load_index(index_dir)
        print(f"Loaded index ({len(retriever.chunks)} chunks, mode: {retriever.mode})\n")
    elif legacy_dir.exists() and (legacy_dir / "bm25_index.json").exists():
        retriever.load_index(legacy_dir)
        print(f"Loaded legacy .serena index ({len(retriever.chunks)} chunks, mode: {retriever.mode})")
        print(f"Hint: run 'beacon index' to migrate to .beacon/\n")
    else:
        print("No saved index found. Building fresh index...\n")
        retriever.build_index()
        retriever.save_index(index_dir)

    # Search
    query = " ".join(args.query)
    output = retriever.search_formatted(
        query,
        top_k=args.top_k,
        filter_doc=args.doc,
        filter_type=args.type,
    )
    print(output)


def cmd_garden(args):
    """Run doc health checks."""
    gardener = DocGardener(args.project_root)
    report = gardener.run(auto_fix=args.fix, deep=args.deep)

    if args.json:
        print(report.to_json())
    else:
        print(report.to_markdown())

    if args.output:
        Path(args.output).write_text(
            report.to_json() if args.json else report.to_markdown()
        )
        print(f"\nReport saved to {args.output}")

    sys.exit(2 if report.errors else 1 if report.warnings else 0)


def cmd_stats(args):
    """Show knowledge base statistics."""
    root = Path(args.project_root)

    chunks = index_knowledge_base(root)

    if not chunks:
        print("No knowledge base found. Run 'init docs' first.")
        return

    # Aggregate stats
    docs = set(c.doc_path for c in chunks)
    types = {}
    for c in chunks:
        types[c.chunk_type] = types.get(c.chunk_type, 0) + 1

    total_tokens = sum(c.tokens_approx for c in chunks)

    doc_sizes = {}
    for c in chunks:
        doc_sizes[c.doc_path] = doc_sizes.get(c.doc_path, 0) + c.tokens_approx

    print(f"Knowledge Base Statistics")
    print(f"========================")
    print(f"Documents:    {len(docs)}")
    print(f"Chunks:       {len(chunks)}")
    print(f"Total tokens: ~{total_tokens:,}")
    print(f"Avg tokens/chunk: ~{total_tokens // len(chunks):,}")
    print()

    print(f"Chunk Types:")
    for ctype, count in sorted(types.items(), key=lambda x: -x[1]):
        print(f"  {ctype:15s} {count:4d}  ({count/len(chunks)*100:.0f}%)")
    print()

    print(f"Documents by Size (tokens):")
    for doc, size in sorted(doc_sizes.items(), key=lambda x: -x[1]):
        bar = "\u2588" * min(40, size // 50)
        print(f"  {doc:40s} ~{size:5,} {bar}")
    print()

    # Semantic availability
    print(f"Semantic search: {'available' if sem.is_available() else 'not available'}")
    if not sem.is_available():
        print(f"  Install with: {sem.install_hint()}")


def main():
    parser = argparse.ArgumentParser(
        description="Beacon Knowledge System CLI",
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument("-v", "--verbose", action="store_true")
    subparsers = parser.add_subparsers(dest="command", required=True)

    # index
    p_index = subparsers.add_parser("index", help="Build search index")
    p_index.add_argument("project_root")
    p_index.add_argument("--no-semantic", action="store_true", help="Skip semantic index")

    # search
    p_search = subparsers.add_parser("search", help="Search the knowledge base")
    p_search.add_argument("project_root")
    p_search.add_argument("query", nargs="+")
    p_search.add_argument("-k", "--top-k", type=int, default=5)
    p_search.add_argument("--doc", help="Filter to specific document")
    p_search.add_argument("--type", help="Filter to chunk type (prose, code, table, etc.)")
    p_search.add_argument("--no-semantic", action="store_true")

    # garden
    p_garden = subparsers.add_parser("garden", help="Run doc health checks")
    p_garden.add_argument("project_root")
    p_garden.add_argument("--fix", action="store_true", help="Auto-fix safe issues")
    p_garden.add_argument("--deep", action="store_true", help="Run deep checks")
    p_garden.add_argument("--json", action="store_true")
    p_garden.add_argument("-o", "--output", help="Save report to file")

    # stats
    p_stats = subparsers.add_parser("stats", help="Show KB statistics")
    p_stats.add_argument("project_root")

    args = parser.parse_args()

    if args.verbose:
        logging.basicConfig(level=logging.DEBUG)
    else:
        logging.basicConfig(level=logging.INFO, format="%(message)s")

    commands = {
        "index": cmd_index,
        "search": cmd_search,
        "garden": cmd_garden,
        "stats": cmd_stats,
    }
    commands[args.command](args)


if __name__ == "__main__":
    main()
