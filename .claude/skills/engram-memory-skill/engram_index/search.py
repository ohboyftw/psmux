"""
BM25 + semantic hybrid search for Engram memories.

Adapted from Beacon's search patterns. Supports:
- BM25 keyword search (zero deps)
- ONNX semantic search (optional, requires numpy/onnxruntime/tokenizers)
- Reciprocal Rank Fusion (RRF) for hybrid results
- Incremental index updates (add without full rebuild)
"""

from __future__ import annotations

import logging
import math
import re
from collections import defaultdict
from dataclasses import dataclass
from typing import Optional

from .store import MemoryRecord

logger = logging.getLogger("engram.search")

# ─── BM25 Parameters ────────────────────────────────────────────

K1 = 1.5              # Term frequency saturation
B = 0.75              # Length normalization
TYPE_BOOST = 1.5       # Memory type tokens get boosted
BIGRAM_BOOST = 1.3     # Adjacent term pair boost

# RRF fusion constant
RRF_K = 60

# Tokenization
TOKEN_RE = re.compile(r"[a-zA-Z0-9][\w\-\.]*[a-zA-Z0-9]|[a-zA-Z0-9]")

STOP_WORDS = {
    "the", "a", "an", "is", "it", "in", "on", "at", "to", "for",
    "of", "and", "or", "we", "i", "he", "she", "they", "my", "our",
    "this", "that", "with", "from", "by", "as", "be", "was", "were",
    "are", "been", "has", "had", "have", "do", "did", "will", "would",
    "could", "should", "may", "can", "not", "but", "if", "so", "no",
}


def tokenize(text: str) -> list[str]:
    """Tokenize text into lowercase word tokens, removing stop words."""
    tokens = TOKEN_RE.findall(text.lower())
    return [t for t in tokens if len(t) >= 2 and t not in STOP_WORDS]


# ─── Search Result ──────────────────────────────────────────────


@dataclass
class SearchResult:
    """A single search result with scoring metadata."""
    record: MemoryRecord
    score: float
    source: str = "bm25"  # "bm25", "semantic", "hybrid"
    bm25_rank: int = 0
    semantic_rank: int = 0


# ─── BM25 Index ─────────────────────────────────────────────────


