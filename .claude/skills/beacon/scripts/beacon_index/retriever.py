"""
Hybrid retriever: fuses BM25 (sparse) + Semantic (dense) results using
Reciprocal Rank Fusion (RRF).

If semantic search is unavailable, falls back to BM25-only.
The fusion approach gives best-of-both-worlds:
  - BM25 excels at exact keyword matching (config names, function names, acronyms)
  - Semantic excels at conceptual matching ("how does auth work" → security docs)

RRF is simple and doesn't need tuning: score = Σ 1/(k + rank_i) across retrievers.
"""

from __future__ import annotations

import logging
from dataclasses import dataclass
from pathlib import Path
from typing import Optional

from .chunker import Chunk, index_knowledge_base
from .bm25 import BM25Index, ScoredChunk
from . import semantic as sem

logger = logging.getLogger("beacon.retriever")

# RRF constant (standard value from literature)
RRF_K = 60


@dataclass
class RetrievalResult:
    """A chunk with fused ranking information."""

    chunk: Chunk
    score: float  # RRF score (or BM25 score if no semantic)
    bm25_rank: Optional[int]  # Rank in BM25 results (None if not in top-k)
    semantic_rank: Optional[int]  # Rank in semantic results
    matched_terms: list[str]  # BM25 matched terms (empty if semantic-only)
    source: str  # "bm25", "semantic", or "hybrid"

    @property
    def context(self) -> str:
        """Full retrieval context for the agent."""
        return (
            f"{self.chunk.context_header}\n"
            f"Score: {self.score:.3f} | Source: {self.source} | "
            f"Matched: {', '.join(self.matched_terms) if self.matched_terms else 'semantic'}\n"
            f"---\n"
            f"{self.chunk.content}"
        )


