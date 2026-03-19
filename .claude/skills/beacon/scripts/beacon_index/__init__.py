"""
Beacon Index: Chunking, retrieval, and maintenance for the Beacon knowledge base.

Modules:
    chunker   - Markdown-aware document chunking
    bm25      - BM25 sparse retrieval (zero dependencies)
    semantic  - Dense vector retrieval (optional: requires sentence-transformers)
    retriever - Hybrid BM25 + Semantic with Reciprocal Rank Fusion
    gardener  - Knowledge base health checking and maintenance
"""

__version__ = "0.1.0"