class BM25Index:
    """
    In-memory BM25 inverted index over memory records.

    Supports incremental additions — no full rebuild needed for new records.
    """

    def __init__(self):
        # doc_id -> list of (token, weight) pairs
        self._doc_tokens: dict[str, list[tuple[str, float]]] = {}
        # token -> set of doc_ids
        self._inverted: dict[str, set[str]] = defaultdict(set)
        # doc_id -> weighted token frequency
        self._doc_freqs: dict[str, dict[str, float]] = {}
        # doc_id -> document length (weighted token count)
        self._doc_lengths: dict[str, float] = {}
        # Total documents
        self._total_docs: int = 0
        self._avg_dl: float = 0.0

    def add_record(self, record: MemoryRecord) -> None:
        """Add a single record to the index (incremental)."""
        doc_id = record.id

        if doc_id in self._doc_tokens:
            return  # Already indexed

        # Build weighted token bag
        tokens = self._weighted_tokens(record)
        self._doc_tokens[doc_id] = tokens

        # Build frequency map
        freq_map: dict[str, float] = defaultdict(float)
        for token, weight in tokens:
            freq_map[token] += weight
            self._inverted[token].add(doc_id)

        self._doc_freqs[doc_id] = dict(freq_map)
        self._doc_lengths[doc_id] = sum(freq_map.values())

        # Update stats
        self._total_docs += 1
        total_length = sum(self._doc_lengths.values())
        self._avg_dl = total_length / max(self._total_docs, 1)

    def _weighted_tokens(self, record: MemoryRecord) -> list[tuple[str, float]]:
        """Build weighted token bag from a memory record."""
        tokens: list[tuple[str, float]] = []

        # Content tokens (base weight 1.0)
        content_toks = tokenize(record.content)
        for t in content_toks:
            tokens.append((t, 1.0))

        # Memory type as boosted token
        type_toks = tokenize(record.memory_type)
        for t in type_toks:
            tokens.append((t, TYPE_BOOST))

        # Tags as boosted tokens
        for tag in record.tags:
            for t in tokenize(tag):
                tokens.append((t, TYPE_BOOST))

        # Bigrams for phrase matching
        for i in range(len(content_toks) - 1):
            bigram = f"{content_toks[i]}_{content_toks[i+1]}"
            tokens.append((bigram, BIGRAM_BOOST))

        return tokens

    def search(self, query: str, top_k: int = 10) -> list[tuple[str, float]]:
        """
        BM25 search. Returns list of (doc_id, score) sorted by score desc.
        """
        if self._total_docs == 0:
            return []

        query_tokens = tokenize(query)
        if not query_tokens:
            return []

        # Add query bigrams
        query_bigrams = []
        for i in range(len(query_tokens) - 1):
            query_bigrams.append(f"{query_tokens[i]}_{query_tokens[i+1]}")

        all_query_terms = query_tokens + query_bigrams

        scores: dict[str, float] = defaultdict(float)

        for term in all_query_terms:
            if term not in self._inverted:
                continue

            doc_set = self._inverted[term]
            n = len(doc_set)
            idf = math.log((self._total_docs - n + 0.5) / (n + 0.5) + 1)

            for doc_id in doc_set:
                tf = self._doc_freqs[doc_id].get(term, 0)
                dl = self._doc_lengths[doc_id]
                numerator = tf * (K1 + 1)
                denominator = tf + K1 * (1 - B + B * dl / max(self._avg_dl, 1e-6))
                scores[doc_id] += idf * numerator / denominator

        ranked = sorted(scores.items(), key=lambda x: -x[1])
        return ranked[:top_k]

    def remove_record(self, doc_id: str) -> None:
        """Remove a record from the index."""
        if doc_id not in self._doc_tokens:
            return

        for token, _ in self._doc_tokens[doc_id]:
            self._inverted[token].discard(doc_id)
            if not self._inverted[token]:
                del self._inverted[token]

        del self._doc_tokens[doc_id]
        del self._doc_freqs[doc_id]
        del self._doc_lengths[doc_id]

        self._total_docs -= 1
        if self._total_docs > 0:
            total_length = sum(self._doc_lengths.values())
            self._avg_dl = total_length / self._total_docs
        else:
            self._avg_dl = 0.0

    @property
    def size(self) -> int:
        return self._total_docs


# ─── Hybrid Search Index ────────────────────────────────────────