class HybridRetriever:
    """
    Hybrid BM25 + Semantic retriever with Reciprocal Rank Fusion.

    Usage:
        retriever = HybridRetriever("/path/to/project")
        retriever.build_index()
        results = retriever.search("database migration strategy", top_k=5)

    Falls back to BM25-only if sentence-transformers is not installed.
    """

    def __init__(self, project_root: str | Path, use_semantic: bool = True):
        self.project_root = Path(project_root)
        self.bm25 = BM25Index()
        self.semantic: Optional[sem.SemanticIndex] = None
        self.chunks: list[Chunk] = []
        self._use_semantic = use_semantic and sem.is_available()

        if use_semantic and not sem.is_available():
            logger.info(
                f"Semantic search unavailable. Install with: {sem.install_hint()}\n"
                f"Falling back to BM25-only."
            )

    @property
    def mode(self) -> str:
        return "hybrid" if self._use_semantic and self.semantic else "bm25"

    def build_index(self) -> dict:
        """
        Index the knowledge base. Returns stats about what was indexed.
        """
        logger.info(f"Indexing knowledge base at: {self.project_root}")

        # Chunk all documents
        self.chunks = index_knowledge_base(self.project_root)
        logger.info(f"Chunked {len(self.chunks)} segments from knowledge base")

        if not self.chunks:
            return {"chunks": 0, "mode": "empty", "docs": 0}

        # Build BM25 index (always)
        self.bm25.add_chunks(self.chunks)
        logger.info("BM25 index built")

        # Build semantic index (if available)
        if self._use_semantic:
            try:
                self.semantic = sem.SemanticIndex()
                self.semantic.build(self.chunks)
                logger.info("Semantic index built")
            except Exception as e:
                logger.warning(f"Semantic index failed, using BM25 only: {e}")
                self.semantic = None

        # Stats
        doc_paths = set(c.doc_path for c in self.chunks)
        stats = {
            "chunks": len(self.chunks),
            "docs": len(doc_paths),
            "mode": self.mode,
            "doc_paths": sorted(doc_paths),
            "chunk_types": {
                t: sum(1 for c in self.chunks if c.chunk_type == t)
                for t in set(c.chunk_type for c in self.chunks)
            },
        }
        return stats

    def search(
        self,
        query: str,
        top_k: int = 5,
        filter_doc: Optional[str] = None,
        filter_type: Optional[str] = None,
        bm25_weight: float = 1.0,
        semantic_weight: float = 1.0,
    ) -> list[RetrievalResult]:
        """
        Search the knowledge base with hybrid retrieval.

        Args:
            query: Natural language query
            top_k: Number of results to return
            filter_doc: Restrict to a specific document
            filter_type: Restrict to a chunk type
            bm25_weight: Weight for BM25 in RRF fusion
            semantic_weight: Weight for semantic in RRF fusion

        Returns:
            List of RetrievalResult objects, ranked by fused score
        """
        # Fetch more candidates than needed for fusion
        fetch_k = top_k * 3

        # BM25 retrieval
        bm25_results = self.bm25.search(
            query, top_k=fetch_k, filter_doc=filter_doc, filter_type=filter_type
        )

        # If no semantic index, return BM25 directly
        if not self.semantic:
            return [
                RetrievalResult(
                    chunk=r.chunk,
                    score=r.score,
                    bm25_rank=i,
                    semantic_rank=None,
                    matched_terms=r.matched_terms,
                    source="bm25",
                )
                for i, r in enumerate(bm25_results[:top_k])
            ]

        # Semantic retrieval
        sem_results = self.semantic.search(
            query, top_k=fetch_k, filter_doc=filter_doc, filter_type=filter_type
        )

        # Build rank maps (chunk_id → rank)
        bm25_ranks: dict[str, tuple[int, ScoredChunk]] = {}
        for rank, r in enumerate(bm25_results):
            bm25_ranks[r.chunk.chunk_id] = (rank, r)

        sem_ranks: dict[str, int] = {}
        for rank, r in enumerate(sem_results):
            sem_ranks[r.chunk.chunk_id] = rank

        # Reciprocal Rank Fusion
        all_chunk_ids = set(bm25_ranks.keys()) | set(sem_ranks.keys())
        fused_scores: dict[str, float] = {}

        for chunk_id in all_chunk_ids:
            score = 0.0
            if chunk_id in bm25_ranks:
                rank = bm25_ranks[chunk_id][0]
                score += bm25_weight * (1.0 / (RRF_K + rank))
            if chunk_id in sem_ranks:
                rank = sem_ranks[chunk_id]
                score += semantic_weight * (1.0 / (RRF_K + rank))
            fused_scores[chunk_id] = score

        # Sort by fused score
        sorted_ids = sorted(fused_scores.keys(), key=lambda x: -fused_scores[x])

        # Build results
        results = []
        # We need chunk objects — build a lookup
        chunk_lookup: dict[str, Chunk] = {}
        matched_lookup: dict[str, list[str]] = {}
        for r in bm25_results:
            chunk_lookup[r.chunk.chunk_id] = r.chunk
            matched_lookup[r.chunk.chunk_id] = r.matched_terms
        for r in sem_results:
            chunk_lookup[r.chunk.chunk_id] = r.chunk

        for chunk_id in sorted_ids[:top_k]:
            bm25_rank = bm25_ranks[chunk_id][0] if chunk_id in bm25_ranks else None
            semantic_rank = sem_ranks.get(chunk_id)

            # Determine source label
            if bm25_rank is not None and semantic_rank is not None:
                source = "hybrid"
            elif bm25_rank is not None:
                source = "bm25"
            else:
                source = "semantic"

            results.append(
                RetrievalResult(
                    chunk=chunk_lookup[chunk_id],
                    score=fused_scores[chunk_id],
                    bm25_rank=bm25_rank,
                    semantic_rank=semantic_rank,
                    matched_terms=matched_lookup.get(chunk_id, []),
                    source=source,
                )
            )

        return results

    def search_formatted(self, query: str, top_k: int = 5, **kwargs) -> str:
        """Search and return a formatted string for agent consumption."""
        results = self.search(query, top_k=top_k, **kwargs)

        if not results:
            return f"No results found for: {query}"

        parts = [f"## Search Results for: {query}\n"]
        parts.append(f"Mode: {self.mode} | Results: {len(results)}\n")

        for i, r in enumerate(results, 1):
            parts.append(f"### Result {i} (score: {r.score:.3f}, via {r.source})")
            parts.append(r.chunk.context_header)
            if r.matched_terms:
                parts.append(f"Matched: {', '.join(r.matched_terms[:10])}")
            parts.append("```")
            # Truncate very long content
            content = r.chunk.content
            if len(content) > 1500:
                content = content[:1500] + "\n... [truncated]"
            parts.append(content)
            parts.append("```\n")

        return "\n".join(parts)

    # ─── Persistence ─────────────────────────────────────────────

    def save_index(self, directory: str | Path = ".beacon") -> None:
        """Save the full index to disk."""
        directory = Path(directory)
        directory.mkdir(parents=True, exist_ok=True)

        self.bm25.save(directory / "bm25_index.json")

        if self.semantic:
            self.semantic.save(directory / "semantic")

        logger.info(f"Index saved to {directory}")

    def load_index(self, directory: str | Path = ".beacon") -> None:
        """Load a saved index from disk."""
        directory = Path(directory)

        bm25_path = directory / "bm25_index.json"
        if bm25_path.exists():
            self.bm25.load(bm25_path)
            self.chunks = self.bm25.chunks
            logger.info(f"BM25 index loaded: {len(self.chunks)} chunks")

        sem_path = directory / "semantic"
        if self._use_semantic and (sem_path / "meta.json").exists():
            try:
                self.semantic = sem.SemanticIndex()
                self.semantic.load(sem_path)
                logger.info("Semantic index loaded")
            except Exception as e:
                logger.warning(f"Could not load semantic index: {e}")
