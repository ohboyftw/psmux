"""
Engram: Dynamic memory layer for Claude Code agents.

Companion to Beacon knowledge system. Beacon manages static docs;
Engram manages learned patterns, entity relationships, and reasoning traces.

Backend: SQLite + BM25 + optional ONNX semantic search.
Zero API calls. All local. <200ms per operation.

Modules:
    memory      - EngramMemory main class
    store       - SQLite memory storage with dedup
    search      - BM25 + semantic hybrid search
    entities    - Entity extraction and graph
    embeddings  - ONNX embedding model loader
    config      - Configuration loading and validation
    export      - Import/export portable snapshots
"""

__version__ = "0.2.0"