class MemorySearchIndex:
    """
    Hybrid BM25 + semantic search over memory records.

    Falls back to BM25-only if ONNX embeddings are unavailable.
    """

    def __init__(self, use_semantic: bool = True):
        self.bm25 = BM25Index()
        self._use_semantic = use_semantic

        # Semantic components (lazy-loaded)
        self._embeddings_model = None
        self._embedding_matrix = None  # numpy array, shape (N, 384)
        self._embedding_ids: list[str] = []  # doc_id for each row
        self._semantic_ready = False

        # Record cache for returning full records
        self._records: dict[str, MemoryRecord] = {}

        if use_semantic:
            self._init_semantic()

    def _init_semantic(self):
        """Try to initialize semantic search."""
        try:
            from .embeddings import EmbeddingModel, is_available, EMBEDDING_DIM
            import numpy as np

            if not is_available():
                logger.info("ONNX model not available — using BM25-only search")
                self._use_semantic = False
                return

            self._embeddings_model = EmbeddingModel()
            self._embedding_matrix = np.empty((0, EMBEDDING_DIM), dtype=np.float32)
            self._embedding_ids = []
            self._semantic_ready = True
            logger.info("Semantic search initialized")

        except Exception as e:
            logger.info(f"Semantic search unavailable: {e}")
            self._use_semantic = False

    def add_record(self, record: MemoryRecord) -> None:
        """Add a record to both BM25 and semantic indexes."""
        self._records[record.id] = record

        # BM25
        self.bm25.add_record(record)

        # Semantic
        if self._semantic_ready and self._embeddings_model is not None:
            try:
                import numpy as np
                embedding = self._embeddings_model.encode_single(record.content)
                self._embedding_matrix = np.vstack([
                    self._embedding_matrix, embedding.reshape(1, -1)
                ])
                self._embedding_ids.append(record.id)
            except Exception as e:
                logger.debug(f"Semantic indexing failed for {record.id[:8]}: {e}")

    def build_from_records(self, records: list[MemoryRecord]) -> None:
        """Build index from a list of records (cold start / rebuild)."""
        self._records.clear()
        self.bm25 = BM25Index()

        for record in records:
            self._records[record.id] = record
            self.bm25.add_record(record)

        # Batch encode for semantic
        if self._semantic_ready and self._embeddings_model is not None and records:
            try:
                import numpy as np
                texts = [r.content for r in records]
                self._embedding_matrix = self._embeddings_model.encode(texts)
                self._embedding_ids = [r.id for r in records]
                logger.info(f"Semantic index built: {len(records)} records")
            except Exception as e:
                logger.warning(f"Batch semantic encoding failed: {e}")
                self._embedding_matrix = None
                self._embedding_ids = []

    def search(
        self,
        query: str,
        top_k: int = 10,
        scope_filter: Optional[list[str]] = None,
        type_filter: Optional[list[str]] = None,
        since: Optional[str] = None,
        until: Optional[str] = None,
    ) -> list[SearchResult]:
        """
        Hybrid search with optional filters.

        Uses RRF (Reciprocal Rank Fusion) when both BM25 and semantic
        results are available. Falls back to BM25-only otherwise.
        """
        # Fetch more candidates for fusion
        fetch_k = top_k * 3

        # BM25 search
        bm25_results = self.bm25.search(query, top_k=fetch_k)
        bm25_ranks: dict[str, tuple[int, float]] = {}
        for rank, (doc_id, score) in enumerate(bm25_results):
            bm25_ranks[doc_id] = (rank, score)

        # Semantic search
        sem_ranks: dict[str, int] = {}
        if self._semantic_ready and self._embeddings_model is not None and len(self._embedding_ids) > 0:
            try:
                import numpy as np
                query_emb = self._embeddings_model.encode_single(query)
                # Cosine similarity (embeddings are normalized)
                similarities = self._embedding_matrix @ query_emb
                top_indices = np.argsort(-similarities)[:fetch_k]
                for rank, idx in enumerate(top_indices):
                    if similarities[idx] > 0.0:
                        doc_id = self._embedding_ids[idx]
                        sem_ranks[doc_id] = rank
            except Exception as e:
                logger.debug(f"Semantic search failed: {e}")

        # Fuse results
        if sem_ranks:
            # RRF fusion
            all_ids = set(bm25_ranks.keys()) | set(sem_ranks.keys())
            fused: dict[str, float] = {}

            bm25_weight = 0.5
            semantic_weight = 0.5

            for doc_id in all_ids:
                score = 0.0
                if doc_id in bm25_ranks:
                    rank = bm25_ranks[doc_id][0]
                    score += bm25_weight * (1.0 / (RRF_K + rank))
                if doc_id in sem_ranks:
                    rank = sem_ranks[doc_id]
                    score += semantic_weight * (1.0 / (RRF_K + rank))
                fused[doc_id] = score

            ranked_ids = sorted(fused.items(), key=lambda x: -x[1])
            source = "hybrid"
        else:
            # BM25-only
            ranked_ids = [(doc_id, score) for doc_id, score in bm25_results]
            source = "bm25"

        # Build results with filters
        results: list[SearchResult] = []
        for doc_id, score in ranked_ids:
            if doc_id not in self._records:
                continue

            record = self._records[doc_id]

            # Apply filters
            if scope_filter and record.scope not in scope_filter:
                continue
            if type_filter and record.memory_type not in type_filter:
                continue
            if since and record.created_at < since:
                continue
            if until and record.created_at > until:
                continue

            results.append(SearchResult(
                record=record,
                score=score,
                source=source,
                bm25_rank=bm25_ranks.get(doc_id, (999,))[0],
                semantic_rank=sem_ranks.get(doc_id, 999),
            ))

            if len(results) >= top_k:
                break

        return results

    def remove_record(self, doc_id: str) -> None:
        """Remove a record from all indexes."""
        self.bm25.remove_record(doc_id)
        self._records.pop(doc_id, None)

        if self._semantic_ready and doc_id in self._embedding_ids:
            try:
                import numpy as np
                idx = self._embedding_ids.index(doc_id)
                self._embedding_matrix = np.delete(self._embedding_matrix, idx, axis=0)
                self._embedding_ids.pop(idx)
            except (ValueError, Exception):
                pass

    @property
    def size(self) -> int:
        return self.bm25.size

    @property
    def has_semantic(self) -> bool:
        return self._semantic_ready and len(self._embedding_ids) > 0
